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
    // (role, fault?, what)
    for (role, fault, what) in [
        (Role::ThreadChurn, 0, "threads that exit"),
        (Role::ProcessChurn, 0, "processes that exit"),
        (Role::ProcessChurn, 1, "processes that fault"),
    ] {
        let attacker = b.budget(rd::USERS, 100, 2, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(attacker, role, &[run, fault], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let (start, end) = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        let vs = b.share(counts[v], end - start);
        b.check(vs + TOL >= 500, format_args!("{}: the victim got {} of 1000", what, vs));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-EXIT-CHURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("exit-churn", info) }
