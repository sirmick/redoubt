//! Asks the kernel for random words (`random`) and reports them. The bench boots this twice and
//! requires the first word to differ, which fails if the kernel RNG is seeded with something
//! constant.

#![no_std]
#![no_main]

use test_programs::{Logger, log, rd};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let first = rd::random().expect("random");
    let second = rd::random().expect("a second random");
    log!(logger, "[rng] first word: {:016x}", first);
    log!(logger, "[rng] second word: {:016x}", second);
    if first == second {
        log!(logger, "RNG TEST FAILED: two words were the same");
    } else {
        log!(logger, "RNG TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
