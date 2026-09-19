//! Asks the kernel for randomly addressed servers and reports their IDs. A server ID is
//! a capability: whoever knows it can connect. The bench boots this twice and requires
//! the IDs to differ, which fails if the kernel RNG is seeded with something constant.

#![no_std]
#![no_main]

use test_programs::{log, Logger};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let first = xous::create_server().expect("couldn't create a server");
    let second = xous::create_server().expect("couldn't create a second server");
    log!(logger, "[rng] first server id: {:08x?}", first.to_array());
    log!(logger, "[rng] second server id: {:08x?}", second.to_array());
    if first == second {
        log!(logger, "RNG TEST FAILED: two servers got the same id");
    } else {
        log!(logger, "RNG TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
