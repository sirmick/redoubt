//! Attacker: exhaust the handle table. It fills its table with revocation scopes (cheap: one
//! page each, charged to a child budget of its own) until the kernel refuses, closes and
//! reopens across a page boundary, then destroys the child, which sweeps every one of them.
//! Every table page is charged to `system`, the attacker's budget, and the victim beside it must
//! still get its pages afterwards. See `redoubt/tests/budget-table-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // Let log-server map what serving a message needs before `system`'s usage is read.
    log!(logger, "[attacker] starting");
    test_programs::wait_ms(20);
    let before = rd::usage(rd::SYSTEM).unwrap().pages_usage;
    let pool = rd::create(rd::SYSTEM, &rd::spec(5000, 0, 0)).expect("carve");
    let mut last = pool;
    let refusal = loop {
        match rd::create(pool, &rd::spec(0, 0, 0)) {
            Ok(h) => last = h,
            Err(e) => break e,
        }
    };
    let pool_usage = rd::usage(pool).unwrap().pages_usage;
    log!(logger, "[table] filled to handle {}, then {:?}; the pool paid {} pages", last, refusal, pool_usage);
    // Table pages were charged to system: 32 of them, beyond the pool (5000 pages and its own).
    let table_pages = rd::usage(rd::SYSTEM).unwrap().pages_usage - before - 5001;
    log!(logger, "[table] system paid {} pages for the table", table_pages);
    // Hand back the last page's handles, then take one again: the page is freed and recharged.
    let closed = (3969..=last).all(|h| rd::close(h).is_ok());
    let reopened = rd::create(pool, &rd::spec(0, 0, 0));
    log!(logger, "[table] closed 3969..={}: {}; reopened -> {:?}", last, closed, reopened);
    // Indices past the table are not handles.
    for forged in [last + 1, 4097, 4224, u32::MAX] {
        let got = rd::usage(forged);
        if got != Err(Error::BadHandle) {
            log!(logger, "[table] BREACH: index {} -> {:?}", forged, got);
        }
    }
    let destroyed = rd::destroy(pool);
    let after = rd::usage(rd::SYSTEM).unwrap().pages_usage;
    log!(logger, "[table] destroyed the pool -> {:?}; system usage back where it was: {}", destroyed, before == after);
    let gone = (4..=3969).all(|h| rd::usage(h) == Err(Error::BadHandle));
    log!(logger, "[table] every swept index is empty: {}", gone);
    log!(logger, "[table] attempts done");
    rd::victim::go();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
