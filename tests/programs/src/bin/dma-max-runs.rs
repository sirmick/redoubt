//! A device holds at most `MAX_RUNS` (32) runs (kernel/devices.md, `dma_alloc`). Through one
//! empty virtio-mmio slot, DMA-flagged with no device behind it, one-page runs are allocated until
//! the kernel refuses: a child gets exactly 32 and `OutOfMemory` for the 33rd, and exits; its
//! places come back with its end, so this program gets 32 again and `OutOfMemory` for the 33rd,
//! while its budget has room and a run through a second slot succeeds at once, so the refusal is
//! the device's, not the budget's nor the pool's; and a second child then gets no run at all
//! through the full device, so the limit is the device's and not the caller's. The verdicts are the
//! kernel's answers.
//!
//! It runs as the bundle's first program, so it holds every device object, prints on the console
//! it maps itself, as `device-test` does, and ends with `system_reset`. The children are copies of
//! it (`spawn.rs`).

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Cause, DeviceInfo, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// Runs one device holds (kernel `dma.rs`, `MAX_RUNS`).
const MAX_RUNS: u32 = 32;
/// How long a check waits for a child's end, in microseconds of guest time.
const WAIT: u64 = 10_000_000;
/// A child's exit codes: the runs it got before `OutOfMemory`; `OTHER` plus the runs it got before
/// any other refusal.
const OTHER: u32 = 100;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[dma-max-runs] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// One-page runs through `slot` until the kernel refuses one, never more than `MAX_RUNS + 1`
/// asked: how many it gave, and the refusal.
fn fill(slot: u32) -> (u32, Result<(), Error>) {
    let mut got = 0;
    while got <= MAX_RUNS {
        match rd::dma_alloc(slot, 1) {
            Ok(_) => got += 1,
            Err(e) => return (got, Err(e)),
        }
    }
    (got, Ok(()))
}

/// A child: the slot in its slot 1. Fill it, then exit with what happened.
extern "C" fn fill_and_exit(_: usize) -> ! {
    let code = match fill(1) {
        (got, Err(Error::OutOfMemory)) => got,
        (got, _) => OTHER + got,
    };
    rd::process_exit(code)
}

/// Start a child filling `slot` in a budget of its own and wait for its exit code.
fn child_fills(slot: u32, exit: u32) -> Option<u32> {
    // Its image, stack and tables, and a page table or two for each run's mapping.
    let limit = spawn::image().pages() as u64 + 96 + u64::from(2 * MAX_RUNS);
    let budget = rd::create(rd::SYSTEM, &rd::spec(limit, 1, 10)).ok()?;
    spawn::spawn(&spawn::image(), budget, exit, fill_and_exit as *const () as usize, &[], &[slot]).ok()?;
    match rd::receive(Some(exit), WAIT, 0) {
        Ok(Received::Exit(n)) if n.cause == Cause::Exited => Some(n.code),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[dma-max-runs] mapped the console");

    // Two devices that carry the DMA flag: QEMU virt's empty virtio slots.
    let mut dma = (rd::OTHER_DEVICES..rd::first_free())
        .filter(|&h| matches!(rd::device_info(h), Ok(DeviceInfo::Mmio { dma: true, .. })));
    let (Some(first), Some(second)) = (dma.next(), dma.next()) else {
        say!(out, "[dma-max-runs] FAIL: fewer than two devices carry the DMA flag");
        test_programs::park()
    };
    let exit = rd::endpoint_create().expect("an exit endpoint");

    // --- A child fills the device and exits -----------------------------------------------------
    let code = child_fills(first, exit);
    check!(
        out,
        code == Some(MAX_RUNS),
        "a child allocating one-page runs through device handle {} got {:?} and then OutOfMemory",
        first,
        code
    );

    // --- Its places came back: this program fills the device, and the refusal is the device's ---
    let (got, last) = fill(first);
    let two = rd::dma_alloc(first, 2).map(|_| ());
    let usage = rd::usage(rd::ROOT).expect("root's usage");
    let room = usage.pages_limit - usage.pages_usage;
    let other = rd::dma_alloc(second, 1).map(|_| ());
    check!(
        out,
        got == MAX_RUNS && last == Err(Error::OutOfMemory) && two == Err(Error::OutOfMemory),
        "after the child's end this program got {} runs through handle {}, and the 33rd is {:?} (2 pages: {:?})",
        got,
        first,
        last,
        two
    );
    check!(
        out,
        room >= 64 && other == Ok(()),
        "the refusal is the device's: the budget has {} pages of room, and a run through handle {} is {:?}",
        room,
        second,
        other
    );

    // --- The limit is the device's, not the caller's -----------------------------------------
    let code = child_fills(first, exit);
    check!(
        out,
        code == Some(0),
        "a second child gets no run through the full device: {:?} runs before OutOfMemory",
        code
    );

    say!(out, "[dma-max-runs] DMA MAX RUNS PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[dma-max-runs] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
