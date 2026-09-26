//! Borrower for `return-lent`: receives a writable lend, holds it long enough for the lender's
//! second thread to attack the lent address, then returns it by replying.

#![no_std]
#![no_main]

use test_programs::rd::{self, Received};
use test_programs::{Logger, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's second program: it holds the boot endpoint's receive right.
    log!(logger, "[borrower] listening");
    if let Ok(Received::Message(m)) = rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0) {
        // Hold past the clobber thread's 30 ms delay, so the unmap/remap hit a still-lent page.
        test_programs::wait_ms(90);
        rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
    }
    log!(logger, "[borrower] returned the page");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
