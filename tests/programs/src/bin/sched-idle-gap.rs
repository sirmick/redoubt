//! A sleeper cannot bank credit across an idle gap (R12, the floor): A sleeps while B
//! runs alone for two seconds; B then sleeps 5 ms, and A wakes inside that gap, when nothing else
//! is runnable. Afterwards B still gets its share, a third of the harts the three share (each runs
//! one thread). A budget created inside the gap enters at the floor too, and gets at most a third
//! once all three run. Both are the post-check's, of the kernel's charges net of lock waits
//! (`HART-SHARE`); the counts are noted beside.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark, ticks};

const TOL: u64 = 50;
/// The weights of the empty children that mark B's and C's budgets in the kernel's trace.
const B_MARK: u32 = 2;
const C_MARK: u32 = 3;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("idle-gap");
    let tpu = b.tpu;
    // After the gap: long enough that the budget created in the gap (whose process takes a while
    // to start, a copy of this image) competes for a second or more.
    let (alone, gap, after) = (2_000_000u64, 5_000u64, 2_000_000u64);
    let lead = 50_000u64;
    let t0 = ticks() + lead * tpu;
    let (gap_start, gap_end) = (t0 + alone * tpu, t0 + (alone + gap) * tpu);
    let ba = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let bb = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    mark(bb, B_MARK);
    let a = b.start(ba, Role::SpinFrom, &[gap_start + 2_000 * tpu], &[]);
    let bi = b.start(bb, Role::SpinGap, &[gap_start, gap_end], &[]);
    let (start, end) = b.go(lead, alone + gap + after);
    // Inside the gap: a new budget, started with the window's end.
    test_programs::sched::sleep_until(gap_start + 1_000 * tpu, tpu);
    let bc = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    mark(bc, C_MARK);
    let c = b.start(bc, Role::Spin, &[], &[]);
    let c_from = b.now_us();
    b.go(0, end.saturating_sub(c_from));
    let counts = b.collect(3);
    let after_gap = (start + alone + gap, end);
    b.hart_share("b-after-gap", after_gap, (TOL, "+"), (B_MARK, 1), &[(100, 1), (100, 1)]);
    b.hart_share("c-from-gap", (c_from, end), (TOL, "-"), (C_MARK, 1), &[(100, 1), (100, 1)]);
    let window_after = end - after_gap.0;
    b.note(format_args!(
        "after the gap B counted {} of 1000 of the window (A {}; C {})",
        b.share(counts[bi], window_after),
        b.share(counts[a], window_after),
        b.share(counts[c], window_after)
    ));
    b.note(format_args!(
        "the budget created in the gap counted {} of 1000",
        b.share(counts[c], end - c_from)
    ));
    b.finish("SCHED-IDLE-GAP")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("idle-gap", info) }
