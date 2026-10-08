//! The lead follows the weight (kernel/scheduling.md, "The lead follows the weight"): U (weight
//! 1000) carves 999 away to an empty child, runs on the 1 it kept until 9/10 into its slice, and
//! destroys the child, so its weight comes back. What it ran is restated at the restored weight,
//! so from then on it gets its share against an equal victim V, judged by the post-check on the
//! kernel's charges net of lock waits (`HART-SHARE`). Kept at weight 1, its lead would
//! hold it off the CPU for seconds. U sleeps first, so that the carve begins a slice and the
//! destruction falls inside it: requeued at weight 1 before it destroys the child, U would be held
//! off the CPU past the window by its pass, under the rule or not.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd;
use test_programs::sched::{Bench, Role, join, mark};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;
/// The weight of the empty child that marks U's budget in the kernel's trace (U's own carve is
/// 999).
const MARK: u32 = 2;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("carve-return");
    // Room for the carve's own page.
    let u = b.budget(rd::USERS, 1000, 1, rd::FOREVER);
    let v = b.budget(rd::USERS, 1000, 1, rd::FOREVER);
    mark(u, MARK);
    let ui = b.start(u, Role::CarveSpin, &[999], &[u]);
    let vi = b.start(v, Role::Spin, &[], &[]);
    let (start, end) = b.go(50_000, WINDOW);
    // U reports create and return offsets (µs from its wake), absolute return, then count.
    let w = b.collect_words(5);
    let (create_us, return_us) = (join(w[ui][0][0], w[ui][0][1]), join(w[ui][1][0], w[ui][1][1]));
    let returned = join(w[ui][2][0], w[ui][2][1]);
    let (uc, vc) = (join(w[ui][3][0], w[ui][3][1]), join(w[vi][0][0], w[vi][0][1]));
    b.check(create_us > 0 && return_us >= create_us, format_args!("the create and return were measured"));
    let _ = writeln!(test_programs::console::Console, "CARVE-OBS {create_us} {return_us} {returned}");
    // U's share from its carve's return, of the kernel's charges: the post-check's.
    b.hart_share("u-after-return", (returned, end), (TOL, "+"), (MARK, 1), &[(1000, 1)]);
    b.note(format_args!(
        "after its carve returned, {} µs into the window, U counted {} of 1000; V {} over the window",
        returned - start,
        b.share(uc, end - returned),
        b.share(vc, end - start)
    ));
    b.finish("SCHED-CARVE-RETURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("carve-return", info) }
