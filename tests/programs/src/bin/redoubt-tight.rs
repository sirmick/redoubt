//! A budget carved to exactly what R4's pre-check used to count (from a red-team probe): a
//! `call` with a lend delivered to a receiver whose budget has precisely the open-call page plus
//! the lent pages free, and nothing for the page tables that map the lend where it lands.
//!
//! R4 lists those page tables among what the receiving budget must be able to pay for, so the
//! answer is `Refused` to the sender. The kernel used to leave them out of the decision and
//! charge them anyway, and the charge that followed -- written as "checked just above" -- then
//! failed and stopped the machine, from an ordinary `call` (I14).
//!
//! The receiver is a thread of this same process, so the lend lands in a Messages region with
//! no page tables yet, which is what makes the shortfall exact. Must run as the loader's first
//! program: it needs a `system` budget handle to carve.
//!
//! See `tests/redoubt-tight.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, FOREVER, Received};
use test_programs::log;

static mut ENDPOINT: u32 = 0;

fn endpoint() -> u32 {
    // SAFETY: the main thread writes it once, before the receiver thread is created.
    unsafe { core::ptr::read_volatile(&raw const ENDPOINT) }
}

/// The receiver: it must be waiting, so that delivery runs its whole course.
fn receiver(_arg: usize) {
    loop {
        if let Ok(Received::Message(m)) = rd::receive(Some(endpoint()), FOREVER, 64) {
            rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
        }
    }
}

/// Pages in the lend. Three, so the shortfall cannot be hidden by a single page's table.
const PAGES: usize = 3;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = test_programs::logsrv::start();
    log!(logger, "[tight] starting");
    let endpoint = rd::endpoint_create().expect("an endpoint");
    // SAFETY: written before the receiver thread is created.
    unsafe { core::ptr::write_volatile(&raw mut ENDPOINT, endpoint) };
    rd::thread(receiver, 0).expect("the receiver");
    test_programs::wait_ms(30);

    let buf = rd::many_pages(PAGES);
    let free = rd::free(rd::SYSTEM);
    log!(logger, "[tight] system free before the carve: {}", free);
    // Creating the hog charges `system` its own page as well, so this leaves exactly the
    // open-call page plus the lent pages free: everything the old pre-check counted, and not
    // one page more.
    let hog = rd::create(rd::SYSTEM, &rd::spec(free - PAGES as u64 - 2, 0, 0)).expect("a hog budget");
    let left = rd::free(rd::SYSTEM);
    log!(logger, "[tight] system free after the carve: {} (want {})", left, PAGES + 1);

    let result = rd::call(endpoint, &rd::body([0; rd::WORDS]), rd::pages(buf, PAGES), FOREVER);
    // A refused delivery costs the receiving budget nothing: not the open-call page, not the
    // lend, and not the page tables that would have mapped it. Its free pages are as they were.
    let spent = left - rd::free(rd::SYSTEM);
    log!(logger, "[tight] the tight lend -> {:?}, {} pages spent", result, spent);

    // With the pages back, the same message goes through to the same waiting receiver.
    rd::destroy(hog).expect("the hog goes");
    let after = rd::call(endpoint, &rd::body([0; rd::WORDS]), rd::pages(buf, PAGES), FOREVER);
    log!(logger, "[tight] after the hog goes: the same lend -> {:?}", after.map(|_| ()));

    if result.err() == Some(Error::Refused) && spent == 0 && after.is_ok() {
        log!(logger, "REDOUBT-TIGHT TEST PASSED");
    } else {
        log!(logger, "REDOUBT-TIGHT TEST FAILED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
