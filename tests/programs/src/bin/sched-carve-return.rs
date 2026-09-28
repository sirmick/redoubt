//! The lead follows the weight (kernel/scheduling.md, "The lead follows the weight"): U (weight
//! 1000) carves 999 away to an empty child, runs 9 ms on the 1 it kept, and destroys the child,
//! so its weight comes back. What it ran is restated at the restored weight, so from then on it
//! gets its half against an equal victim V. Kept at weight 1, its lead would hold it off the CPU
//! for seconds.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
/// U's carve and its 9 ms run, and a slice of V's before them, left out of U's share.
const CARVED: u64 = 20_000;
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
    let counts = b.collect(2);
    let us = b.share(counts[ui], end - start - CARVED);
    let vs = b.share(counts[vi], end - start);
    b.note(format_args!("after its carve returned, U got {} of 1000; V {} over the window", us, vs));
    b.check(us + TOL >= 500, format_args!("U got its half back after its carve returned: {} of 1000", us));
    b.finish("SCHED-CARVE-RETURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("carve-return", info) }
