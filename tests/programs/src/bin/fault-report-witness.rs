//! The witness of `fault-report-bound`: it holds the big process's `GONE` call, answers its
//! `HELD` call once it does, and waits for the kernel's abandoned notice for `GONE`, which comes
//! only once the big process's fault has ended it. It then reports to the checker, which powers
//! off; the oracle judges the fault's lock section from the kernel's trace (rule F). The same
//! handshake as `touch-beyond-ram-survivor`.

#![no_std]
#![no_main]

use test_programs::beyond_ram::{GONE, HELD};
use test_programs::rd::{self, MessageKind, Received};
use test_programs::{Logger, checker, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's second program: it holds the boot endpoint's receive right.
    let (mut gone, mut held) = (None, None);
    loop {
        match rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0) {
            Ok(Received::Abandoned(id)) if gone == Some(id.get()) => break,
            Ok(Received::Message(m)) if matches!(m.kind, MessageKind::Call { lend: None }) => {
                match m.body.words[0] {
                    GONE => gone = Some(m.msg_id.get()),
                    HELD => held = Some(m.msg_id.get()),
                    _ => {
                        rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
                    }
                }
            }
            _ => {}
        }
        if let (Some(_), Some(h)) = (gone, held) {
            rd::reply(h, &rd::body([1, 0, 0, 0])).ok();
            held = None;
        }
    }
    log!(logger, "[witness] the big process's fault ended it");
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
