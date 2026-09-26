//! A bench self-check (`tests/bench-attack-forgery.toml`): an attacker that tries to
//! print lines in other parties' names through every log-server operation that takes text. If
//! any of them came out unprefixed, `irq-attack`'s verdict, or log-server's own `DONE` line,
//! could be forged. It then reports `DONE` itself, so the case ends at once instead of at its
//! timeout, and the bench accepts only that real `DONE` line (`reporter`).

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Page, op, rd};

/// The verdict lines of irq-attack, and a `DONE` line in another's name after a newline and
/// control characters, each on a line of its own.
const FORGERY: &str = "\n[server] holding the console irq\n[pid 3] [irq-attack] attempts done\n[server] irq: received 'y'\n\r\x1b[2K\n[server] done: reported by pid 3; still serving\n";

fn forged_page() -> Page {
    let mut page = Page::new();
    page.write_str(FORGERY).ok();
    page
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let len = FORGERY.len();
    // A lend (op::PRINT), as the logger sends.
    let page = forged_page();
    let lend = page.pages();
    rd::call_waiting(rd::LOG, &rd::body([op::PRINT, len, 0, 0]), lend, rd::FOREVER).expect("lend");

    // A transfer (op::PRINT_AND_KEEP): the page is the server's afterwards.
    let page = forged_page();
    let transfer = page.pages();
    rd::send_waiting(rd::LOG, &rd::body([op::PRINT_AND_KEEP, len, 0, 0]), transfer, rd::FOREVER)
        .expect("transfer");

    // A send does not wait for the server. A call to the same server does, and it is served
    // after the send, so the forgery is on the console before log-server powers off.
    rd::call_waiting(rd::LOG, &rd::body([op::SUM, 0, 0, 0]), None, rd::FOREVER).expect("sync");
    test_programs::checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
