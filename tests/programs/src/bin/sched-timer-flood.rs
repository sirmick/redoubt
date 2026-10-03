//! Timer interrupts an attacker arms cost the attacker, not a victim (R12, billing): 30 threads
//! sleeping a microsecond each, staggered and re-armed, a variant that also creates budgets whose
//! deadlines fall a microsecond apart, and two threads whose waits, timed out 15 ms ahead, a
//! sibling ends at once, so the timer they armed comes early in the victim's slice, leave an
//! equal-weight victim at least half. (A wait ends early only while the attacker runs, and a
//! block hands the CPU on, so the timeouts outlive a slice: hundreds of microseconds ahead, every
//! wait timed out for real.)

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("timer-flood");
    for (params, name, what) in [
        ([1u64, 1, 30, 0], "sleepers", "30 sleepers, 1 us apart"),
        ([1, 1, 30, 1], "sleepers-and-deadlines", "30 sleepers and 64 staggered budget deadlines"),
        (
            [15_000, 1_300, 3, 2],
            "cancelled-waits",
            "2 waits answered at once, timeouts 15 ms ahead, 1.3 ms apart",
        ),
    ] {
        // Room for 30 thread stacks and 64 budgets of its own.
        let attacker = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let a = b.start(attacker, Role::TimerFlood, &params, &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        // The waits must have ended early: answered at once.
        if params[3] == 2 {
            b.check(counts[a] > 0, format_args!("{}: {} waits answered", what, counts[a]));
        }
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
