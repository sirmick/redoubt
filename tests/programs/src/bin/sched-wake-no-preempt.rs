//! A timeout only wakes (WP-K5; OWNER DECISION 2): it never takes the CPU from the thread running.
//!
//! A sleeper and a spinner, in budgets of equal weight. The sleeper sleeps 3 ms, twenty times;
//! each time it blocks, the spinner is picked and starts a 10 ms slice, so the sleeper's timeout
//! falls about 3 ms into that slice. The sleeper then runs only when the slice ends, about 7 ms
//! after its timeout. A kernel whose timeout preempted would run it within microseconds. Asserted:
//! even the shortest of the twenty delays is over 4 ms.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

/// The sleeper's nap, and the least delay after it that shows no preemption.
const NAP_US: u64 = 3_000;
const LEAST_US: u64 = 4_000;

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
