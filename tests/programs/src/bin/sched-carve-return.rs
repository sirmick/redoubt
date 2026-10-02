//! The lead follows the weight (kernel/scheduling.md, "The lead follows the weight"): U (weight
//! 1000) carves 999 away to an empty child, runs on the 1 it kept until 9 ms into its slice, and
//! destroys the child, so its weight comes back. What it ran is restated at the restored weight,
//! so from then on it gets its half against an equal victim V. Kept at weight 1, its lead would
//! hold it off the CPU for seconds. U sleeps first, so that the carve begins a slice and the
//! destruction falls inside it: requeued at weight 1 before it destroys the child, U would be held
//! off the CPU past the window by its pass, under the rule or not.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, join};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("carve-return");
    // Room for the carve's own page.
    let u = b.budget(rd::USERS, 1000, 1, rd::FOREVER);
    let v = b.budget(rd::USERS, 1000, 1, rd::FOREVER);
    let ui = b.start(u, Role::CarveSpin, &[999], &[u]);
    let vi = b.start(v, Role::Spin, &[], &[]);
    let (start, end) = b.go(50_000, WINDOW);
    // U reports when its carve returned, then its count; V its count.
    let w = b.collect_words(3);
    let returned = join(w[ui][0][0], w[ui][0][1]);
    let (uc, vc) = (join(w[ui][1][0], w[ui][1][1]), join(w[vi][0][0], w[vi][0][1]));
    // U's share from its carve's return; the post-check judges it net of the checked build's
    // audits inside that window, which a release build does not run.
    let us = b.judged_share("u-after-return", uc, (returned, end), (500 - TOL, 1000));
    let vs = b.share(vc, end - start);
    b.note(format_args!(
        "after its carve returned, {} µs into the window, U got {} of 1000; V {} over the window: gross, audits included; net in the post-check",
        returned - start,
        us,
        vs
    ));
    b.finish("SCHED-CARVE-RETURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("carve-return", info) }
