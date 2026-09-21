//! Use-after-free attack, victim role. Lends a page to the holder and then terminates
//! while the lend is outstanding, so the kernel's process-teardown path must decide what
//! to do with a page that another process still has mapped.

#![no_std]
#![no_main]

use test_programs::uaf::*;
use test_programs::{log, Logger};
use redoubt_abi::{MemoryFlags, Message, MemorySize, SID};


/// Runs on a second thread: gives the main thread time to lend the page and the holder
/// time to receive it, then kills this process (including the main thread, which is by
/// then blocked in the lend).
fn terminator(_arg: usize) -> ! {
    test_programs::wait_ms(50);
    redoubt_abi::terminate_process(0)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let page = redoubt_abi::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W).expect("map failed");
    unsafe {
        core::ptr::copy_nonoverlapping(VICTIM_SENTINEL.as_ptr(), page.as_mut_ptr(), VICTIM_SENTINEL.len());
    }

    let cid = redoubt_abi::connect(SID::from_bytes(HOLDER_ADDRESS).unwrap()).expect("connect failed");
    redoubt_abi::create_thread_1(terminator, 0).expect("couldn't spawn terminator");
    log!(logger, "[victim] PID {} lending page, then dying", redoubt_abi::current_pid().unwrap());

    // Mutable lend, which blocks until the holder returns the page. The holder never
    // does, so this thread stays here until the terminator kills the process.
    redoubt_abi::send_message(cid, Message::new_lend_mut(HOLD, page, None, MemorySize::new(4096)))
        .expect("lend failed");
    unreachable!("the terminator should have ended this process");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
