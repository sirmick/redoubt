//! A timeout only wakes (kernel/scheduling.md, "Preemption points"): it never takes the CPU from
//! the thread running.
//!
//! A sleeper and a spinner, in budgets of equal weight. The sleeper sleeps 300 µs, twenty times;
//! each time it blocks, the spinner is picked and starts a 1 ms slice. The timeout should fall
//! inside that slice, so the sleeper runs only after its end. The trace post-check establishes
//! that event order; the shortest measured delay must also exceed 400 µs.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

/// The sleeper's nap, and the least delay after it that shows no preemption.
const NAP_US: u64 = 300;
const LEAST_US: u64 = 400;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("wake");
    let sleeper = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let w = b.start(sleeper, Role::WakeDelay, &[NAP_US, 20], &[]);
    b.start(spinner, Role::Spin, &[], &[]);
    b.go(50_000, 400_000);
    let r = b.collect(2);
    b.check(
        r[w] >= LEAST_US,
        format_args!(
            "a timeout mid-slice waited for the slice's end: the shortest delay was {} µs (at least {})",
            r[w], LEAST_US
        ),
    );
    b.finish("SCHED-WAKE-NO-PREEMPT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("wake", info) }
