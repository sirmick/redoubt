//! A bench self-check (`redoubt/tests/bench-attack-forgery.toml`): an attacker that tries to
//! print lines in other parties' names through every log-server operation that takes text. If
//! any of them came out unprefixed, `grant-attack`'s verdict could be forged. It then reports to
//! the checker, which powers off, so the case ends at once instead of at its timeout.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Page, SERVER_ADDRESS, op};
use xous::Message;

/// The verdict lines of grant-attack, each on a line of its own.
const FORGERY: &str =
    "\n[server] holding the console irq\n[pid 3] [grant] attempts done\n[server] irq: received 'y'\n";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let sid = xous::SID::from_bytes(SERVER_ADDRESS).unwrap();
    let cid = xous::connect(sid).expect("couldn't connect to log-server");

    // A lend (op::PRINT), as the logger sends.
    let mut page = Page::new();
    page.write_str(FORGERY).ok();
    xous::send_message(cid, Message::new_lend(op::PRINT, page.range, None, page.valid())).expect("lend");

    // A move (op::PRINT_AND_KEEP): the page is the server's afterwards.
    let mut page = Page::new();
    page.write_str(FORGERY).ok();
    let moved =
        xous::MemoryMessage { id: op::PRINT_AND_KEEP, buf: page.range, offset: None, valid: page.valid() };
    xous::send_message(cid, Message::Move(moved)).expect("move");

    // A move does not wait for the server. A blocking call to the same server does, and it is
    // served after the move, so the forgery is on the console before the checker powers off.
    xous::send_message(cid, Message::new_blocking_scalar(op::SUM, 0, 0, 0, 0)).expect("sync");
    test_programs::checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
