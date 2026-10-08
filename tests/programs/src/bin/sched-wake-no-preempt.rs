//! A timeout only wakes (kernel/scheduling.md, "Preemption points"): it never takes the CPU from
//! the thread running.
//!
//! A sleeper and four spinners, in budgets of equal weight, so every hart runs a spinner on up to
//! four harts. The sleeper sleeps 300 µs, twenty times; each time it blocks, a spinner is picked
//! and starts a 1 ms slice. The timeout falls inside a running slice, and the spinner its hart runs
//! keeps the hart to that slice's end: the trace post-check establishes that event order, hart by
//! hart. The shortest delay is noted: on one hart it is past the slice's end, over 400 µs; on
//! several, another hart's slice may end sooner.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

/// The sleeper's nap.
const NAP_US: u64 = 300;
/// The spinners: one for each hart, up to four.
const SPINNERS: usize = 4;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("wake");
    let sleeper = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let w = b.start(sleeper, Role::WakeDelay, &[NAP_US, 20], &[]);
    for _ in 0..SPINNERS {
        let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(spinner, Role::Spin, &[], &[]);
    }
    b.go(50_000, 400_000);
    let r = b.collect(1 + SPINNERS);
    b.note(format_args!("the shortest delay from a nap's start to the sleeper's run was {} µs", r[w]));
    b.finish("SCHED-WAKE-NO-PREEMPT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("wake", info) }
