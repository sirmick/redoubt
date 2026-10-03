//! Attack test: hand over pages the process has never touched.
//!
//! The stack of the program the loader starts, the one in `init`'s place, is reserved, and the
//! kernel backs each page with a zeroed frame on first touch (kernel/memory.md, "Backing and
//! zeroing"); no other process has such pages, so this program runs there. Lending, writably
//! lending or
//! transferring such a page makes the kernel deal with it inside the call. That once re-entered
//! the kernel's memory-manager cell and panicked the whole machine ("RefCell already borrowed",
//! or a spinlock deadlock with `smp`), from any unprivileged process. It also left a half-mapped
//! range half lent, and panicked when backing ran out of RAM. See `tests/lend-untouched-page.toml`.
//!
//! It serves its own log endpoint on a second thread, which receives the lends and the transfer,
//! as `log-server` did when this ran behind it. The verdict is survival only: this program makes
//! every attempt, then powers off through the Reset right it holds. A kernel that panicked on any
//! attempt never reaches the power-off. Nothing this program prints is the verdict.

#![no_std]
#![no_main]

use test_programs::rd::ResetKind;
use test_programs::{log, logsrv, op, rd};

/// More pages than the case's small guest has RAM (see the toml's `memory_mib`): mapping them
/// must fail as the caller's error. 12k pages = 48 MiB, above the 32 MiB machine.
const HUGE_PAGES: usize = 12 * 1024;

fn lend(log: u32, at: usize, npages: usize, op: usize) -> Result<(), rd::Error> {
    rd::call_waiting(log, &rd::body([op, 0, 0, 0]), rd::pages(at, npages), rd::FOREVER).map(|_| ())
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();
    logsrv::start_serving();
    // A send on its own log endpoint, which its serving thread answers.
    let log = logsrv::mint_child(logsrv::CHILD_BADGES).expect("a send on the log endpoint");
    let stack = rd::untouched_stack_page();

    // (1) A lend, and (2) a writable lend of three pages: all never touched.
    let read = lend(log, stack, 1, op::PRINT);
    let write = lend(log, stack - 3 * rd::PAGE_SIZE, 3, op::UPPERCASE);
    log!(logger, "[untouched] lend {:?}, writable lend {:?}", read, write);

    // (3) A range whose first page is mapped and whose second is not mapped at all. The lend
    // must fail as a whole and leave the first page ours, not half lent.
    let straddle = rd::map_anon(2 * rd::PAGE_SIZE, rd::rw()).expect("map");
    rd::unmap(straddle + rd::PAGE_SIZE, rd::PAGE_SIZE).expect("unmap the hole");
    let refused = lend(log, straddle, 2, op::PRINT).is_err();
    log!(logger, "[untouched] half-mapped lend refused: {}", refused);
    // The first page is still ours: write it and lend it again. If it had been half-lent, this
    // write would fault; the attempt is progress, not the verdict.
    let text = b"still ours";
    // SAFETY: `straddle` is a page mapped read-write in this process, longer than `text`.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), straddle as *mut u8, text.len()) };
    rd::call_waiting(log, &rd::body([op::PRINT, text.len(), 0, 0]), rd::pages(straddle, 1), rd::FOREVER)
        .expect("re-lend");

    // (4) Transfer a page never touched to the serving thread, which keeps it.
    let gift = stack - 6 * rd::PAGE_SIZE;
    let moved =
        rd::send_waiting(log, &rd::body([op::PRINT_AND_KEEP, 0, 0, 0]), rd::pages(gift, 1), rd::FOREVER);
    log!(logger, "[untouched] transfer {:?}", moved);

    // (5) More memory than the machine has RAM: the caller's error, not a kernel panic.
    let beyond = rd::map_anon(HUGE_PAGES * rd::PAGE_SIZE, rd::rw()).map(|_| ());
    log!(logger, "[untouched] map beyond RAM: {:?}", beyond);

    // Survived every attempt. The clean power-off, which only this program can ask for, is the
    // verdict.
    log!(logger, "[untouched] every attempt made; powering off");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
