//! A timeout only wakes, on several harts (kernel/scheduling.md, "Preemption points"): the kernel
//! entry that answers it, on whatever hart, takes no hart from the thread it entered from.
//!
//! A sleeper and three spinners, in budgets of equal weight. The sleeper sleeps 120 times, the
//! k-th nap 130 + 17k mod 1000 µs, so the naps' ends move through the slices' phases. Each spinner
//! enters the kernel with `time_now` every 37 µs, so some timeouts are answered at another
//! budget's call while that budget's slice runs, some at a timer interrupt in the middle of a
//! slice, and the rest at a timer interrupt that ends a slice anyway. The trace post-check judges
//! each wake at the entry that made it, and needs at least five mid-slice.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

/// The sleeper's shortest nap, how many, and the step each next nap adds (mod 1000 µs).
const NAP_US: u64 = 130;
const NAPS: u64 = 120;
const NAP_STEP_US: u64 = 17;
/// How often each spinner enters the kernel: often enough that a call, not the slice's end, answers
/// some timeouts whatever the slice's phase (the case's comment).
const CALL_US: u64 = 37;
/// The spinners: one more than two harts can run at once.
const SPINNERS: usize = 3;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("wake");
    let sleeper = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let w = b.start(sleeper, Role::WakeDelay, &[NAP_US, NAPS, NAP_STEP_US], &[]);
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
