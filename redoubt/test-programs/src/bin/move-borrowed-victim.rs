//! Victim for `move-borrowed`: lends a page to it, and once the lend returns, reads the page
//! back and reports what it holds. Its report is relayed by `log-server` under the victim's own
//! PID, so the attacker cannot forge it (redoubt/README.md, "Writing an attack case").

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Logger, Page, log, move_borrowed};
use xous::Message;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let sid = xous::SID::from_bytes(move_borrowed::ADDRESS).unwrap();
    let server = xous::connect(sid).expect("couldn't connect to move-borrowed");
    let mut page = Page::new();
    page.write_str(move_borrowed::VICTIM_TEXT).ok();
    let result = xous::send_message(server, Message::new_lend(1, page.range, None, page.valid()));
    let text = core::str::from_utf8(page.bytes()).unwrap_or("<not utf-8>");
    let intact = result.is_ok() && text == move_borrowed::VICTIM_TEXT;
    log!(
        logger,
        "[victim] lend returned {:?}, page {}: {}",
        result,
        if intact { "intact" } else { "CHANGED" },
        text
    );
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
