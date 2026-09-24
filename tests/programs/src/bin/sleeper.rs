//! Sleeps on the kernel's timer beside other work (WP-K5): batches of short timeouts of varying
//! length, each of which must come back `Timeout` and never early, for as long as the machine
//! runs. A verdict line follows every batch, so one comes after whatever else the case waits for.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

const BATCH: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    loop {
        let mut good = true;
        for i in 0..BATCH {
            let timeout = 500 + (i % 7) * 400;
            let before = rd::time_now().expect("time_now");
            let r = rd::receive(None, timeout, 0);
            let after = rd::time_now().expect("time_now");
            good &= r == Err(Error::Timeout) && after >= before + timeout;
        }
        log!(
            logger,
            "[sleeper] {}: {} sleeps returned Timeout, none early",
            if good { "ok" } else { "FAIL" },
            BATCH
        );
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
