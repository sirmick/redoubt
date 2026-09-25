//! A budget's destruction is charged to its parent at the weight the parent has once the child is
//! gone (WP-K5; K5-code-review-4 D1, the orchestrator's ruling): the top's carve returns before any
//! destruction work is billed.
//!
//! P (weight 1000) runs one process and has carved 990 of that weight to its child C, which holds
//! four processes (blocked; they do no work that would move up with C's debt). As its window
//! opens, P's process destroys C, then counts beside a victim V of weight 1000, noting how long
//! the call took to come back and the longest it then goes without the CPU. R10's work (killing C's
//! processes, sweeping, freeing: tens of milliseconds of kernel time here) is P's syscall, billed to P. At
//! P's restored weight, V then catches up about that long, and P waits about that long once. At the 10 P kept
//! while C held the rest, it would wait a hundred times that, past the end of the window.
//!
//! Asserted: from P's `budget_destroy` to its running again, and any later wait, at most twice the
//! destruction's cost (the work, then V catching up its charge) plus four slices (slice rounding,
//! and P's entry into the call, which runs before the carve is back and so is charged at the weight
//! P kept, as KERNEL-SPEC R12 states). The cost is
//! measured first, on a copy this program destroys itself with nothing else to run (P cannot tell
//! its kernel time from its wait after it).

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US};
use test_programs::spawn;

const WINDOW_US: u64 = 600_000;

/// C's processes: blocked for good.
extern "C" fn blocked(_: usize) -> ! {
    loop {
        let _ = rd::receive(None, rd::FOREVER, 0);
    }
}

/// Under parent `p` (weight 1000), a child C holding 990 of it and four blocked processes.
fn child_of(b: &Bench, p: u32) -> u32 {
    let per = b.image().pages() as u64 + 96;
    let c = rd::create(p, &rd::spec(per * 4 + 64, 4, 990)).expect("C");
    for _ in 0..4 {
        spawn::spawn(b.image(), c, b.exit_endpoint(), blocked as *const () as usize, &[], &[])
            .expect("C's process");
    }
    c
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("destroy-billing");
    // What destroying C costs, with nothing else to run: this program does it, on a copy.
    let cp = b.budget(rd::USERS, 1000, 6, rd::FOREVER);
    let cc = child_of(&b, cp);
    let _ = rd::receive(None, 10_000, 0);
    let before = b.now_us();
    rd::destroy(cc).unwrap();
    let cost = b.now_us() - before;
    rd::destroy(cp).unwrap();
    // The real one.
    let p = b.budget(rd::USERS, 1000, 6, rd::FOREVER);
    let v = b.budget(rd::USERS, 1000, 1, rd::FOREVER);
    let c = child_of(&b, p);
    // V spins from well before P's window, so the floor has passed what P's own start-up ran up
    // while carved down (charged at the weight it kept: KERNEL-SPEC R12, accepted), and P wakes
    // at the floor.
    b.start(v, Role::Spin, &[], &[]);
    b.go(10_000, 100_000 + WINDOW_US);
    let pi = b.start(p, Role::DestroyThenCount, &[], &[c]);
    b.go(100_000, WINDOW_US);
    let w = b.collect_words(3);
    let (took, gap) = (w[pi][0][0] as u64, w[pi][0][1] as u64);
    // The destruction itself, then V catching up its charge (at P's restored weight), then slice
    // rounding; and two slices for the few hundred microseconds P runs, entering the call, before
    // the carve is back (charged at the weight it kept: KERNEL-SPEC R12, accepted). Billed at the
    // kept weight, the destruction alone would keep P off the CPU for seconds.
    let bound = 2 * cost + 4 * SLICE_US;
    b.check(
        took <= bound && gap <= bound,
        format_args!(
            "P (free weight 10 while C held 990) destroyed C (alone it takes {} µs); from the call to running again {} µs, the longest wait after {} µs (bound {})",
            cost, took, gap, bound
        ),
    );
    b.finish("SCHED-DESTROY-BILLING")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("destroy-billing", info) }
