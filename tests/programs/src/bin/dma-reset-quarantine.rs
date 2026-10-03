//! A device that does not confirm its reset keeps its frames for ever (I16; kernel/devices.md,
//! "Quarantine"). Built with `dma-reset-deaf`, whose first reset of each
//! device reports "not confirmed" after the real write.
//!
//! Two drivers hold DMA runs through the same empty virtio-mmio slot, each in its own budget.
//! The first faults: the slot's first reset fails, so its run is quarantined and the slot with
//! it, and every handle to the slot is gone (`BadHandle`). Its budget still carries the run; when
//! the budget is destroyed the charge moves to the parent. The co-holder is destroyed after, and
//! its run is quarantined too, although a retry would now confirm (a quarantined device never
//! counts as reset).
//!
//! Then the DMA pool is searched: `users` is destroyed, which gives `root`, this program's budget,
//! room for all of the pool and its page tables, and this program `dma_alloc`s through the other empty slots
//! with halving chunk sizes until a single page is refused. The pages allocated plus the two quarantined runs
//! must be the whole pool (`DMA_POOL_PAGES`), no run may overlap a quarantined one, and the last
//! `dma_alloc(1)` must be refused `OutOfMemory` while the budget still has room, so the refusal is the
//! pool's. The verdicts come from the kernel's answers.
//!
//! It runs in `init`'s place, so it holds every device object, prints on the console it maps
//! itself, as `device-test` does, and ends with `system_reset`. The children are copies
//! of it (`spawn.rs`).

#![no_std]
#![no_main]

use core::fmt::Write;

use redoubt_layout::DMA_POOL_PAGES;
use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// Pages of each driver's run.
const RUN_PAGES: usize = 4;
/// How long a check waits for a driver's report or exit, in microseconds of guest time, which
/// follows the host's clock: well inside the case's timeout, so a check that runs out says which.
const WAIT: u64 = 10_000_000;
/// Empty virtio-mmio slots this program can use: QEMU `virt` has 8.
const MAX_SLOTS: usize = 8;
/// Badges on the report endpoint.
const D1: u64 = 1;
const D2: u64 = 2;
/// The page tables a one-page mapping can need below the root table on rv64 (Sv39). A budget with
/// no more free pages than that could refuse the last `dma_alloc(1)` for want of a table, not of
/// the pool.
const TABLES: u64 = 2;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[dma-reset-quarantine] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// A driver: the slot in its slot 1, a send handle in slot 2. It allocates one run, reports its
/// physical address, and then faults (a startup block of `[1]`) or waits to be destroyed.
extern "C" fn driver(arg: usize) -> ! {
    let phys = rd::dma_alloc(1, RUN_PAGES).map_or(0, |(_, p)| p as usize);
    if rd::send(2, &rd::body([phys, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    if arg != 0 && spawn::startup_byte(arg, 0) == 1 {
        // SAFETY: not sound, and that is the point: a store to address 0, which no process ever
        // has mapped, must trap, so that the kernel reports the driver `faulted`.
        unsafe { (0usize as *mut usize).write_volatile(1) };
    }
    test_programs::park()
}

/// What the search of the pool found.
struct Search {
    /// Pages allocated.
    allocated: u64,
    /// Runs overlapping a quarantined one.
    overlaps: u64,
    /// The last refusal, of a single page.
    last: Result<(), Error>,
}

/// Allocate every free page of the DMA pool through `slots`, halving the chunk size from the pool's
/// down to one page until the kernel refuses it on every slot.
fn search(slots: &[u32], quarantined: &[u64]) -> Search {
    let (mut allocated, mut overlaps, mut last) = (0, 0, Ok(()));
    let mut chunk = 1 << DMA_POOL_PAGES.ilog2();
    while chunk > 0 {
        for &slot in slots {
            loop {
                match rd::dma_alloc(slot, chunk) {
                    Ok((_, phys)) => {
                        allocated += chunk as u64;
                        let end = phys + (chunk * rd::PAGE_SIZE) as u64;
                        overlaps += quarantined
                            .iter()
                            .filter(|&&q| q < end && phys < q + (RUN_PAGES * rd::PAGE_SIZE) as u64)
                            .count() as u64;
                    }
                    Err(e) => {
                        last = Err(e);
                        break;
                    }
                }
            }
        }
        chunk /= 2;
    }
    Search { allocated, overlaps, last }
}

/// The next message from `badge` on `ep`, skipping exit notices.
fn report(ep: u32, badge: u64) -> Option<[usize; rd::WORDS]> {
    loop {
        match rd::receive(Some(ep), WAIT, 0).ok()? {
            Received::Message(m) if m.badge == badge => return Some(m.body.words),
            Received::Exit(_) => continue,
            _ => return None,
        }
    }
}

fn pages(budget: u32) -> u64 { rd::usage(budget).expect("budget_usage").pages_usage }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[dma-reset-quarantine] mapped the console");

    // Every empty virtio slot this program holds. Mapping one joins this program's reset set,
    // which matters only if it dies, and it never does before the power-off.
    let mut slots = [0u32; MAX_SLOTS];
    let mut count = 0;
    for h in rd::OTHER_DEVICES..rd::first_free() {
        let Ok((at, len)) = rd::map_device(h) else { continue };
        // SAFETY: `at` maps `len` bytes of this device's registers; its first word is the magic.
        let magic = unsafe { (at as *const u32).read_volatile() };
        if len >= rd::PAGE_SIZE && magic == u32::from_le_bytes(*b"virt") && count < MAX_SLOTS {
            slots[count] = h;
            count += 1;
        }
    }
    if count < 2 {
        say!(out, "[dma-reset-quarantine] FAIL: {} empty virtio slots, not 2", count);
        test_programs::park()
    }
    let (deaf, others) = (slots[0], &slots[1..count]);

    // --- Two drivers on the deaf slot; the first faults --------------------------------------
    let ep = rd::endpoint_create().expect("an endpoint");
    let send = |badge| rd::mint_from_handle(ep, badge, None).expect("a send handle");
    let limit = spawn::image().pages() as u64 + 96;
    let mut carve = [0u64; 2];
    let mut budgets = [0u32; 2];
    for (b, c) in budgets.iter_mut().zip(carve.iter_mut()) {
        let before = pages(rd::SYSTEM);
        *b = rd::create(rd::SYSTEM, &rd::spec(limit, 1, 10)).expect("a driver budget");
        *c = pages(rd::SYSTEM) - before;
    }
    let empty = pages(budgets[0]);
    let entry = driver as *const () as usize;
    let d2 = spawn::spawn(&spawn::image(), budgets[1], ep, entry, &[], &[deaf, send(D2)]);
    let run2 = d2.ok().and_then(|_| report(ep, D2)).map_or(0, |w| w[0] as u64);
    let d1 = spawn::spawn(&spawn::image(), budgets[0], ep, entry, &[1], &[deaf, send(D1)]);
    let run1 = d1.ok().and_then(|_| report(ep, D1)).map_or(0, |w| w[0] as u64);
    let faulted = loop {
        match rd::receive(Some(ep), WAIT, 0) {
            Ok(Received::Exit(n)) => break n.cause == Cause::Faulted,
            Ok(_) => continue,
            Err(_) => break false,
        }
    };
    check!(
        out,
        run1 != 0 && run2 != 0 && faulted,
        "two drivers hold a run each on the deaf slot ({:#x}, {:#x}); the first faulted",
        run1,
        run2
    );

    // --- The slot is gone, and the faulted driver's budget still pays for its run --------------
    let mapped = rd::map_device(deaf).map(|_| ());
    let allocated = rd::dma_alloc(deaf, 1).map(|_| ());
    check!(
        out,
        mapped == Err(Error::BadHandle) && allocated == Err(Error::BadHandle),
        "every handle to the quarantined slot is gone (map_device {:?}, dma_alloc {:?})",
        mapped,
        allocated
    );
    let held = pages(budgets[0]);
    check!(
        out,
        held == empty + RUN_PAGES as u64,
        "the faulted driver's budget still carries its quarantined run ({} -> {} pages)",
        empty,
        held
    );

    // --- Destroyed: each quarantined charge moves to the parent --------------------------------
    for (i, (&b, &c)) in budgets.iter().zip(carve.iter()).enumerate() {
        let before = pages(rd::SYSTEM);
        let destroyed = rd::destroy(b);
        let after = pages(rd::SYSTEM);
        check!(
            out,
            destroyed.is_ok() && after == before - c + RUN_PAGES as u64,
            "driver {} destroyed: its carve of {} came back and its run's {} pages are charged to system ({} -> {})",
            i + 1,
            c,
            RUN_PAGES,
            before,
            after
        );
    }

    // --- The whole pool: none of it is a quarantined frame ----------------------------------------
    let users = rd::destroy(rd::USERS);
    let room = rd::free(rd::ROOT);
    let s = search(others, &[run1, run2]);
    let left = rd::free(rd::ROOT);
    check!(
        out,
        s.allocated + 2 * RUN_PAGES as u64 == DMA_POOL_PAGES as u64,
        "the search took the whole pool but the quarantined runs: {} allocated plus 2 runs of {} of {}",
        s.allocated,
        RUN_PAGES,
        DMA_POOL_PAGES
    );
    check!(out, s.overlaps == 0, "no page handed out overlaps a quarantined run ({} overlaps)", s.overlaps);
    check!(
        out,
        users.is_ok()
            && room > DMA_POOL_PAGES as u64 + TABLES
            && s.last == Err(Error::OutOfMemory)
            && left > TABLES,
        "the pool, not the budget, refused the last page: {:?} with {} of {} pages left (users destroyed: {:?})",
        s.last,
        left,
        room,
        users
    );

    say!(out, "[dma-reset-quarantine] DMA RESET QUARANTINE PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[dma-reset-quarantine] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
