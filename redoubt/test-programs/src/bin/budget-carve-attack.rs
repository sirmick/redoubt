//! Attacker: carve beyond the parent (R7). It first carves all but `MARGIN` of `system`'s free
//! pages into a child of its own, legitimately, so that any carve the kernel wrongly allows
//! beyond `system`'s limits leaves `system` too little for the victim's pages. Then it tries every
//! way of carving more than a parent has: each limit, the largest values, sums that would wrap,
//! and the same one level down. See `redoubt/tests/budget-carve-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

/// Pages `system` keeps free for the victim: more than its 64 pages and their tables, fewer than
/// the smallest over-carve tried below.
const MARGIN: u64 = 100;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // Let log-server map what serving a message needs before `system`'s usage is read.
    log!(logger, "[attacker] starting");
    test_programs::wait_ms(20);
    let hog = rd::create(rd::SYSTEM, &rd::spec(rd::free(rd::SYSTEM) - MARGIN, 0, 0)).expect("carve");
    let u = rd::usage(rd::SYSTEM).unwrap();
    let (free_procs, free_weight) = (u.processes_limit - u.processes_usage, u.weight_limit - u.weight_carved);
    let attempts: [(u32, u64, u32, u32); 10] = [
        (rd::SYSTEM, MARGIN + 1, 0, 0),
        (rd::SYSTEM, u64::MAX, 0, 0),
        (rd::SYSTEM, 1, free_procs + 1, 0),
        (rd::SYSTEM, 1, u32::MAX, 0),
        (rd::SYSTEM, 1, 0, free_weight + 1),
        (rd::SYSTEM, 1, 0, u32::MAX),
        (rd::ROOT, 1, 0, 0), // root is full: users took the rest
        (hog, u64::MAX, 0, 0),
        (hog, u64::MAX - 1, u32::MAX, u32::MAX),
        (rd::USERS, u64::MAX / 2 + 1, 0, 0),
    ];
    for (parent, pages, processes, weight) in attempts {
        let got = rd::create(parent, &rd::spec(pages, processes, weight));
        log!(
            logger,
            "[carve] {} pages {} processes {} weight under {} -> {:?}",
            pages,
            processes,
            weight,
            parent,
            got
        );
    }
    // Two children that together exceed their parent, or whose sum wraps a u64.
    let first = rd::create(hog, &rd::spec(rd::free(hog) - 1, 0, 0));
    let second = rd::create(hog, &rd::spec(u64::MAX, 0, 0));
    log!(logger, "[carve] fill the child -> {:?}, then u64::MAX more -> {:?}", first.map(|_| ()), second);
    let scope = rd::create(hog, &rd::spec(0, 0, 0));
    log!(logger, "[carve] a scope in the full child -> {:?}", scope);
    let err = matches!(second, Err(Error::OutOfMemory)) && matches!(scope, Err(Error::OutOfMemory));
    log!(logger, "[carve] attempts done ({})", if err { "all refused" } else { "BREACH" });
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
