//! Attacker: destroy a budget while handles to it and to everything below it are still held,
//! then keep using them. It builds B > C > D > scope, and E beside B, each held in its own table
//! slot, destroys B, and tries every call on every stale index; then churns budgets so freed
//! frames and indices are reused, and tries the stale indices again. A handle that outlived its
//! budget would reach a freed frame, and the kernel stops on that (I1) rather than trust it; a
//! reused frame read through a stale handle would carve from a budget the attacker no longer
//! holds. The kernel surviving, and the victim still getting its pages from `system`, is the
//! verdict. (No other process holds handles here; copies in other tables, which travel in
//! messages, are not covered by this case.) See `tests/budget-destroy-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's third program: its budgets come from log-server, once, and no device (R2).
    let rd::Gifts { system, users, .. } = rd::take_gifts().expect("the budgets");
    log!(logger, "[attacker] starting");
    let b = rd::create(system, &rd::spec(300, 0, 0)).expect("b");
    let c = rd::create(b, &rd::spec(200, 0, 0)).expect("c");
    let d = rd::create(c, &rd::spec(100, 0, 0)).expect("d");
    let scope = rd::create(d, &rd::spec(0, 0, 0)).expect("scope");
    let e = rd::create(system, &rd::spec(10, 0, 0)).expect("e");
    let before = rd::usage(system).unwrap();
    log!(logger, "[destroy] held b={} c={} d={} scope={} e={}; destroy b -> {:?}", b, c, d, scope, e, rd::destroy(b));
    let mut stale_ok = 0;
    for round in 0..2 {
        for h in [b, c, d, scope] {
            let results = [
                rd::usage(h).map(|_| ()),
                rd::create(h, &rd::spec(1, 0, 0)).map(|_| ()),
                rd::create(h, &rd::spec(0, 0, 0)).map(|_| ()),
                rd::destroy(h),
                rd::close(h),
            ];
            if results.iter().all(|r| *r == Err(Error::BadHandle)) {
                stale_ok += 1;
            } else {
                log!(logger, "[destroy] round {} index {}: {:?}", round, h, results);
            }
        }
        // Reuse the freed frames (and, from the second round, the indices below b's).
        for _ in 0..50 {
            let x = rd::create(system, &rd::spec(20, 0, 0));
            let y = x.and_then(|x| rd::create(x, &rd::spec(0, 0, 0)));
            if let Ok(x) = x {
                rd::destroy(x).ok();
            }
            let _ = y;
        }
    }
    let after = rd::usage(system).unwrap();
    log!(logger, "[destroy] stale index checks refused: {} of 8", stale_ok);
    log!(logger, "[destroy] system carve returned: {}", after.pages_usage + 301 == before.pages_usage);
    log!(logger, "[destroy] e untouched: {:?}", rd::usage(e).map(|u| u.pages_limit));
    // Destroying `users`, which the attacker also holds, frees root's share; its handle goes.
    log!(logger, "[destroy] destroy users -> {:?}, then {:?}", rd::destroy(users), rd::usage(users));
    log!(logger, "[destroy] attempts done");
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
