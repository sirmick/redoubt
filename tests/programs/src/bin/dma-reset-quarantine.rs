//! A device that does not confirm its reset keeps its frames for ever (WP-K5b, answer 173; OD5,
//! OD6, P1-1; KERNEL-SPEC.md, Device). Built with `dma-reset-deaf`, whose first reset of each
//! device reports "not confirmed" after the real write.
//!
//! Two drivers hold DMA runs through the same empty virtio-mmio slot, each in its own budget.
//! The first faults: the slot's first reset fails, so its run is quarantined and the slot with
//! it, and every handle to the slot is gone (`BadHandle`). Its budget still carries the run; when
//! the budget is destroyed the charge moves to the parent. The co-holder is destroyed after, and
//! its run is quarantined too, although a retry would now confirm (P1-1).
//!
//! Then every free page in the tree is searched: `users` is destroyed, a checker child takes all
//! of `root`'s free pages, and it and this program each `dma_alloc` through the other empty slots
//! with halving chunk sizes until a single page is refused. The pages allocated plus the page
//! tables charged must equal the free pages recorded before, bar at most `TABLES` in a budget
//! whose last mapping could not pay for its tables, and no run may overlap a quarantined one. The
//! verdicts come from the kernel's answers.
//!
//! It runs as the bundle's first program, so it holds every device object, prints on the console
//! it maps itself, as `device-test` does, and ends with `system_reset`. The children are copies
//! of it (`spawn.rs`).

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// Pages of each driver's run.
const RUN_PAGES: usize = 4;
/// How long a check waits for anything, in microseconds.
const WAIT: u64 = 2_000_000;
/// Empty virtio-mmio slots this program can use: QEMU `virt` has 8.
const MAX_SLOTS: usize = 8;
/// Badges on the report endpoint.
const D1: u64 = 1;
const D2: u64 = 2;
const CHECKER: u64 = 3;
/// The page tables a one-page mapping can need below the root table on rv64 (Sv39). A budget with
/// fewer free pages than that can be refused the last `dma_alloc(1)`: the frame is charged, and
/// the mapping then fails for want of a table, so those pages stay free.
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
        unsafe { (0usize as *mut u64).write_volatile(1) };
    }
    test_programs::park()
}

/// What one search found, in pages.
#[derive(Clone, Copy, Default)]
struct Search {
    /// The budget's free pages before it.
    free: u64,
    allocated: u64,
    /// Page tables charged along the way: the rest of the budget's growth.
    tables: u64,
    /// Runs overlapping a quarantined one.
    overlaps: u64,
}

/// Allocate every free page of `budget` through `slots`, halving the chunk size down to one page
/// until the kernel refuses it on every slot.
fn search(budget: u32, slots: &[u32], quarantined: &[u64]) -> Search {
    let before = rd::usage(budget).expect("budget_usage");
    let free = before.pages_limit - before.pages_usage;
    let (mut allocated, mut overlaps) = (0, 0);
    let mut chunk = if free == 0 { 0 } else { 1 << free.ilog2() };
    while chunk > 0 {
        for &slot in slots {
            while let Ok((_, phys)) = rd::dma_alloc(slot, chunk as usize) {
                allocated += chunk;
                let end = phys + chunk * rd::PAGE_SIZE as u64;
                overlaps += quarantined
                    .iter()
                    .filter(|&&q| q < end && phys < q + (RUN_PAGES * rd::PAGE_SIZE) as u64)
                    .count() as u64;
            }
        }
        chunk /= 2;
    }
    let grown = rd::usage(budget).expect("budget_usage").pages_usage - before.pages_usage;
    Search { free, allocated, tables: grown - allocated, overlaps }
}

/// The checker: a send handle in slot 1, its own budget in slot 2, then `startup_byte(arg, 0)`
/// slots; the quarantined runs' addresses follow at bytes 8 and 16. It reports its search as
/// `[free, allocated, tables, overlaps]` and waits.
extern "C" fn checker(arg: usize) -> ! {
    let count = spawn::startup_byte(arg, 0) as usize;
    let mut slots = [0u32; MAX_SLOTS];
    for (i, slot) in slots[..count].iter_mut().enumerate() {
        *slot = 3 + i as u32;
    }
    let word = |at: usize| (0..8).fold(0u64, |w, i| w | (spawn::startup_byte(arg, at + i) as u64) << (8 * i));
    let s = search(2, &slots[..count], &[word(8), word(16)]);
    let report = [s.free, s.allocated, s.tables, s.overlaps].map(|w| w as usize);
    rd::send(1, &rd::body(report), None, WAIT).ok();
    test_programs::park()
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

    // --- Destroyed: each quarantined charge moves to the parent (OD5) --------------------------
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

    // --- Every free page in the tree: none of them is a quarantined frame ----------------------
    let users = rd::destroy(rd::USERS);
    // A budget's own pages are its carve less its limit, the same for every budget.
    let own = carve[0] - limit;
    let root_free = rd::free(rd::ROOT);
    let checker_budget =
        rd::create(rd::ROOT, &rd::spec(root_free - own, 1, 1)).expect("the checker's budget");
    let mut startup = [0u8; 24];
    startup[0] = others.len() as u8;
    startup[8..16].copy_from_slice(&run1.to_le_bytes());
    startup[16..].copy_from_slice(&run2.to_le_bytes());
    let mut handles = [0u32; 2 + MAX_SLOTS];
    handles[..2].copy_from_slice(&[send(CHECKER), checker_budget]);
    handles[2..2 + others.len()].copy_from_slice(others);
    let entry = checker as *const () as usize;
    let started =
        spawn::spawn(&spawn::image(), checker_budget, ep, entry, &startup, &handles[..2 + others.len()]);
    let theirs = started.ok().and_then(|_| report(ep, CHECKER)).map(|w| Search {
        free: w[0] as u64,
        allocated: w[1] as u64,
        tables: w[2] as u64,
        overlaps: w[3] as u64,
    });
    let Some(theirs) = theirs else {
        say!(out, "[dma-reset-quarantine] FAIL: the checker did not report");
        test_programs::park()
    };
    let mine = search(rd::SYSTEM, others, &[run1, run2]);
    let left = [rd::ROOT, rd::SYSTEM, checker_budget].map(rd::free);
    let free = theirs.free + mine.free;
    let taken = theirs.allocated + theirs.tables + mine.allocated + mine.tables;
    check!(
        out,
        users.is_ok()
            && free > 0
            && taken + left.iter().sum::<u64>() == free
            && left.iter().all(|&l| l <= TABLES),
        "the search took every free page a mapping could pay for: {} allocated plus {} of tables of {} free; left in root, system, the checker: {:?}",
        theirs.allocated + mine.allocated,
        theirs.tables + mine.tables,
        free,
        left
    );
    check!(
        out,
        theirs.overlaps + mine.overlaps == 0,
        "no page handed out overlaps a quarantined run ({} overlaps)",
        theirs.overlaps + mine.overlaps
    );

    say!(out, "[dma-reset-quarantine] DMA RESET QUARANTINE PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[dma-reset-quarantine] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
