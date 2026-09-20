//! Attack test: a server moves on a page that was only lent to it.
//!
//! A borrowed page still belongs to its lender. Moving it would hand the lender's frame to a
//! third process for good; the kernel once found the ownership mismatch only after changing
//! the page tables, and panicked ("unable to send"). The move must be refused, and the
//! lender (`move-borrowed-victim`) must get its page back. See
//! `redoubt/tests/move-borrowed-page.toml`.

#![no_std]
#![no_main]

use test_programs::{Logger, log, op};
use xous::{MemoryMessage, Message};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let sid = xous::create_server_with_address(test_programs::move_borrowed::ADDRESS)
        .expect("couldn't create server");
    log!(logger, "[move-borrowed] listening");
    let mut envelope = xous::receive_message(sid).expect("couldn't receive");
    if let Message::Borrow(m) = &envelope.body {
        let message = MemoryMessage { id: op::PRINT_AND_KEEP, buf: m.buf, offset: None, valid: None };
        let result = xous::send_message(logger.cid, Message::Move(message));
        log!(logger, "[move-borrowed] moving the borrowed page on: {:?}", result);
    }
    drop(envelope); // returns the page to its lender
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
