//! Timer interrupts an attacker arms cost the attacker, not a victim (R12, billing): 30 threads
//! sleeping a microsecond each, staggered and re-armed, and a variant that also creates budgets
//! whose deadlines fall a microsecond apart, leave an equal-weight victim at least half.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("timer-flood");
    for (leases, name, what) in [
        (0u64, "sleepers", "30 sleepers, 1 us apart"),
        (1, "sleepers-and-deadlines", "30 sleepers and 64 staggered budget deadlines"),
    ] {
        // Room for 30 thread stacks and 64 budgets of its own.
        let attacker = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(attacker, Role::TimerFlood, &[1, 1, 30, leases], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        // The post-check judges the victim's share net of the checked build's audits (each
        // deadline's destruction runs one), which a release build does not run.
        let vs = b.judged_share(name, counts[v], window, (500 - TOL, 1000));
        b.note(format_args!(
            "{}: the victim got {} of 1000: gross, audits included; net in the post-check",
            what, vs
        ));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-TIMER-FLOOD")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("timer-flood", info) }
