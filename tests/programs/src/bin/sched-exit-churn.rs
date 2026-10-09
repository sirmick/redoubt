//! Exiting on the CPU is charged (R12; accounting at the trap boundary): an attacker whose
//! threads, or child processes, each run nearly a slice and then exit (or fault) gets at most
//! its weight against an equal-weight victim: the victim keeps at least its share of the harts,
//! judged by the post-check on the kernel's charges net of lock waits (`HART-SHARE`). Against
//! threads that exit it is judged at one hart and reported at more: the attacker's exits hold the
//! kernel lock, and the victim's hart takes most of the waits (kernel/scheduling.md, "Residual
//! risks").

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US, mark};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("exit-churn");
    let run = (SLICE_US - SLICE_US / 10) * b.tpu;
    // (role, fault?, name, what, the victim's mark, the harts it is judged at)
    for (role, fault, name, what, m, at) in [
        (Role::ThreadChurn, 0, "threads-exit", "threads that exit", 2, "+@1"),
        (Role::ProcessChurn, 0, "processes-exit", "processes that exit", 3, "+"),
        (Role::ProcessChurn, 1, "processes-fault", "processes that fault", 4, "+"),
    ] {
        let attacker = b.budget(rd::USERS, 100, 2, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        mark(victim, m);
        b.start(attacker, role, &[run, fault], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        // The post-check judges the victim's share of the kernel's charges, which bill the
        // checked build's audits (each process's start and end runs one) to no one. The attacker
        // runs at most two threads.
        b.hart_share(name, window, (TOL, at), (m, 1), &[(100, 2)]);
        b.note(format_args!(
            "{}: the victim counted {} of 1000 of the window",
            what,
            b.share(counts[v], window.1 - window.0)
        ));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-EXIT-CHURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("exit-churn", info) }
