//! Attacker: carve beyond the parent (R7). It tries to carve more pages than `system` has free
//! (the tester has carved all of them), then more than `users` has of each limit, the largest
//! values, and more than `root` has. A carve the kernel wrongly allowed would leave `system` or
//! `users` past a limit, which the victim, holding both, reads afterwards.
//! Then it carves all but `MARGIN` of its own budget's free pages into a child, legitimately, and
//! tries the same one level down: more than the child has, sums that would wrap. See
//! `tests/budget-carve-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

/// Pages its own budget keeps free beside the child: room for what serving its log lines needs.
const MARGIN: u64 = 100;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The case's third program: the tester gives it its own budget in slot 3, the budgets its
    // case names from slot 4, and no device (R2).
    let (root, system, users) = (rd::GIVEN, rd::GIVEN + 1, rd::GIVEN + 2);
    log!(logger, "[attacker] starting");
    let u = rd::usage(users).unwrap();
    let (free_procs, free_weight) = (u.processes_limit - u.processes_usage, u.weight_limit - u.weight_carved);
    let hog = rd::create(rd::OWN, &rd::spec(rd::free(rd::OWN) - MARGIN, 0, 0)).expect("carve");
    let attempts: [(u32, u64, u32, u32); 10] = [
        (system, rd::free(system) + 1, 0, 0),
        (system, u64::MAX, 0, 0),
        (users, 1, free_procs + 1, 0),
        (users, 1, u32::MAX, 0),
        (users, 1, 0, free_weight + 1),
        (users, 1, 0, u32::MAX),
        (root, rd::free(root) + 1, 0, 0),
        (hog, u64::MAX, 0, 0),
        (hog, u64::MAX - 1, u32::MAX, u32::MAX),
        (users, u64::MAX / 2 + 1, 0, 0),
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
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
