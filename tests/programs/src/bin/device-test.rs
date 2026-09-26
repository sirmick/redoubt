//! Device objects and the Redoubt memory calls: `map_device`, `dma_alloc`, `system_reset`,
//! `map_anon`, `unmap` and `set_flags` (kernel/devices.md, kernel/memory.md; R11).
//!
//! It runs as the bundle's first program, so it holds every device object the loader made
//! (kernel `device.rs`; docs/plan/m1-separation.md moves them to `init`), and it prints through
//! the console it maps itself -- there is no `log-server` in this case, because only one process
//! can own the UART. Its last act is `system_reset`, so the case's verdict is its lines *and* a
//! clean power-off: a program that never reached the end cannot produce one.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Error, MemFlags, ResetKind};
use test_programs::console::{self, Console};

/// Pages a DMA buffer is asked for.
const DMA_PAGES: usize = 4;

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out, "[device] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out, $($arg)*).ok();
    }};
}

/// The first MMIO device that carries the DMA flag, found by asking: `dma_alloc` is
/// `NotPermitted` without it and `WrongObject` on an IRQ or the Reset right, so a driver can
/// tell which of its handles is a bus master without naming any address.
fn dma_device(mut devices: core::ops::Range<u32>) -> Option<(u32, usize, u64)> {
    devices.find_map(|h| rd::dma_alloc(h, DMA_PAGES).ok().map(|(a, p)| (h, a, p)))
}

fn word(at: usize) -> u64 { rd::peek(at) }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // The console's registers, through its device object. Nothing here names an address,
    // and the length comes back with it (kernel/devices.md, `map_device`).
    let (uart, uart_len) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    say!(out, "[device] mapped the console");

    let devices = rd::OTHER_DEVICES..rd::log_rx();
    let free = rd::first_free();

    // --- map_device ----------------------------------------------------------------------
    // Mapping the same device twice gives two addresses, both the kernel's choice (R11), and
    // the same length each time: whole pages, and the console's registers fit in one.
    let again = rd::map_device(rd::CONSOLE_MMIO);
    let same_len = again.map(|(_, len)| len) == Ok(uart_len);
    check!(out, again.is_ok() && again.map(|(at, _)| at) != Ok(uart) && same_len,
        "a second map_device -> another address, {} bytes both times", uart_len);
    check!(out, uart_len >= rd::PAGE_SIZE && uart_len % rd::PAGE_SIZE == 0,
        "map_device's length is a whole number of pages");
    check!(out, rd::map_device(rd::CONSOLE_IRQ) == Err(Error::WrongObject)
        && rd::map_device(rd::RESET) == Err(Error::WrongObject)
        && rd::map_device(rd::SYSTEM) == Err(Error::WrongObject)
        && rd::map_device(free) == Err(Error::BadHandle),
        "map_device refuses an irq, the reset right, a budget and an index it does not hold");

    // --- map_anon, unmap, set_flags (R11) ---------------------------------------------------
    let anon = rd::map_anon(3 * rd::PAGE_SIZE, rd::rw()).expect("map_anon");
    let zeroed = (0..3).all(|i| word(anon + i * rd::PAGE_SIZE) == 0);
    rd::poke(anon, 0x5ec_2e7);
    check!(out, zeroed && word(anon) == 0x5ec_2e7, "map_anon: 3 zeroed pages, writable");
    // The bad shapes its row names: no length, a length or address that is not whole pages,
    // no permission, and writable without readable. W+X cannot even be encoded.
    let bad = [
        rd::map_anon(0, rd::rw()),
        rd::map_anon(rd::PAGE_SIZE - 1, rd::rw()),
        rd::map_anon(rd::PAGE_SIZE, MemFlags::NONE),
        rd::map_anon(rd::PAGE_SIZE, MemFlags::WRITE),
    ];
    check!(out, bad.iter().all(|r| *r == Err(Error::InvalidArgument)), "map_anon refuses {:?}", bad);
    // Permissions may be dropped and added, as long as the page is never writable and
    // executable at once: that is how a program turns a page it wrote into code.
    let flips = [
        rd::set_flags(anon, rd::PAGE_SIZE, MemFlags::READ),
        rd::set_flags(anon, rd::PAGE_SIZE, MemFlags::READ | MemFlags::EXECUTE),
        rd::set_flags(anon, rd::PAGE_SIZE, rd::rw()),
    ];
    check!(out, flips.iter().all(|r| r.is_ok()) && word(anon) == 0x5ec_2e7,
        "set_flags: read, then read-execute, then read-write again");
    let bad = [
        rd::set_flags(anon, 0, rd::rw()),
        rd::set_flags(anon + 1, rd::PAGE_SIZE, rd::rw()),
        rd::set_flags(anon, 4 * rd::PAGE_SIZE, rd::rw()),
        rd::set_flags(0, rd::PAGE_SIZE, rd::rw()),
        rd::unmap(anon, 4 * rd::PAGE_SIZE),
        rd::unmap(anon + 1, rd::PAGE_SIZE),
        rd::unmap(0, rd::PAGE_SIZE),
        rd::unmap(usize::MAX - rd::PAGE_SIZE + 1, rd::PAGE_SIZE),
    ];
    check!(out, bad.iter().all(|r| *r == Err(Error::InvalidArgument)),
        "set_flags and unmap refuse a range that is not the caller's own whole pages");
    // Unmapping gives the pages back; the next `map_anon` is zero again whatever lands there.
    check!(out, rd::unmap(anon, 3 * rd::PAGE_SIZE) == Ok(()), "unmap: the three pages go back");
    // Measured with the page tables of that region already in place, so what this compares is
    // the pages themselves: three charged by `map_anon`, three returned by `unmap`.
    let before = rd::usage(rd::SYSTEM).expect("system usage");
    let reused = rd::map_anon(3 * rd::PAGE_SIZE, rd::rw()).expect("map_anon again");
    let charged = rd::usage(rd::SYSTEM).expect("system usage").pages_usage;
    // R11's sharp half: a page a process gets never holds what the last one left there.
    check!(out, (0..3).all(|i| word(reused + i * rd::PAGE_SIZE) == 0),
        "pages come back zeroed, whatever was written in them before");
    check!(out, charged == before.pages_usage + 3, "map_anon charges its pages to the caller's budget");
    check!(out, rd::unmap(reused, 3 * rd::PAGE_SIZE) == Ok(()), "and go back again");
    let after = rd::usage(rd::SYSTEM).expect("system usage");
    check!(out, after.pages_usage == before.pages_usage, "unmap returns every page it took");

    // --- dma_alloc -------------------------------------------------------------------------
    check!(out, rd::dma_alloc(rd::CONSOLE_MMIO, 1) == Err(Error::NotPermitted),
        "dma_alloc on a device with no DMA flag -> NotPermitted");
    check!(out, rd::dma_alloc(rd::CONSOLE_MMIO, 0) == Err(Error::InvalidArgument)
        && rd::dma_alloc(rd::CONSOLE_IRQ, 1) == Err(Error::WrongObject)
        && rd::dma_alloc(rd::RESET, 1) == Err(Error::WrongObject)
        && rd::dma_alloc(free, 1) == Err(Error::BadHandle),
        "dma_alloc refuses no pages, an irq, the reset right and an index it does not hold");
    match dma_device(devices) {
        None => check!(out, false, "no device carries the DMA flag"),
        Some((h, at, phys)) => {
            let zeroed = (0..DMA_PAGES).all(|i| word(at + i * rd::PAGE_SIZE) == 0);
            for i in 0..DMA_PAGES {
                rd::poke(at + i * rd::PAGE_SIZE, 0x1000 + i as u64);
            }
            let writable = (0..DMA_PAGES).all(|i| word(at + i * rd::PAGE_SIZE) == 0x1000 + i as u64);
            let aligned = phys != 0 && phys % rd::PAGE_SIZE as u64 == 0;
            check!(out, zeroed && writable && aligned,
                "dma_alloc on device handle {}: {} zeroed pages, physical address page-aligned",
                h, DMA_PAGES);
            // A second buffer is somewhere else: the kernel never hands the same frames twice.
            let other = rd::dma_alloc(h, DMA_PAGES).expect("a second DMA buffer");
            check!(out, other.1 != phys && other.0 != at, "a second dma_alloc is a different buffer");
        }
    }

    // --- system_reset ------------------------------------------------------------------------
    check!(out, rd::system_reset(rd::CONSOLE_MMIO, ResetKind::PowerOff) == Err(Error::WrongObject)
        && rd::system_reset(rd::CONSOLE_IRQ, ResetKind::PowerOff) == Err(Error::WrongObject)
        && rd::system_reset(free, ResetKind::PowerOff) == Err(Error::BadHandle),
        "system_reset refuses every handle but the Reset right");
    say!(out, "[device] DEVICE TEST PASSED");
    // The Reset right ends the case: the bench requires the power-off, so nothing this program
    // printed would pass on its own.
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[device] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
