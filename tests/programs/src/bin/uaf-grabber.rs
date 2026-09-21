//! Use-after-free attack, grabber role. After the victim has died, allocates many pages
//! and fills them with a distinctive sentinel, to reclaim the physical frame that backed
//! the victim's lent page. Then it asks the holder to re-read that page.

#![no_std]
#![no_main]

use test_programs::uaf::*;
use test_programs::{log, Logger};
use redoubt_abi::{MemoryFlags, Message, SID};

const PAGES: usize = 64;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let cid = redoubt_abi::connect(SID::from_bytes(HOLDER_ADDRESS).unwrap()).expect("connect failed");

    // Ordering: the holder answers SYNC only once it is past receiving the lend. Then give
    // the victim's termination time to complete before we try to reclaim its frame.
    redoubt_abi::send_message(cid, Message::new_blocking_scalar(SYNC, 0, 0, 0, 0)).expect("sync failed");
    test_programs::wait_ms(100);

    let mut grabbed = 0;
    for _ in 0..PAGES {
        if let Ok(page) = redoubt_abi::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W) {
            // Touch every page so it is really backed, then stamp it.
            let base = page.as_mut_ptr();
            unsafe { core::ptr::copy_nonoverlapping(GRABBER_SENTINEL.as_ptr(), base, GRABBER_SENTINEL.len()) };
            grabbed += 1;
        }
    }
    log!(logger, "[grabber] stamped {} pages", grabbed);

    redoubt_abi::send_message(cid, Message::new_blocking_scalar(CHECK, 0, 0, 0, 0)).expect("check failed");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
