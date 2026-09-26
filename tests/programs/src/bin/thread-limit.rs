//! A process has exactly `MAX_THREADS` threads, its initial one included, numbered
//! `1..=MAX_THREADS` (kernel/processes.md, "Threads"). The verdicts are the kernel's: the TIDs
//! `thread_create` returns, and `TooManyThreads` for the one past the limit.

#![no_std]
#![no_main]

use redoubt_sys::MAX_THREADS;
use test_programs::rd::{self, Error};
use test_programs::{Logger, log};

extern "C" fn parked(_arg: usize) -> ! { test_programs::park() }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = test_programs::logsrv::start();

    // The initial thread holds one TID; every other one is created here, each on its own page.
    let mut seen = 1u64 << 1;
    let mut in_range = true;
    let mut threads = 1;
    let refusal = loop {
        let stack = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("a stack page");
        match rd::thread_create(parked as *const () as usize, stack + rd::PAGE_SIZE - 16, 0) {
            Ok(tid) => {
                in_range &= (1..=MAX_THREADS as u32).contains(&tid) && seen & (1 << tid) == 0;
                seen |= 1 << tid;
                threads += 1;
            }
            Err(e) => break e,
        }
        if threads > MAX_THREADS + 1 {
            break Error::InvalidArgument;
        }
    };
    let all = seen == ((1u64 << (MAX_THREADS + 1)) - 2);
    log!(logger, "[thread-limit] {} threads, the initial one included", threads);
    log!(logger, "[thread-limit] TIDs distinct and within 1..={}, all used: {}", MAX_THREADS, in_range && all);
    log!(logger, "[thread-limit] the next thread_create: {:?}", refusal);
    if threads == MAX_THREADS && in_range && all && refusal == Error::TooManyThreads {
        log!(logger, "THREAD LIMIT TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[thread-limit] FAIL: panic: {}", info);
    test_programs::park()
}
