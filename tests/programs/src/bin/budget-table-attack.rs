//! Attacker: exhaust the handle table. It fills its table with revocation scopes (cheap: one
//! page each, charged to a pool it carves from `users`) until the kernel refuses (`TooLarge` past
//! `MAX_HANDLES`), closes and reopens across a page boundary, then destroys the pool, which
//! closes every one of them.
//! Every table page is charged to the budget it runs in, its own (slot 3), and the victim beside
//! it, under `system` too, must still get its pages afterwards. See `tests/budget-table-attack.toml`.

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
    // Log once before its own usage is read: the first line maps what logging needs.
    log!(logger, "[attacker] starting");
    let before = rd::usage(own).unwrap().pages_usage;
    // The first index this program does not already hold.
    let base = rd::first_free();
    let pool = rd::create(users, &rd::spec(5000, 0, 0)).expect("carve");
    let mut last = pool;
    let refusal = loop {
        match rd::create(pool, &rd::spec(0, 0, 0)) {
            Ok(h) => last = h,
            Err(e) => break e,
        }
    };
    let pool_usage = rd::usage(pool).unwrap().pages_usage;
    // The scopes are every index from the pool's own to the last: one page each, and the
    // table stops at MAX_HANDLES whatever the machine handed this program to start with.
    let one_each = pool_usage == u64::from(rd::MAX_HANDLES as u32 - base) && last == rd::MAX_HANDLES as u32;
    log!(
        logger,
        "[table] filled to handle {}, then {:?}; the pool paid {} pages ({})",
        last,
        refusal,
        pool_usage,
        if one_each { "one per scope" } else { "FAIL" }
    );
    // Table pages are charged to its own budget: 63 more than the one it had (64 handles a page);
    // the pool is `users`'.
    let table_pages = rd::usage(own).unwrap().pages_usage - before;
    log!(logger, "[table] its own budget paid {} pages for the table", table_pages);
    // Hand back the last page's handles, then take one again: the page is freed and recharged.
    let closed = (4033..=last).all(|h| rd::close(h).is_ok());
    let reopened = rd::create(pool, &rd::spec(0, 0, 0));
    log!(logger, "[table] closed 4033..={}: {}; reopened -> {:?}", last, closed, reopened);
    let destroyed = rd::destroy(pool);
    let after = rd::usage(own).unwrap().pages_usage;
    log!(
        logger,
        "[table] destroyed the pool -> {:?}; its own usage back where it was: {}",
        destroyed,
        before == after
    );
    let gone = (base..=4033).all(|h| rd::usage(h) == Err(Error::BadHandle));
    log!(logger, "[table] every swept index is empty: {}", gone);
    log!(logger, "[table] attempts done");
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
