//! A system server busy on one user's requests delays other users only by its weight (R12;
//! kernel/scheduling.md): user A floods a system-class server (weight 100, 2 ms of work per
//! request) from four threads; users B and C spin. The server's work stays within its weight's
//! share, and B and C each keep theirs (a quarter: A's calls are its own CPU too). The shares are
//! judged by ratio of counts (R12's relative claim): at equal weights the server's work for A is
//! at most what B and C each ran, and B and C ran alike. A's count is calls, not CPU, so it is in
//! no ratio. Each count over what the window would give one loop alone is printed beside, as the
//! useful work the kernel's per-slice time leaves.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("server-busy");
    let work = rd::endpoint_create().unwrap();
    let client = rd::mint_from_handle(work, 9, None).unwrap();
    let server = b.budget(rd::SYSTEM, 100, 1, rd::FOREVER);
    let (ua, ub, uc) = (
        b.budget(rd::USERS, 100, 1, rd::FOREVER),
        b.budget(rd::USERS, 100, 1, rd::FOREVER),
        b.budget(rd::USERS, 100, 1, rd::FOREVER),
    );
    let s = b.start(server, Role::Server, &[2_000], &[work]);
    let a = b.start(ua, Role::Flood, &[0, 0, 4], &[client]);
    let bi = b.start(ub, Role::Spin, &[], &[]);
    let ci = b.start(uc, Role::Spin, &[], &[]);
    let (start, end) = b.go(50_000, WINDOW);
    let counts = b.collect(4);
    let w = end - start;
    let (sc, bc, cc) = (counts[s], counts[bi], counts[ci]);
    // Four budgets of equal weight compete: the server, B, C, and A itself (its calls are kernel
    // work, charged to it). The server's work for A is at most its quarter, so at most what B and
    // C each ran (per thousand of their mean); B and C keep theirs, alike (each of their pair).
    let ratio = sc * 1000 / ((bc + cc) / 2).max(1);
    b.check(
        ratio <= 1000 + TOL,
        format_args!(
            "the server's work for A is {} per 1000 of B's and C's mean ({} calls), at most its quarter",
            ratio, counts[a]
        ),
    );
    let (bp, cp) = (bc * 1000 / (bc + cc).max(1), cc * 1000 / (bc + cc).max(1));
    b.check(
        bp + TOL >= 500 && cp + TOL >= 500,
        format_args!("users B and C got {} and {} of 1000 of their pair, alike", bp, cp),
    );
    b.note(format_args!(
        "useful work: the server {}, B {}, C {} of 1000 of the window",
        b.share(sc, w),
        b.share(bc, w),
        b.share(cc, w)
    ));
    b.finish("SCHED-SERVER-BUSY")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("server-busy", info) }
