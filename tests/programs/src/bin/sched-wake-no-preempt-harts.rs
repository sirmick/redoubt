//! A timeout only wakes, on several harts (kernel/scheduling.md, "Preemption points"): the kernel
//! entry that answers it, on whatever hart, takes no hart from the thread it entered from.
//!
//! A sleeper and three spinners, in budgets of equal weight. The sleeper sleeps 300 µs, sixty
//! times. Each spinner enters the kernel with `time_now` every 100 µs, so some timeouts are
//! answered at another budget's call while that budget's slice runs; most come at a timer
//! interrupt that ends a slice anyway, since a spinner waiting for the kernel lock at its call
//! takes the pending timer late. The trace post-check judges each wake at the entry that made it,
//! and needs at least five that could have preempted.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

/// The sleeper's nap, and how many.
const NAP_US: u64 = 300;
const NAPS: u64 = 60;
/// How often each spinner enters the kernel.
const CALL_US: u64 = 100;
/// The spinners: one more than two harts can run at once.
const SPINNERS: usize = 3;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("wake");
    let sleeper = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let w = b.start(sleeper, Role::WakeDelay, &[NAP_US, NAPS], &[]);
    for _ in 0..SPINNERS {
        let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(spinner, Role::SpinCalling, &[CALL_US], &[]);
    }
    b.go(50_000, 400_000);
    let r = b.collect(1 + SPINNERS);
    b.note(format_args!("the shortest delay from a nap's start to the sleeper's run was {} µs", r[w]));
    b.finish("SCHED-WAKE-NO-PREEMPT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("wake", info) }
