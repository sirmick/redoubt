//! Use-after-free attack, holder role. Receives a page lent by the victim and never
//! replies, keeping a raw pointer to where it is mapped. After the victim dies and
//! the grabber has run, it re-reads that pointer: if it now sees the grabber's data, the
//! physical frame was freed and reused while still mapped here — a cross-process
//! use-after-free.

#![no_std]
#![no_main]

use test_programs::rd::{self, MessageKind, Received};
use test_programs::uaf::*;
use test_programs::{Logger, log};

fn read8(at: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, b) in out.iter_mut().enumerate() {
        // SAFETY: `at` is the held lend, still mapped here (the call is never replied to).
        *b = unsafe { ((at + i) as *const u8).read_volatile() };
    }
    out
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's second program: it holds the boot endpoint's receive right.
    log!(logger, "[holder] up");
    let mut held = None;
    loop {
        // An abandoned notice for the held call is left unanswered: replying would free the
        // call and give the lend back, and the point is to keep holding it (R3).
        let Ok(Received::Message(m)) = rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0) else { continue };
        let MessageKind::Call { lend } = m.kind else { continue };
        let id = m.msg_id.get();
        match (m.body.words[0], lend) {
            (HOLD, Some(pages)) => {
                log!(
                    logger,
                    "[holder] holding page, victim wrote {:?}",
                    core::str::from_utf8(&read8(pages.addr))
                );
                // Never replied to: the lend stays mapped here and the victim's calling thread
                // stays blocked until the victim dies.
                held = Some(pages.addr);
            }
            (SYNC, None) => {
                rd::reply(id, &rd::body([1, 0, 0, 0])).ok();
            }
            (CHECK, None) => {
                let Some(at) = held else {
                    log!(logger, "UAF TEST FAILED: holder never received the lend (test setup)");
                    rd::reply(id, &rd::body([0; rd::WORDS])).ok();
                    continue;
                };
                let seen = read8(at);
                log!(logger, "[holder] page now reads {:?}", core::str::from_utf8(&seen));
                if &seen == GRABBER_SENTINEL {
                    log!(logger, "UAF TEST FAILED: use-after-free, holder sees the grabber's data");
                } else {
                    log!(logger, "UAF TEST PASSED: freed frame was not reused under the holder");
                }
                rd::reply(id, &rd::body([(&seen == VICTIM_SENTINEL) as usize, 0, 0, 0])).ok();
                if &seen != GRABBER_SENTINEL {
                    // Off the console too: the checker names this PID and powers off, so a
                    // forged verdict line alone cannot pass the case.
                    test_programs::checker::done();
                }
            }
            (other, _) => {
                log!(logger, "[holder] unexpected {}", other);
                rd::reply(id, &rd::body([0; rd::WORDS])).ok();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
