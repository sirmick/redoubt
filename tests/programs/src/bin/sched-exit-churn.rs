//! Exiting on the CPU is charged (R12; accounting at the trap boundary): an attacker whose
//! threads, or child processes, each run nearly a slice and then exit (or fault) gets at most
//! its weight against an equal-weight victim.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("exit-churn");
    let run = (SLICE_US - SLICE_US / 10) * b.tpu;
    // (role, fault?, name, what)
    for (role, fault, name, what) in [
        (Role::ThreadChurn, 0, "threads-exit", "threads that exit"),
        (Role::ProcessChurn, 0, "processes-exit", "processes that exit"),
        (Role::ProcessChurn, 1, "processes-fault", "processes that fault"),
    ] {
        let attacker = b.budget(rd::USERS, 100, 2, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(attacker, role, &[run, fault], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        // The post-check judges the victim's share net of the checked build's audits (each
        // process's start and end runs one), which a release build does not run.
        let vs = b.judged_share(name, counts[v], window, (500 - TOL, 1000));
        b.note(format_args!(
            "{}: the victim got {} of 1000: gross, audits included; net in the post-check",
            what, vs
        ));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-EXIT-CHURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("exit-churn", info) }
