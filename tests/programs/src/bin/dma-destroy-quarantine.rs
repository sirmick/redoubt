//! Devices quarantined while a budget is being destroyed still lose their handles (R10, I1, I2,
//! I16; kernel/devices.md, "Quarantine").
//!
//! The first program's two drivers each dma_alloc through a distinct deaf slot and park. They are
//! ended by one `budget_destroy`, not a fault: a destruction is running (object-frame frees
//! deferred and the per-object handle sweeps folded into one pass), and each driver's death fails
//! its slot's first reset (`dma-reset-deaf`), quarantining it. Each device object is charged to
//! `system`, not to the dying driver budget, so the destruction's one sweep, which keys on the
//! owner, would leave a handle naming the frame it is about to free; and the second death's
//! quarantine must not find and free the first device's already-deferred frame again. The
//! destruction must return, every handle to either quarantined slot must be gone (`BadHandle`),
//! and both runs' charges must move to the parent.
//!
//! It runs in `init`'s place, so it holds every device object, prints on the console it maps
//! itself, and ends with `system_reset`. The drivers are copies of it (`spawn.rs`).

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// Pages of each driver's run.
const RUN_PAGES: usize = 4;
/// How long a check waits for the drivers' reports, in microseconds of guest time.
const WAIT: u64 = 60_000_000;
/// Empty virtio-mmio slots this program can use: QEMU `virt` has 8.
const MAX_SLOTS: usize = 8;
/// The badges the two drivers report on.
const D1: u64 = 1;
const D2: u64 = 2;

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

/// A driver: the deaf slot in its slot 1, a send handle in slot 2. It allocates one run, reports
/// its physical address, then parks to be destroyed.
extern "C" fn driver(_arg: usize) -> ! {
    let phys = rd::dma_alloc(1, RUN_PAGES).map_or(0, |(_, p)| p as usize);
    if rd::send(2, &rd::body([phys, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    test_programs::park()
}

/// Both drivers' reports, told apart by badge; the drivers race, so neither may be assumed first.
fn two_reports(ep: u32) -> Option<([usize; rd::WORDS], [usize; rd::WORDS])> {
    let (mut d1, mut d2) = (None, None);
    while d1.is_none() || d2.is_none() {
        match rd::receive(Some(ep), WAIT, 0).ok()? {
            Received::Message(m) if m.badge == D1 => d1 = d1.or(Some(m.body.words)),
            Received::Message(m) if m.badge == D2 => d2 = d2.or(Some(m.body.words)),
            Received::Message(_) | Received::Exit(_) => continue,
            _ => return None,
        }
    }
    Some((d1.unwrap(), d2.unwrap()))
}

fn pages(budget: u32) -> u64 { rd::usage(budget).expect("budget_usage").pages_usage }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[dma-destroy-quarantine] mapped the console");

    // Two empty virtio slots, the "deaf" ones: each first reset reports not confirmed.
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
        say!(out, "[dma-destroy-quarantine] FAIL: {} empty virtio slots, not 2", count);
        test_programs::park()
    }
    let (deaf0, deaf1) = (slots[0], slots[1]);

    // One budget holding both drivers, each dma_allocing through its own deaf slot, then
    // destroyed: the second driver's death quarantines a second slot in the same destruction.
    let ep = rd::endpoint_create().expect("an endpoint");
    let limit = 2 * spawn::image().pages() as u64 + 200;
    let before = pages(rd::SYSTEM);
    let budget = rd::create(rd::SYSTEM, &rd::spec(limit, 2, 10)).expect("a driver budget");
    let entry = driver as *const () as usize;
    let image = spawn::image();
    let send1 = rd::mint_from_handle(ep, D1, None).expect("a send handle");
    let send2 = rd::mint_from_handle(ep, D2, None).expect("a send handle");
    let started1 = spawn::spawn(&image, budget, ep, entry, &[], &[deaf0, send1]);
    let started2 = spawn::spawn(&image, budget, ep, entry, &[], &[deaf1, send2]);
    let reports = (started1.is_ok() && started2.is_ok()).then(|| two_reports(ep)).flatten();
    let (r1, r2) = reports.unwrap_or(([0; rd::WORDS], [0; rd::WORDS]));
    let (run1, run2) = (r1[0] as u64, r2[0] as u64);
    check!(
        out,
        run1 != 0 && run2 != 0,
        "two drivers dma_alloced through two deaf slots ({:#x}, {:#x})",
        run1,
        run2
    );

    // The destruction: `budget_destroy` kills both parked drivers, each death fails its slot's
    // first reset, and each device object is destroyed while object-frame frees are deferred.
    let destroyed = rd::destroy(budget);
    check!(out, destroyed.is_ok(), "the drivers' budget is destroyed, not faulted");

    let mapped0 = rd::map_device(deaf0).map(|_| ());
    let allocated0 = rd::dma_alloc(deaf0, 1).map(|_| ());
    let mapped1 = rd::map_device(deaf1).map(|_| ());
    let allocated1 = rd::dma_alloc(deaf1, 1).map(|_| ());
    check!(
        out,
        mapped0 == Err(Error::BadHandle)
            && allocated0 == Err(Error::BadHandle)
            && mapped1 == Err(Error::BadHandle)
            && allocated1 == Err(Error::BadHandle),
        "every handle to either quarantined slot is gone (map_device {:?}/{:?}, dma_alloc {:?}/{:?})",
        mapped0,
        mapped1,
        allocated0,
        allocated1
    );

    // `system` gets the carve back and both runs' pages, and loses the two quarantined device
    // objects' pages (one each, kernel/devices.md). The drivers' process objects are charged to
    // their creator's budget, `root`, where `init`'s place runs, so they are not counted here.
    let after = pages(rd::SYSTEM);
    check!(
        out,
        after == before + (2 * RUN_PAGES) as u64 - 2,
        "the carve came back, both runs' {} pages are charged to system and both device objects freed ({} -> {})",
        2 * RUN_PAGES,
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
