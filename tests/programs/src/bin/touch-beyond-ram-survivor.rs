//! Survivor for `touch-beyond-ram`: after the attacker has had time to exhaust RAM and be
//! terminated, it does ordinary work (a round trip through log-server) and reports `DONE`. The
//! kernel still scheduling and serving it, and the clean power-off, are the verdict; the attacker
//! cannot produce them.

#![no_std]
#![no_main]

use test_programs::{Logger, checker, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // Give the attacker time to fault through RAM and be terminated.
    test_programs::wait_ms(300);
    log!(logger, "[survivor] still scheduled after the attacker exhausted RAM");
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
