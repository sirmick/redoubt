//! DMA frames come back only after a reset (WP-K5b, answer 173; KERNEL-SPEC.md, Device): a
//! driver child brings the case's virtio disk up, allocates DMA runs through it and is destroyed
//! with its budget. This program then allocates until it gets back at least one of the driver's
//! physical frames, and for each one it got back, reads the disk's status register through its
//! own `map_device` handle: it must read 0, the reset, by the time the frame was handed out. No
//! reuse at all is an inconclusive FAIL, never a pass.
//!
//! It runs as the bundle's first program, so it holds every device object, prints on the console
//! it maps itself, as `device-test` does, and ends with `system_reset`. The child is a copy of it
//! (`spawn.rs`); it programs no queue, and nothing reaches the disk.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// The driver's runs, and the pages of each.
const RUNS: usize = 4;
const RUN_PAGES: usize = 4;
/// How long a check waits for anything, in microseconds.
const WAIT: u64 = 2_000_000;

// virtio-mmio registers (virtio 1.2, 4.2.2) and device status bits (2.1).
const MAGIC: usize = 0x000;
const DEVICE_ID: usize = 0x008;
const STATUS: usize = 0x070;
const BLOCK: u32 = 2;
const ACKNOWLEDGE: u32 = 1;
const DRIVER: u32 = 2;
const DRIVER_OK: u32 = 4;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[dma-reset-reuse] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// A 32-bit register of the device mapped at `at`.
fn register(at: usize, offset: usize) -> *mut u32 { (at + offset) as *mut u32 }

fn read(at: usize, offset: usize) -> u32 {
    // SAFETY: `at` is a device's first register page, mapped for this process by `map_device`,
    // and every offset used here is a 4-aligned register inside it.
    unsafe { register(at, offset).read_volatile() }
}

fn write(at: usize, offset: usize, value: u32) {
    // SAFETY: as `read`.
    unsafe { register(at, offset).write_volatile(value) }
}

/// The driver: the disk in its slot 1, a send handle to the caller in slot 2. It brings the
/// disk up to DRIVER_OK, allocates `RUNS` DMA runs through it and reports each as
/// `[status, phys, 0, 0]`, then waits to be destroyed.
extern "C" fn driver(_: usize) -> ! {
    let Ok((at, _)) = rd::map_device(1) else { rd::process_exit(1) };
    for status in [0, ACKNOWLEDGE, ACKNOWLEDGE | DRIVER, ACKNOWLEDGE | DRIVER | DRIVER_OK] {
        write(at, STATUS, status);
    }
    for _ in 0..RUNS {
        let Ok((_, phys)) = rd::dma_alloc(1, RUN_PAGES) else { rd::process_exit(2) };
        let report = rd::body([read(at, STATUS) as usize, phys as usize, 0, 0]);
        if rd::send(2, &report, None, WAIT).is_err() {
            rd::process_exit(3);
        }
    }
    test_programs::park()
}

fn overlaps(a: u64, a_pages: usize, b: u64, b_pages: usize) -> bool {
    a < b + (b_pages * rd::PAGE_SIZE) as u64 && b < a + (a_pages * rd::PAGE_SIZE) as u64
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[dma-reset-reuse] mapped the console");

    let free = rd::first_free();
    let disk = (rd::OTHER_DEVICES..free).find_map(|h| {
        let (at, len) = rd::map_device(h).ok()?;
        let virtio = len >= rd::PAGE_SIZE && read(at, MAGIC) == u32::from_le_bytes(*b"virt");
        (virtio && read(at, DEVICE_ID) == BLOCK).then_some((h, at))
    });
    let Some((disk, regs)) = disk else {
        say!(out, "[dma-reset-reuse] FAIL: no virtio disk");
        test_programs::park()
    };

    // --- The driver brings the disk up and holds DMA runs through it ---------------------------
    let budget =
        rd::create(rd::SYSTEM, &rd::spec(spawn::image().pages() as u64 + 96, 1, 10)).expect("a budget");
    let ep = rd::endpoint_create().expect("an endpoint");
    let send = rd::mint_from_handle(ep, 1, None).expect("a send handle");
    let entry = driver as *const () as usize;
    let spawned = spawn::spawn(&spawn::image(), budget, ep, entry, &[], &[disk, send]);
    let mut runs = [0u64; RUNS];
    let mut status = 0;
    let mut got = 0;
    while spawned.is_ok() && got < RUNS {
        let Ok(Received::Message(m)) = rd::receive(Some(ep), WAIT, 0) else { break };
        status = m.body.words[0] as u32;
        runs[got] = m.body.words[1] as u64;
        got += 1;
    }
    let seen = read(regs, STATUS);
    check!(
        out,
        got == RUNS && status & DRIVER_OK != 0 && seen == status,
        "the driver brought the disk up (status {:#x}, {:#x} here) and holds {} DMA runs of {} pages",
        status,
        seen,
        got,
        RUN_PAGES
    );

    // --- Destroyed: its frames come back only after the disk's reset ---------------------------
    let destroyed = rd::destroy(budget);
    let mut reused = 0;
    let mut dirty = 0;
    let mut devices = rd::OTHER_DEVICES..free;
    let mut device = devices.next();
    while let Some(h) = device {
        let Ok((_, phys)) = rd::dma_alloc(h, RUN_PAGES) else {
            device = devices.next();
            continue;
        };
        // Read at once: nothing but a reset clears the status between the hand-out and here.
        let now = read(regs, STATUS);
        for &run in runs.iter().filter(|&&r| overlaps(phys, RUN_PAGES, r, RUN_PAGES)) {
            reused += 1;
            if now != 0 {
                dirty += 1;
                say!(
                    out,
                    "[dma-reset-reuse] frame {:#x} of run {:#x} handed out at status {:#x}",
                    phys,
                    run,
                    now
                );
            }
        }
        if reused >= RUNS {
            break;
        }
    }
    check!(
        out,
        destroyed.is_ok() && reused > 0,
        "the driver frames came back to this program ({} of its runs reused)",
        reused
    );
    check!(
        out,
        reused > 0 && dirty == 0,
        "every reused frame was handed out after the disk reset ({} at a non-zero status)",
        dirty
    );

    say!(out, "[dma-reset-reuse] DMA RESET REUSE PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[dma-reset-reuse] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
