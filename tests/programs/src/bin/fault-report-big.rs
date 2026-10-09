//! The faulting side of `fault-report-bound`: a process that holds many pages faults, and its
//! fault's handling must not cost what it holds (kernel/scheduling.md, "Fair kernel entry is
//! bounded by count"). It maps `PAGES` pages, leaves the witness a call that its end abandons,
//! and stores to an address it never mapped. The verdict is the oracle's, from the kernel's trace
//! of the fault's lock section, and the witness's report of the abandoned call; this program
//! prints only what it did.

#![no_std]
#![no_main]

use test_programs::beyond_ram::{GONE, HELD};
use test_programs::{Logger, log, rd};

/// Pages mapped before the fault: 16 MiB, each a leaf in the process's tables.
const PAGES: usize = 4096;

/// Calls the witness and blocks there until this process ends, which abandons the call.
fn leave(_arg: usize) {
    rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([GONE, 0, 0, 0]), None, rd::FOREVER).ok();
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    rd::map_anon(PAGES * rd::PAGE_SIZE, rd::rw()).expect("couldn't map the pages");
    log!(logger, "[big] mapped {} pages; faulting", PAGES);
    rd::thread(leave, 0).expect("couldn't spawn the leaving thread");
    rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([HELD, 0, 0, 0]), None, rd::FOREVER)
        .expect("the witness never held the leaving call");
    // SAFETY: none: a store to the page at 0x1000, which nothing maps, so that the process faults
    // and ends there. It never returns, and nothing is written.
    unsafe { (0x1000 as *mut usize).write_volatile(1) };
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
