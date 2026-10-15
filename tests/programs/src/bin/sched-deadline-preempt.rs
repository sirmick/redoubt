//! Another budget's deadline preempts whatever runs (kernel/scheduling.md, "Preemption points"):
//! the destruction lifts passes and removes budgets, so the entry that finds a deadline due
//! deschedules its runner and the queue is picked again, mid-slice or not.
//!
//! A spinner, and a creator in an equal-weight budget that makes one empty weight-0 budget at a
//! time under its own, due 1.3 ms ahead, then counts for a millisecond and makes the next: the
//! deadlines fall at every phase of the two runners' slices, and whichever runs is interrupted.
//! The verdict is the post-check's, from the trace: `deadline_preempts=N` requires every timer
//! interrupt from user mode that destroyed a budget other than the one it interrupted to return
//! as a preemption, the runner requeued and the queue picked again, and N of them.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

/// Each budget's deadline, µs ahead of its creation: not a multiple of the slice, so the
/// deadlines walk through the slices' phases.
const AHEAD_US: u64 = 1_300;
/// The window, µs.
const WINDOW_US: u64 = 2_500_000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("deadline-preempt");
    // Room for the creator's own process and the budgets it makes, one at a time.
    let creator = b.budget(rd::USERS, 100, 2, rd::FOREVER);
    let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let c = b.start(creator, Role::DeadlineFlood, &[1, AHEAD_US], &[creator]);
    let s = b.start(spinner, Role::Spin, &[], &[]);
    // The checked build's audits, after each destruction and at the marks, take most of the
    // window: 2.5 s holds some eighty deadlines.
    let (start, end) = b.go(50_000, WINDOW_US);
    let counts = b.collect(2);
    b.note(format_args!(
        "the spinner counted {} and the creator {} of 1000 of the window; which deadlines preempted is the post-check's",
        b.share(counts[s], end - start),
        b.share(counts[c], end - start)
    ));
    rd::destroy(creator).unwrap();
    rd::destroy(spinner).unwrap();
    b.finish("SCHED-DEADLINE-PREEMPT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("deadline-preempt", info) }
