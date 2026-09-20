//! Attack test, attacker role: reserve far more anonymous memory than the machine has RAM and
//! touch every page. Each first touch faults into the kernel, which backs it with a real page
//! until RAM runs out. That out-of-memory must terminate this one process, not panic the whole
//! kernel. This program is expected to die mid-loop; the verdict is that another process is
//! still served afterwards (see `touch-beyond-ram-survivor`).

#![no_std]
#![no_main]

use test_programs::{Logger, log};
use xous::MemoryFlags;

/// More pages than the case's small guest has RAM (see the toml's `memory_mib`).
const PAGES: usize = 16 * 1024; // 64 MiB, above the 32 MiB machine.

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let region =
        xous::map_memory(None, None, PAGES * 4096, MemoryFlags::R | MemoryFlags::W).expect("reserve");
    log!(logger, "[attacker] touching {} reserved pages", PAGES);
    let base = region.as_mut_ptr();
    for page in 0..PAGES {
        // SAFETY: `base + page*4096` is inside the reservation; the write faults the page in.
        // The kernel terminates this process when it can no longer back one; the loop never ends
        // normally.
        unsafe { base.add(page * 4096).write_volatile(0xa5) };
    }
    log!(logger, "[attacker] BREACH: touched all pages without dying");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
