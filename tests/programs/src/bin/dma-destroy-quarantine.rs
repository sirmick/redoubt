//! A device quarantined while a budget is being destroyed still loses its handles (R10, I1, I2,
//! I16; kernel/devices.md, "Quarantine").
//!
//! The first program's driver dma_allocs through the deaf slot and parks. It is ended by
//! `budget_destroy`, not by a fault: a destruction is running (object-frame frees deferred and
//! the per-object handle sweeps folded into one pass), and the driver's death fails the slot's
//! first reset (`dma-reset-deaf`), quarantining it. The device object is charged to `system`,
//! not to the dying driver budget, so the destruction's one sweep, which keys on the owner,
//! would leave a handle naming the frame it is about to free. The destruction must return, every
//! handle to the quarantined slot must be gone (`BadHandle`), and the run's charge must move to
//! the parent.
//!
//! It runs as the bundle's first program, so it holds every device object, prints on the console
//! it maps itself, and ends with `system_reset`. The driver is a copy of it (`spawn.rs`).

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// Pages of the driver's run.
const RUN_PAGES: usize = 4;
/// How long a check waits for the driver's report, in microseconds of guest time.
const WAIT: u64 = 60_000_000;
/// Empty virtio-mmio slots this program can use: QEMU `virt` has 8.
const MAX_SLOTS: usize = 8;
/// The badge the driver reports on.
const D1: u64 = 1;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[dma-destroy-quarantine] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// The driver: the deaf slot in its slot 1, a send handle in slot 2. It allocates one run,
/// reports its physical address, then parks to be destroyed.
extern "C" fn driver(_arg: usize) -> ! {
    let phys = rd::dma_alloc(1, RUN_PAGES).map_or(0, |(_, p)| p as usize);
    if rd::send(2, &rd::body([phys, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
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
    say!(out, "\n[dma-destroy-quarantine] mapped the console");

    // One empty virtio slot, the "deaf" one: its first reset reports not confirmed.
    let mut slots = [0u32; MAX_SLOTS];
    let mut count = 0;
    for h in rd::OTHER_DEVICES..rd::log_rx() {
        let Ok((at, len)) = rd::map_device(h) else { continue };
        // SAFETY: `at` maps `len` bytes of this device's registers; its first word is the magic.
        let magic = unsafe { (at as *const u32).read_volatile() };
        if len >= rd::PAGE_SIZE && magic == u32::from_le_bytes(*b"virt") && count < MAX_SLOTS {
            slots[count] = h;
            count += 1;
        }
    }
    if count == 0 {
        say!(out, "[dma-destroy-quarantine] FAIL: no empty virtio slot");
        test_programs::park()
    }
    let deaf = slots[0];

    // One driver budget, a driver dma_allocing through the deaf slot, then destroyed.
    let ep = rd::endpoint_create().expect("an endpoint");
    let limit = spawn::image().pages() as u64 + 96;
    let before = pages(rd::SYSTEM);
    let budget = rd::create(rd::SYSTEM, &rd::spec(limit, 1, 10)).expect("a driver budget");
    let send = rd::mint_from_handle(ep, D1, None).expect("a send handle");
    let entry = driver as *const () as usize;
    let started = spawn::spawn(&spawn::image(), budget, ep, entry, &[], &[deaf, send]);
    let run = started.ok().and_then(|_| report(ep, D1)).map_or(0, |w| w[0] as u64);
    check!(out, run != 0, "the driver dma_alloced through the deaf slot ({:#x})", run);

    // The destruction: `budget_destroy` kills the parked driver, its death fails the slot's
    // first reset, and the device object is destroyed while object-frame frees are deferred.
    let destroyed = rd::destroy(budget);
    check!(out, destroyed.is_ok(), "the driver's budget is destroyed, not faulted");

    let mapped = rd::map_device(deaf).map(|_| ());
    let allocated = rd::dma_alloc(deaf, 1).map(|_| ());
    check!(
        out,
        mapped == Err(Error::BadHandle) && allocated == Err(Error::BadHandle),
        "every handle to the quarantined slot is gone (map_device {:?}, dma_alloc {:?})",
        mapped,
        allocated
    );

    let after = pages(rd::SYSTEM);
    check!(
        out,
        after == before + RUN_PAGES as u64,
        "its carve came back and its run's {} pages are charged to system ({} -> {})",
        RUN_PAGES,
        before,
        after
    );

    say!(out, "[dma-destroy-quarantine] DMA DESTROY QUARANTINE PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[dma-destroy-quarantine] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
