//! Victim for `move-borrowed`: lends a page to it, and once the lend returns, reads the page
//! back and reports what it holds. Its report is relayed by `log-server` under the victim's own
//! PID, so the attacker cannot forge it (docs/testbench.md, "Rule F (trusted verdicts)").

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Logger, Page, log, move_borrowed, rd};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let mut page = Page::new();
    page.write_str(move_borrowed::VICTIM_TEXT).ok();
    // Slot 1: a send on the boot endpoint, whose receive right `move-borrowed` holds.
    let result = rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([1, 0, 0, 0]), page.pages(), rd::FOREVER);
    let text = core::str::from_utf8(page.bytes()).unwrap_or("<not utf-8>");
    let intact = result.is_ok() && text == move_borrowed::VICTIM_TEXT;
    log!(
        logger,
        "[victim] lend returned {}, page {}: {}",
        if result.is_ok() { "Ok" } else { "an error" },
        if intact { "intact" } else { "CHANGED" },
        text
    );
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
