//! Attacker: destroy a budget while handles to it and to everything below it are still held,
//! then keep using them. It builds B > C > D > scope, and E beside B, each held in its own table
//! slot, destroys B, and tries every call on every stale index; then churns budgets so freed
//! frames are reused, and tries the stale indices again. A handle that outlived its budget would
//! reach a freed frame, and the kernel stops on that (I1) rather than trust it; a reused frame
//! read through a stale handle would carve from a budget the attacker no longer holds. Each stale
//! index is a dead entry (I17): every call through it is `Dead`, no handle the churn makes takes
//! it, and only the attacker's close frees it, once. It builds the tree in its own budget, under
//! `system`. The kernel surviving, and the victim beside it still getting its pages, with
//! `system` within its limits, is the verdict; the kernel's answers to the stale indices are
//! pinned as progress.
//! (No other process holds handles to the tree; copies in other tables, which travel in
//! messages, are not covered by this case.) See `tests/budget-destroy-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The case's third program: the tester gives it its own budget in slot 3, `users` in slot 4,
    // and no device (R2).
    let (own, users) = (rd::OWN, rd::GIVEN);
    log!(logger, "[attacker] starting");
    let b = rd::create(own, &rd::spec(300, 0, 0)).expect("b");
    let c = rd::create(b, &rd::spec(200, 0, 0)).expect("c");
    let d = rd::create(c, &rd::spec(100, 0, 0)).expect("d");
    let scope = rd::create(d, &rd::spec(0, 0, 0)).expect("scope");
    let e = rd::create(own, &rd::spec(10, 0, 0)).expect("e");
    let before = rd::usage(own).unwrap();
    log!(
        logger,
        "[destroy] held b={} c={} d={} scope={} e={}; destroy b -> {:?}",
        b,
        c,
        d,
        scope,
        e,
        rd::destroy(b)
    );
    let stale = [b, c, d, scope];
    let mut stale_ok = 0;
    let mut taken = 0;
    for round in 0..2 {
        for h in stale {
            let results = [
                rd::usage(h).map(|_| ()),
                rd::create(h, &rd::spec(1, 0, 0)).map(|_| ()),
                rd::create(h, &rd::spec(0, 0, 0)).map(|_| ()),
                rd::destroy(h),
            ];
            if results.iter().all(|r| *r == Err(Error::Dead)) {
                stale_ok += 1;
            } else {
                log!(logger, "[destroy] round {} index {}: {:?}", round, h, results);
            }
        }
        // Reuse the freed frames: each new handle takes the lowest index neither live nor dead,
        // and is closed after its destroy, as its holder must, so the next takes it again.
        for _ in 0..50 {
            let x = rd::create(own, &rd::spec(20, 0, 0));
            let y = x.and_then(|x| rd::create(x, &rd::spec(0, 0, 0)));
            for h in [x, y].into_iter().flatten() {
                taken += usize::from(stale.contains(&h));
            }
            if let Ok(x) = x {
                rd::destroy(x).ok();
                rd::close(x).ok();
            }
            if let Ok(y) = y {
                rd::close(y).ok();
            }
        }
    }
    // The holder's close frees each dead entry, once.
    let closed = stale.iter().all(|h| rd::close(*h) == Ok(()) && rd::close(*h) == Err(Error::BadHandle));
    let after = rd::usage(own).unwrap();
    log!(logger, "[destroy] stale index checks dead: {} of 8; stale indices taken: {}", stale_ok, taken);
    log!(logger, "[destroy] stale indices closed once: {}", closed);
    log!(logger, "[destroy] own carve returned: {}", after.pages_usage + 301 == before.pages_usage);
    log!(logger, "[destroy] e untouched: {:?}", rd::usage(e).map(|u| u.pages_limit));
    // Destroying `users`, which the attacker also holds, frees root's share; its handle is dead.
    log!(logger, "[destroy] destroy users -> {:?}, then {:?}", rd::destroy(users), rd::usage(users));
    log!(logger, "[destroy] attempts done");
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
