//! Survivor for `touch-beyond-ram`: once the kernel reports abandoned the `GONE` call the
//! attacker left it after its refusal (at the attacker's exit, or at that call's own timeout had
//! the attacker chosen one; a boot process has no exit endpoint, so its exit notice is not
//! available), it does ordinary work (a round trip through log-server) and reports `DONE`. The
//! kernel still scheduling and serving it, and the clean power-off, are the verdict; the attacker
//! cannot produce them.

#![no_std]
#![no_main]

use test_programs::beyond_ram::*;
use test_programs::rd::{self, MessageKind, Received};
use test_programs::{Logger, checker, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's second program: it holds the boot endpoint's receive right. The attacker's
    // GONE call is held unanswered, HELD is answered once it is, and the attacker's exit (or
    // GONE's own timeout) then abandons GONE: only the kernel's notice for it ends the wait.
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
    log!(logger, "[survivor] still scheduled after the attacker exhausted RAM");
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
