//! Regression case for `tables_needed` across gigabytes (K5a review round 2, N1).
//!
//! `tables_needed` (kernel/src/arch/riscv/mem.rs) once deduplicated missing tables by their
//! index in their parent table. On Sv39 a level-1 index recurs in every gigabyte, so two missing
//! level-0 tables at index 0 of consecutive gigabytes, with every table between them present,
//! were counted as one. `map_fixed`'s charge check then let through a request one page table
//! short, and the mapping loop's `alloc_page` `.expect` panicked the kernel. The model's
//! `table_keys` was always right, so only a boot case can catch a regression.
//!
//! The range is `[3 GiB + 2 MiB - rd::PAGE_SIZE, 4 GiB + rd::PAGE_SIZE)`: 261634 pages, which need exactly the two
//! missing level-0 tables at index 0 of gigabytes 3 and 4. With `free == pages + 1` the pages
//! check passes and the page-tables check must refuse: OutOfMemory, nothing charged. Only this
//! tight half runs. The exact half (`free == pages + 2` succeeds) zeroes 1 GiB, about a minute
//! under QEMU. `memory_mib = 4608` because `system` gets a quarter of RAM (budget.rs,
//! `boot_budgets`) and must be able to afford the pages. The guest touches only ~130 MiB.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, ResetKind};
use uart_16550::MmioSerialPort;

static CONSOLE: AtomicUsize = AtomicUsize::new(0);

fn check(out: &mut MmioSerialPort, ok: bool, label: &str) {
    writeln!(out, "[map-fixed-tables] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
    assert!(ok, "{}", label);
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(out).ok();
    case::run(&mut out);
    writeln!(out, "[map-fixed-tables] MAP-FIXED TABLES TEST PASSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    loop {
        rd::receive(None, rd::FOREVER, 0).ok();
    }
}

/// The case itself, rv64 only (the toml says so), but the bin must still build on rv32. Sv32 has
/// one level below the root, whose index never recurs along a range, and 2 GiB of user space.
#[cfg(target_pointer_width = "64")]
mod case {
    use test_programs::rd::{self, Error};
    use uart_16550::MmioSerialPort;

    use super::check;

    const SPAN: usize = 2 << 20;

    fn usage_pages() -> u64 { rd::usage(rd::SYSTEM).unwrap().pages_usage }

    /// Bring `system`'s free pages to exactly `target` with filler in gigabyte 2, as
    /// `map-fixed-attack.rs`'s `page_table_charge` does: open 2 MiB tables by mapping their first
    /// page, then fill their other slots, each costing exactly one page once its table exists.
    fn fill_to(target: u64) {
        const FILL: usize = 0x8000_0000;
        const SLOTS: usize = SPAN / rd::PAGE_SIZE - 1;
        let (mut opened, mut filled) = (0usize, 0usize);
        loop {
            let short = rd::free(rd::SYSTEM).checked_sub(target).expect("the filler overshot") as usize;
            if short == 0 {
                return;
            }
            if short > opened * SLOTS - filled {
                assert!(opened < (1 << 30) / SPAN, "the filler must stay inside gigabyte 2");
                rd::map_fixed(FILL + opened * SPAN, rd::PAGE_SIZE, rd::rw()).expect("open a filler table");
                opened += 1;
                continue;
            }
            let (table, slot) = (filled / SLOTS, filled % SLOTS);
            let n = short.min(SLOTS - slot);
            rd::map_fixed(FILL + table * SPAN + (1 + slot) * rd::PAGE_SIZE, n * rd::PAGE_SIZE, rd::rw()).expect("fill");
            filled += n;
        }
    }

    pub fn run(out: &mut MmioSerialPort) {
        const G3: usize = 0xC000_0000;
        const G4: usize = 0x1_0000_0000;
        // Build every table between the two holes: L1(G3) and the level-0 tables of G3's blocks
        // 1..=511, then L1(G4) and the level-0 table of G4's block 1. Map and unmap one page in
        // each: the kernel keeps a table once it exists, so 514 tables stay charged. If it ever
        // frees them, this check says so instead of the case silently testing nothing.
        let before = usage_pages();
        for block in 1..512 {
            rd::map_fixed(G3 + block * SPAN, rd::PAGE_SIZE, rd::rw()).expect("open a G3 table");
            rd::unmap(G3 + block * SPAN, rd::PAGE_SIZE).expect("unmap");
        }
        rd::map_fixed(G4 + SPAN, rd::PAGE_SIZE, rd::rw()).expect("open a G4 table");
        rd::unmap(G4 + SPAN, rd::PAGE_SIZE).expect("unmap");
        check(out, usage_pages() - before == 514, "setup left 514 page tables");

        let at = G3 + SPAN - rd::PAGE_SIZE;
        let pages = ((G4 + rd::PAGE_SIZE - at) / rd::PAGE_SIZE) as u64;
        fill_to(pages + 1);
        let before = usage_pages();
        let r = rd::map_fixed(at, pages as usize * rd::PAGE_SIZE, rd::rw());
        check(out, r == Err(Error::OutOfMemory), "two missing tables a gigabyte apart are both counted");
        check(out, usage_pages() == before, "nothing charged");
    }
}

#[cfg(not(target_pointer_width = "64"))]
mod case {
    pub fn run(out: &mut uart_16550::MmioSerialPort) { super::check(out, false, "this case is rv64 only") }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = CONSOLE.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: only this process initializes CONSOLE, to its granted UART page.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[map-fixed-tables] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
