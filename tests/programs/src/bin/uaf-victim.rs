//! Use-after-free attack, victim role. Lends a page to the holder and then exits while the
//! lend is outstanding, so the kernel's process-teardown path must decide what to do with a
//! page that another process still has mapped.

#![no_std]
#![no_main]

use test_programs::uaf::*;
use test_programs::{Logger, log, rd};

/// Runs on a second thread: gives the main thread time to lend the page and the holder
/// time to receive it, then ends this process (including the main thread, which is by
/// then blocked in the lend).
fn terminator(_arg: usize) {
    test_programs::wait_ms(50);
    rd::process_exit(0)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let page = rd::page();
    // SAFETY: `page` is this process's own fresh read-write page.
    unsafe {
        core::ptr::copy_nonoverlapping(VICTIM_SENTINEL.as_ptr(), page as *mut u8, VICTIM_SENTINEL.len())
    };
    rd::thread(terminator, 0).expect("couldn't spawn terminator");
    log!(logger, "[victim] lending page, then dying");
    // A writable lend on the boot endpoint, whose receive right the holder holds. The holder
    // never replies, so this thread stays here until the terminator ends the process.
    rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([HOLD, 0, 0, 0]), rd::pages(page, 1), rd::FOREVER)
        .expect("lend failed");
    unreachable!("the terminator should have ended this process");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
