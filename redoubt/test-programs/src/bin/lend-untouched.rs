//! Attack test: hand over anonymous pages the process has never touched.
//!
//! `map_memory(None, None, ..)` only reserves pages; the kernel backs each one with a zeroed
//! frame on first touch. Lending, mutably lending or moving such a page makes the kernel back
//! it inside the syscall. That once re-entered the kernel's memory-manager cell and panicked
//! the whole machine ("RefCell already borrowed", or a spinlock deadlock with `smp`), from any
//! unprivileged process. It also left a half-mapped range half lent, and panicked when backing
//! ran out of RAM. See `redoubt/tests/lend-untouched-page.toml`.
//!
//! The verdict is survival only: this program makes every attempt, then reports to
//! `attack-checker`, which powers off under its own PID. A kernel that panicked on any attempt
//! never reaches the power-off. Nothing this program prints is the verdict.

#![no_std]
#![no_main]

use test_programs::{Logger, checker, log, op};
use xous::{MemoryMessage, MemoryRange, MemorySize, Message};

fn untouched(pages: usize) -> MemoryRange {
    xous::map_memory(None, None, pages * 4096, xous::MemoryFlags::R | xous::MemoryFlags::W)
        .expect("couldn't reserve pages")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let server = logger.cid;

    // (1) Borrow, and (2) mutable borrow of three pages: all never touched.
    let page = untouched(1);
    xous::send_message(server, Message::new_lend(op::PRINT, page, None, None)).expect("lend");
    let pages = untouched(3);
    xous::send_message(
        server,
        Message::new_lend_mut(op::UPPERCASE, pages, None, MemorySize::new(pages.len())),
    )
    .expect("lend_mut");

    // (3) A range whose first page is reserved and whose second is not mapped at all. The lend
    // must fail as a whole (BadAddress on the hole) and leave the first page ours, not half
    // lent. `untouched(2)` reserves both pages, so unmap the second to open the hole; the pages
    // are contiguous because a fresh reservation is one run.
    let straddle = untouched(2);
    let second = straddle.as_ptr() as usize + 4096;
    // SAFETY: `second` is the second page of the reservation above; the kernel validates it.
    xous::unmap_memory(unsafe { MemoryRange::new(second, 4096) }.expect("range")).expect("unmap the hole");
    let refused = xous::send_message(server, Message::new_lend(op::PRINT, straddle, None, None)).is_err();
    log!(logger, "[untouched] half-mapped lend refused: {}", refused);
    // The first page is still ours: touch it and lend it again. If it had been half-lent, this
    // write would fault; the attempt is progress, not the verdict.
    // SAFETY: the first page of the reservation above.
    let first = unsafe { MemoryRange::new(straddle.as_ptr() as usize, 4096) }.expect("range");
    let text = b"still ours";
    // SAFETY: `first` is a page mapped R|W in this process, longer than `text`.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), first.as_ptr() as *mut u8, text.len()) };
    xous::send_message(server, Message::new_lend(op::PRINT, first, None, MemorySize::new(text.len())))
        .expect("re-lend");

    // (4) Move an untouched page to log-server, which keeps it.
    let gift = untouched(1);
    xous::send_message(
        server,
        Message::Move(MemoryMessage { id: op::PRINT_AND_KEEP, buf: gift, offset: None, valid: None }),
    )
    .expect("move");

    // (5) Lend more untouched memory than the machine has RAM. Backing it runs out of RAM part
    // way; that must be this call's error, not a kernel panic. Unmapping gives the RAM back.
    let huge = untouched(HUGE_PAGES);
    let beyond = xous::send_message(server, Message::new_lend(op::PRINT, huge, None, None));
    log!(logger, "[untouched] lend beyond RAM: {:?}", beyond);
    xous::unmap_memory(huge).expect("couldn't unmap the huge range");

    // Survived every attempt. The checker powers off under its own PID; that clean power-off is
    // the verdict.
    checker::done();
    test_programs::park()
}

/// More pages than the case's small guest has RAM (see the toml's `memory_mib`): backing them
/// must run out part way. 12k pages = 48 MiB, above the 32 MiB machine.
const HUGE_PAGES: usize = 12 * 1024;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
