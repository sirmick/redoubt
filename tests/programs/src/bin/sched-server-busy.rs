//! A system server busy on one user's requests delays other users only by its weight (R12;
//! kernel/scheduling.md): user A floods a system-class server (weight 100, 2 ms of work per
//! request) from four threads; users B and C spin. The server's work stays within its weight's
//! share, and B and C each keep theirs (a quarter: A's calls are its own CPU too).

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
    let (ss, bs, cs) = (b.share(counts[s], w), b.share(counts[bi], w), b.share(counts[ci], w));
    // Four budgets of equal weight compete: the server, B, C, and A itself (its calls are kernel
    // work, charged to it). The server's work for A is at most its quarter; B and C keep theirs.
    b.check(
        ss <= 250 + TOL,
        format_args!(
            "the server's work for A is {} of 1000 ({} calls), at most its weight's quarter",
            ss, counts[a]
        ),
    );
    b.check(
        bs + TOL >= 250 && cs + TOL >= 250,
        format_args!("users B and C got {} and {} of 1000, at least a quarter each", bs, cs),
    );
    b.finish("SCHED-SERVER-BUSY")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("server-busy", info) }
