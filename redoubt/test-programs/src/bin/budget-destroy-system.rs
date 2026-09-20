//! Destroys `system`, which every loader-started process lives in, itself included: R10 kills
//! every process in the destroyed budget, the caller last, and the kernel reports each. See
//! `redoubt/tests/budget-destroy-kills.toml`.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::{Logger, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // Let the bystander start and block in its server before `system` goes.
    test_programs::wait_ms(20);
    log!(logger, "[destroyer] destroying system");
    let result = rd::destroy(rd::SYSTEM);
    // Only reached if the kernel let this process live on in a destroyed budget.
    log!(logger, "[destroyer] BREACH: still running after destroying system: {:?}", result);
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
