//! A server's turn across harts (kernel/scheduling.md, "A server's turn crosses harts"): 1000
//! writable lend round trips against `log-server`, as `ipc-client`'s, and nothing else. It says
//! the guest time they took with the width, then has the tester power off unanswered
//! (`op::OFF`), so that a checked kernel's count of hand-offs across harts, from the first message
//! taken to the power-off, covers this program's calls and the server's turns alone.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Logger, Page, checker, log, op, rd};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let mut scratch = Page::new();
    scratch.write_str("lent mutably across address spaces").ok();
    let upper = rd::body([op::UPPERCASE, scratch.bytes().len(), 0, 0]);
    let began = rd::time_now().unwrap_or(0);
    for _ in 0..1000 {
        rd::call_waiting(rd::LOG, &upper, scratch.pages(), rd::FOREVER).expect("a writable lend round trip");
    }
    let took = rd::time_now().unwrap_or(0).saturating_sub(began);
    let ok = scratch.bytes() == b"LENT MUTABLY ACROSS ADDRESS SPACES";
    let width = if cfg!(target_pointer_width = "64") { "rv64" } else { "rv32" };
    log!(
        logger,
        "[ipc-turns] {}: 1000 writable lend round trips, {} us on {}",
        if ok { "ok" } else { "FAIL" },
        took,
        width
    );
    checker::off()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[ipc-turns] FAIL: {}", info);
    test_programs::park()
}
