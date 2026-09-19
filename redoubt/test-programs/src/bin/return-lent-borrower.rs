//! Borrower for `return-lent`: receives a mutable borrow, holds it long enough for the lender's
//! second thread to attack the lent address, then returns it by dropping the envelope.

#![no_std]
#![no_main]

use test_programs::return_lent;
use test_programs::{Logger, log};
use xous::Message;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let sid = xous::create_server_with_address(return_lent::ADDRESS).expect("couldn't create server");
    log!(logger, "[borrower] listening");
    let envelope = xous::receive_message(sid).expect("receive failed");
    if let Message::MutableBorrow(_) = &envelope.body {
        // Hold past the clobber thread's 30 ms delay, so the unmap/remap hit a still-lent page.
        test_programs::wait_ms(90);
    }
    drop(envelope); // returns the page to the lender
    log!(logger, "[borrower] returned the page");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
