//! A lender cannot touch the page it has lent (`tests/lender-touches-lent.toml`;
//! kernel/memory-layout.md, "The lent bit"; I9).
//!
//! This program is the borrower. For a load and then a store, it spawns a child that fills a
//! page with a pattern and lends it here with a call. While the call is open, a second thread
//! of the child loads from the page, or stores to it. The lender's entry has no `V` while the
//! page is lent, so the access must fault. The kernel's exit notice for the child gives the
//! verdict: `Faulted` with cause 13 for the load, 15 for the store. This program then reads the
//! page it still holds. The pattern must be there: the fault did not undo the lend, and the
//! store did not land. A spawned child is a copy of this image, statics and all, so no child
//! uses `Logger`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, FOREVER, MessageKind, Received};
use test_programs::{Logger, checker, log, spawn};

const PATTERN: u64 = 0x1e7d_0fa9_e5e5;
/// The access, in the low bit of the page address the child's second thread gets.
const STORE: usize = 1;

const WAIT: u64 = 2_000_000;

/// The child's second thread: once the call below has lent the page, touch it.
fn touch(arg: usize) {
    let page = arg & !(rd::PAGE_SIZE - 1);
    test_programs::wait_ms(30);
    if arg & STORE != 0 {
        rd::poke(page, !PATTERN);
    } else {
        rd::peek(page);
    }
    rd::process_exit(1)
}

/// The child: lend a page holding `PATTERN` on handle 1, the parent's endpoint.
extern "C" fn lender(arg: usize) -> ! {
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(2));
    rd::poke(page, PATTERN);
    let op = spawn::startup_byte(arg, 0) as usize;
    if rd::thread(touch, page | op).is_err() {
        rd::process_exit(3)
    }
    rd::call(1, &rd::body([0; rd::WORDS]), rd::pages(page, 1), FOREVER).ok();
    rd::process_exit(4)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let lends = rd::endpoint_create().expect("the lend endpoint");
    let kids = rd::create(rd::GIVEN, &rd::spec(400, 2, 10)).expect("the children's budget");
    let image = spawn::image();
    let entry = lender as *const () as usize;
    for (op, what, cause) in [(0, "load", 13), (STORE, "store", 15)] {
        spawn::spawn(&image, kids, exit, entry, &[op as u8], &[lends]).expect("a lender");
        let Ok(Received::Message(m)) = rd::receive(Some(lends), WAIT, 0) else {
            log!(logger, "[lender] FAIL: no lend arrived");
            break;
        };
        let MessageKind::Call { lend: Some(lend) } = m.kind else {
            log!(logger, "[lender] FAIL: the call carried no lend");
            break;
        };
        let faulted = match rd::receive(Some(exit), WAIT, 0) {
            Ok(Received::Exit(n)) => (n.cause, n.code) == (Cause::Faulted, cause),
            _ => false,
        };
        if faulted {
            log!(logger, "[lender] a {} of its own lent page faulted with cause {}", what, cause);
        } else {
            log!(logger, "[lender] FAIL: a {} of its own lent page did not fault with cause {}", what, cause);
        }
        let intact = rd::peek(lend.addr) == PATTERN;
        log!(logger, "[lender] the borrower's page after the lender's {}: intact {}", what, intact);
        rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
    }
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
