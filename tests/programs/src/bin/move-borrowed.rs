//! Attack test: a server transfers on a page that was only lent to it.
//!
//! A lent page still belongs to its lender. Transferring it would hand the lender's frame to a
//! third process for good; the kernel once found the ownership mismatch only after changing
//! the page tables, and panicked ("unable to send"). The transfer must be refused, and the
//! lender (`move-borrowed-victim`) must get its page back. See
//! `tests/move-borrowed-page.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, MessageKind, Received};
use test_programs::{Logger, log, op};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's second program: it holds the boot endpoint's receive right.
    log!(logger, "[move-borrowed] listening");
    let received = rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0);
    if let Ok(Received::Message(m)) = received {
        if let MessageKind::Call { lend: Some(pages) } = m.kind {
            let body = rd::body([op::PRINT_AND_KEEP, 0, 0, 0]);
            let result = rd::send(rd::LOG, &body, Some(pages), rd::FOREVER);
            log!(logger, "[move-borrowed] transferring the lent page on: {:?}", result);
        }
        // Returns the page to its lender.
        rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
