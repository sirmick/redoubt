//! Use-after-free attack, grabber role. After the victim has died, allocates many pages
//! and fills them with a distinctive sentinel, to reclaim the physical frame that backed
//! the victim's lent page. Then it asks the holder to re-read that page.

#![no_std]
#![no_main]

use test_programs::uaf::*;
use test_programs::{Logger, log, rd};

const PAGES: usize = 64;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let call = |op| rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([op, 0, 0, 0]), None, rd::FOREVER);
    // Ordering: the holder answers SYNC only once it is past receiving the lend. Then give
    // the victim's termination time to complete before we try to reclaim its frame.
    call(SYNC).expect("sync failed");
    test_programs::wait_ms(100);
    let mut grabbed = 0;
    for _ in 0..PAGES {
        if let Ok(page) = rd::map_anon(rd::PAGE_SIZE, rd::rw()) {
            // Touch every page so it is really backed, then stamp it.
            // SAFETY: `page` is this process's own fresh read-write page.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    GRABBER_SENTINEL.as_ptr(),
                    page as *mut u8,
                    GRABBER_SENTINEL.len(),
                )
            };
            grabbed += 1;
        }
    }
    log!(logger, "[grabber] stamped {} pages", grabbed);
    call(CHECK).expect("check failed");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
