//! Attack test for `MapMemory` (see the fix in `kernel/src/syscall.rs`).
//!
//! Two invariants a hostile process must not be able to break:
//!   1. RAM handed out anonymously (`phys = 0`) is zeroed before the process sees it.
//!   2. A process cannot map a physical RAM frame *by address* — that would let it point at
//!      another process's freed page and read what was left there.
//!
//! So this program asks for an anonymous page and checks it is all zero, then tries to map
//! several physical addresses inside main RAM and requires each to be refused with
//! `InvalidArgument`. If a map unexpectedly succeeds it also reports whether the frame held
//! nonzero (leaked) data, so a regression is loud.

#![no_std]
#![no_main]

use test_programs::{log, Logger};
use xous::{MemoryAddress, MemoryFlags};

/// Physical addresses inside QEMU `virt` main RAM (base 0x8000_0000, 256 MiB, on both
/// widths). Mapping any of these by explicit address must be refused.
const RAM_ADDRS: &[usize] = &[0x8000_0000, 0x8100_0000, 0x88ff_f000];

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let flags = MemoryFlags::R | MemoryFlags::W;
    let mut ok = true;

    // (1) Anonymous RAM must come back zeroed.
    match xous::map_memory(None, None, 4096, flags) {
        Ok(range) => {
            // SAFETY: the kernel just mapped `range.len()` readable bytes here for us.
            let bytes = unsafe { core::slice::from_raw_parts(range.as_ptr(), range.len()) };
            if bytes.iter().any(|&b| b != 0) {
                ok = false;
                log!(logger, "[mem-attack] anonymous page was NOT zeroed");
            } else {
                log!(logger, "[mem-attack] anonymous page is zeroed: ok");
            }
        }
        Err(e) => {
            ok = false;
            log!(logger, "[mem-attack] anonymous map failed: {:?}", e);
        }
    }

    // (2) Mapping physical RAM by address must be refused.
    for &phys in RAM_ADDRS {
        match xous::map_memory(MemoryAddress::new(phys), None, 4096, flags) {
            Err(xous::Error::InvalidArgument) => {
                log!(logger, "[mem-attack] {:#x}: refused (InvalidArgument): ok", phys);
            }
            Err(e) => {
                ok = false;
                log!(logger, "[mem-attack] {:#x}: refused but with the wrong error {:?}", phys, e);
            }
            Ok(range) => {
                ok = false;
                // SAFETY: the kernel mapped it (the bug under test); read it back to report a leak.
                let bytes = unsafe { core::slice::from_raw_parts(range.as_ptr(), range.len()) };
                let leaked = bytes.iter().any(|&b| b != 0);
                log!(logger, "[mem-attack] {:#x}: MAPPED but should be refused; leaked nonzero = {}", phys, leaked);
            }
        }
    }

    if ok {
        log!(logger, "MEM ATTACK TEST PASSED");
    } else {
        log!(logger, "MEM ATTACK TEST FAILED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
