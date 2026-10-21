//! The page a device object costs its owner (kernel/devices.md, "Device objects"). At boot every
//! device object is `system`'s and `system` holds nothing else, so its usage is exactly one page
//! per device handle this program, in `init`'s place, was given. Then a quarantine destroys one
//! object (built with `dma-reset-deaf`: a driver that allocated through an empty virtio-mmio slot
//! faults, the slot's first reset does not confirm, kernel/devices.md "Quarantine"), and `system`'s
//! usage falls by exactly that page while every handle to the slot is gone. The driver's budget is
//! carved from `system` before the measure, so nothing but the object's page moves inside it.
//!
//! It prints on the console it maps itself, as `device-test` does, and ends with `system_reset`.
//! The driver is a copy of it (`spawn.rs`).

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// What one device object costs its owner (kernel `device.rs`, `DEVICE_PAGES`).
const DEVICE_PAGES: u64 = 1;
/// Pages of the driver's run.
const RUN_PAGES: usize = 4;
/// How long a check waits for the driver's end, in microseconds of guest time.
const WAIT: u64 = 10_000_000;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[device-object-page] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// The driver: the slot in its slot 1. One run through it, then a fault, so that its end resets
/// the slot, which does not confirm.
extern "C" fn driver(_: usize) -> ! {
    if rd::dma_alloc(1, RUN_PAGES).is_err() {
        rd::process_exit(1);
    }
    // SAFETY: not sound, and that is the point: a store to address 0, which no process ever has
    // mapped, must trap, so that the kernel reports the driver `faulted` and resets its slot.
    unsafe { (0usize as *mut usize).write_volatile(1) };
    test_programs::park()
}

fn pages(budget: u32) -> u64 { rd::usage(budget).expect("budget_usage").pages_usage }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[device-object-page] mapped the console");

    // --- One page per device object, and nothing else on `system` ---------------------------
    let devices = rd::first_free() - rd::RESET;
    let answering = (rd::RESET..rd::first_free()).filter(|&h| rd::device_info(h).is_ok()).count() as u32;
    let at_boot = pages(rd::SYSTEM);
    check!(
        out,
        answering == devices && at_boot == u64::from(devices) * DEVICE_PAGES,
        "system is charged {} pages for the {} device objects it owns: one page each",
        at_boot,
        devices
    );

    // --- A quarantine destroys one object: its page goes back ---------------------------------
    // An empty virtio slot: DMA-flagged, with no device behind it to program. Mapping it joins
    // this program's reset set, which matters only if it dies, and it never does before the
    // power-off.
    let slot = (rd::OTHER_DEVICES..rd::first_free()).find(|&h| {
        let Ok((at, len)) = rd::map_device(h) else { return false };
        // SAFETY: `at` maps `len` bytes of this device's registers; its first word is the magic.
        let magic = unsafe { (at as *const u32).read_volatile() };
        len >= rd::PAGE_SIZE && magic == u32::from_le_bytes(*b"virt")
    });
    let Some(slot) = slot else {
        say!(out, "[device-object-page] FAIL: no empty virtio slot");
        test_programs::park()
    };
    // The driver's budget comes from `system` (`root` keeps one process, this program's), before
    // the measure, so the carve is outside it: the driver's own pages, its run included, are the
    // carved budget's, charged to `system` once at the carve and not moved by anything the driver
    // does.
    let limit = spawn::image().pages() as u64 + 96;
    let budget = rd::create(rd::SYSTEM, &rd::spec(limit, 1, 10)).expect("the driver's budget");
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let before = pages(rd::SYSTEM);
    let spawned = spawn::spawn(&spawn::image(), budget, exit, driver as *const () as usize, &[], &[slot]);
    let faulted =
        matches!(rd::receive(Some(exit), WAIT, 0), Ok(Received::Exit(n)) if n.cause == Cause::Faulted);
    let after = pages(rd::SYSTEM);
    let gone = rd::device_info(slot);
    check!(
        out,
        spawned.is_ok() && faulted && after == before - DEVICE_PAGES && gone == Err(Error::Dead),
        "the quarantine destroyed the slot's object: system {} -> {} pages, one page back, and handle {} is gone ({:?})",
        before,
        after,
        slot,
        gone
    );

    say!(out, "[device-object-page] DEVICE OBJECT PAGE PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[device-object-page] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
