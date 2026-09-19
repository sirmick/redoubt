//! Attack test for `MapMemory` (see the fix in `kernel/src/syscall.rs`).
//!
//! Two invariants a hostile process must not be able to break:
//!   1. RAM handed out anonymously (`phys = 0`) is zeroed before the process sees it.
//!   2. A process cannot map a physical RAM frame *by address*: that would let it point at
//!      another process's freed page and read what was left there.
//!
//! `mem-victim` has just freed pages holding its secret. This program takes as many anonymous
//! pages as the victim freed and more, and tries to map several physical addresses inside main
//! RAM, and lends every page it gets to the victim. The victim, not this program, says whether
//! any held data (README "Writing an attack case"); this program's own reports can only fail
//! the case.

#![no_std]
#![no_main]

use test_programs::{log, mem, Logger};
use xous::{MemoryAddress, MemoryFlags, MemoryRange, Message, CID};

/// Physical addresses inside QEMU `virt` main RAM (base 0x8000_0000, 256 MiB, on both
/// widths). Mapping any of these by explicit address must be refused.
const RAM_ADDRS: &[usize] = &[0x8000_0000, 0x8100_0000, 0x88ff_f000];
/// Anonymous pages to take: twice what the victim freed, so its frames are among them if reused.
const PAGES: usize = 128;

fn lend(victim: CID, page: MemoryRange) {
    xous::send_message(victim, Message::new_lend(mem::CHECK, page, None, None)).expect("couldn't lend to the victim");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let sid = xous::SID::from_bytes(mem::VICTIM_ADDRESS).unwrap();
    let victim = xous::connect(sid).expect("couldn't connect to the victim");
    let flags = MemoryFlags::R | MemoryFlags::W;

    // (1) Anonymous RAM: every page goes to the victim to inspect.
    let mut lent = 0;
    for _ in 0..PAGES {
        match xous::map_memory(None, None, 4096, flags) {
            Ok(page) => {
                // Touch the page first. Lending a page never touched (the kernel backs anonymous
                // pages on first use) panics the kernel today ("RefCell already borrowed",
                // kernel/src/cell.rs): a kernel bug, reported with WP-T1b and left to the kernel
                // track. Reading does not change what the victim will see.
                // SAFETY: the kernel just mapped this page readable for us.
                unsafe { page.as_ptr().read_volatile() };
                lend(victim, page);
                lent += 1;
            }
            Err(e) => log!(logger, "[mem-attack] anonymous map failed: {:?}", e),
        }
    }
    log!(logger, "[mem-attack] lent {} anonymous pages to the victim", lent);

    // (2) Mapping physical RAM by address must be refused; anything mapped goes to the victim.
    for &phys in RAM_ADDRS {
        match xous::map_memory(MemoryAddress::new(phys), None, 4096, flags) {
            Err(xous::Error::InvalidArgument) => {
                log!(logger, "[mem-attack] {:#x}: refused (InvalidArgument)", phys);
            }
            Err(e) => log!(logger, "[mem-attack] {:#x}: refused with another error {:?}", phys, e),
            Ok(page) => {
                log!(logger, "[mem-attack] {:#x}: MAPPED, lending it to the victim", phys);
                lend(victim, page);
            }
        }
    }

    xous::send_message(victim, Message::new_blocking_scalar(mem::DONE, 0, 0, 0, 0)).expect("couldn't reach the victim");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
