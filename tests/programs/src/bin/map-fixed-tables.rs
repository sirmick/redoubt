//! `tables_needed` across gigabytes on Sv39 and across root entries on Sv32 (R22;
//! kernel/memory.md, `map_fixed`).
//!
//! `tables_needed` (kernel/src/arch/riscv/mem.rs) once deduplicated missing tables by their
//! index in their parent table. On Sv39 a level-1 index recurs in every gigabyte, so two missing
//! level-0 tables at index 0 of consecutive gigabytes, with every table between them present,
//! were counted as one, and the mapping loop's `alloc_page` `.expect` panicked the kernel. Now
//! that a table mapping nothing is freed (kernel/memory.md, "Page tables"), no table is ever
//! present inside a free range, so that shape cannot be built from userspace any more. What
//! stays is the exact count across the boundary, and the setup shows `unmap` freeing tables.
//!
//! The range is `[3 GiB + 2 MiB - rd::PAGE_SIZE, 4 GiB + rd::PAGE_SIZE)`: 261634 pages, which
//! need 515 tables: gigabyte 3's level-1 table and its 512 level-0 tables, and gigabyte 4's
//! level-1 table and its first level-0 table. With `free == pages + 514` the pages check passes
//! and the page-tables check must refuse: OutOfMemory, nothing charged. Only this tight half
//! runs. The exact half zeroes 1 GiB, about a minute under QEMU. `memory_mib = 4608` because
//! `system` gets a quarter of RAM (budget.rs, `boot_budgets`) and must be able to afford the
//! pages. The guest touches only ~130 MiB.
//!
//! Sv32 has one level below the root, whose index never recurs along a range, and no RAM for a
//! 1 GiB range (`tests/map-fixed-tables-rv32.toml`). Its form is the same two checks over three
//! 4 MiB root entries: the setup opens and frees a leaf table in each, and a range from the last
//! page of the first to the first page of the third needs 3 tables.
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

/// Bring `system`'s free pages to exactly `target` with filler at `fill`: open leaf tables of
/// `span` bytes by mapping their first page, then fill their other slots, each costing exactly
/// one page once its table exists (as `map-fixed-attack.rs`'s `page_table_charge` does).
fn fill_to(target: u64, fill: usize, span: usize, tables: usize) {
    let slots = span / rd::PAGE_SIZE - 1;
    let (mut opened, mut filled) = (0usize, 0usize);
    loop {
        let short = rd::free(rd::SYSTEM).checked_sub(target).expect("the filler overshot") as usize;
        if short == 0 {
            return;
        }
        if short > opened * slots - filled {
            assert!(opened < tables, "the filler must stay inside its area");
            rd::map_fixed(fill + opened * span, rd::PAGE_SIZE, rd::rw()).expect("open a filler table");
            opened += 1;
            continue;
        }
        let (table, slot) = (filled / slots, filled % slots);
        let n = short.min(slots - slot);
        rd::map_fixed(fill + table * span + (1 + slot) * rd::PAGE_SIZE, n * rd::PAGE_SIZE, rd::rw())
            .expect("fill");
        filled += n;
    }
}

fn usage_pages() -> u64 { rd::usage(rd::SYSTEM).unwrap().pages_usage }

#[cfg(target_pointer_width = "64")]
mod case {
    use test_programs::rd::{self, Error};
    use uart_16550::MmioSerialPort;

    use super::{check, fill_to, usage_pages};

    const SPAN: usize = 2 << 20;

    pub fn run(out: &mut MmioSerialPort) {
        const G3: usize = 0xC000_0000;
        const G4: usize = 0x1_0000_0000;
        // The shape the old miscount needed: map one page in each of gigabyte 3's blocks
        // 1..=511 and in gigabyte 4's block 1, which makes 514 tables, then unmap each. Every
        // table goes with its page, so none is left inside the range below.
        let before = usage_pages();
        for block in 1..512 {
            rd::map_fixed(G3 + block * SPAN, rd::PAGE_SIZE, rd::rw()).expect("open a G3 table");
            rd::unmap(G3 + block * SPAN, rd::PAGE_SIZE).expect("unmap");
        }
        rd::map_fixed(G4 + SPAN, rd::PAGE_SIZE, rd::rw()).expect("open a G4 table");
        rd::unmap(G4 + SPAN, rd::PAGE_SIZE).expect("unmap");
        check(out, usage_pages() == before, "unmap freed every page table the setup made");

        const TABLES: u64 = 515;
        let at = G3 + SPAN - rd::PAGE_SIZE;
        let pages = ((G4 + rd::PAGE_SIZE - at) / rd::PAGE_SIZE) as u64;
        // The filler: gigabyte 2.
        fill_to(pages + TABLES - 1, 0x8000_0000, SPAN, (1 << 30) / SPAN);
        let before = usage_pages();
        let r = rd::map_fixed(at, pages as usize * rd::PAGE_SIZE, rd::rw());
        check(out, r == Err(Error::OutOfMemory), "one page table short across two gigabytes is refused");
        check(out, usage_pages() == before, "nothing charged");
    }
}

#[cfg(target_pointer_width = "32")]
mod case {
    use test_programs::rd::{self, Error};
    use uart_16550::MmioSerialPort;

    use super::{check, fill_to, usage_pages};

    /// A root entry's span, which one leaf table maps.
    const SPAN: usize = 4 << 20;

    pub fn run(out: &mut MmioSerialPort) {
        // Three root entries nothing else in this process uses.
        const R0: usize = 0x5000_0000;
        let before = usage_pages();
        for root in 0..3 {
            rd::map_fixed(R0 + root * SPAN + SPAN / 2, rd::PAGE_SIZE, rd::rw()).expect("open a table");
            rd::unmap(R0 + root * SPAN + SPAN / 2, rd::PAGE_SIZE).expect("unmap");
        }
        check(out, usage_pages() == before, "unmap freed every page table the setup made");

        const TABLES: u64 = 3;
        let at = R0 + SPAN - rd::PAGE_SIZE;
        let pages = ((R0 + 2 * SPAN + rd::PAGE_SIZE - at) / rd::PAGE_SIZE) as u64;
        // The filler: between the image and the message area.
        fill_to(pages + TABLES - 1, 0x3000_0000, SPAN, 0x1000_0000 / SPAN);
        let before = usage_pages();
        let r = rd::map_fixed(at, pages as usize * rd::PAGE_SIZE, rd::rw());
        check(out, r == Err(Error::OutOfMemory), "one page table short across three root entries is refused");
        check(out, usage_pages() == before, "nothing charged");
    }
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
