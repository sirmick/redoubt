//! DMA pages stay put (kernel/devices.md, `dma_alloc`): a `dma_alloc` page can be
//! neither lent, transferred nor given to another process with `process_map`; `set_flags` works on
//! it; `unmap` drops only the mapping, so its frames stay held and charged until the process ends;
//! and a child that allocates and exits gives its budget back every page.
//!
//! It runs as the bundle's first program, so it holds every device object, and it uses an empty
//! virtio-mmio slot: DMA-flagged, with no device behind it to program. It prints on the console it
//! maps itself, as `device-test` does, and ends with `system_reset`.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Cause, Error, MemFlags, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// Pages of each DMA buffer.
const DMA_PAGES: usize = 4;
/// How long a check waits for anything, in microseconds.
const WAIT: u64 = 2_000_000;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[dma-rules] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// The child: a DMA buffer through the device in its slot 1, then exit. Code 0 only if it got one.
extern "C" fn allocate_and_exit(_: usize) -> ! {
    let got = rd::dma_alloc(1, DMA_PAGES).is_ok();
    rd::process_exit(if got { 0 } else { 1 })
}

fn overlaps(a: u64, b: u64) -> bool {
    a < b + (DMA_PAGES * rd::PAGE_SIZE) as u64 && b < a + (DMA_PAGES * rd::PAGE_SIZE) as u64
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[dma-rules] mapped the console");

    let Some((dev, at, phys)) = (rd::OTHER_DEVICES..rd::log_rx())
        .find_map(|h| rd::dma_alloc(h, DMA_PAGES).ok().map(|(a, p)| (h, a, p)))
    else {
        say!(out, "[dma-rules] FAIL: no device carries the DMA flag");
        test_programs::park()
    };
    say!(out, "[dma-rules] a DMA buffer of {} pages through device handle {}", DMA_PAGES, dev);

    // --- Lend and transfer -----------------------------------------------------------------
    let ep = rd::endpoint_create().expect("an endpoint");
    let send = rd::mint_from_handle(ep, 1, None).expect("a send handle");
    let body = rd::body([0; rd::WORDS]);
    // A call's own failure comes back in its outcome's status.
    let call = |page: usize| {
        rd::call_outcome(send, &body, rd::pages(page, 1), 1000).and_then(|(o, _)| o.status.map(|_| ()))
    };
    let lent = call(at);
    let sent = rd::send(send, &body, rd::pages(at, 1), 1000);
    // The same call lending an ordinary page is not refused: it waits for a receiver in vain.
    let anon = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("an ordinary page");
    let control = call(anon);
    check!(
        out,
        lent == Err(Error::InvalidArgument)
            && sent == Err(Error::InvalidArgument)
            && control != Err(Error::InvalidArgument),
        "a DMA page cannot be lent or transferred (lend {:?}, transfer {:?}; an ordinary page {:?})",
        lent,
        sent,
        control
    );

    // --- process_map -----------------------------------------------------------------------
    let budget =
        rd::create(rd::SYSTEM, &rd::spec(spawn::image().pages() as u64 + 96, 1, 10)).expect("a budget");
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let process = rd::process_create(budget, exit).expect("a process");
    let moved = rd::process_map(process, at, 0x2000_0000, rd::PAGE_SIZE, rd::rw());
    let other = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("an ordinary page");
    let control = rd::process_map(process, other, 0x2000_0000, rd::PAGE_SIZE, rd::rw());
    check!(
        out,
        moved == Err(Error::InvalidArgument) && control == Ok(()),
        "process_map refuses a DMA page ({:?}) and takes an ordinary one",
        moved
    );
    rd::close(process).expect("close the unstarted process");

    // --- set_flags, unmap ------------------------------------------------------------------
    rd::poke(at, 0xd0a);
    let flips =
        [rd::set_flags(at, rd::PAGE_SIZE, MemFlags::READ), rd::set_flags(at, rd::PAGE_SIZE, rd::rw())];
    check!(
        out,
        flips.iter().all(|r| r.is_ok()) && rd::peek(at) == 0xd0a,
        "set_flags on a DMA page: read-only, then read-write again"
    );
    let before = rd::usage(rd::SYSTEM).expect("system usage").pages_usage;
    let unmapped = rd::unmap(at, DMA_PAGES * rd::PAGE_SIZE);
    let after = rd::usage(rd::SYSTEM).expect("system usage").pages_usage;
    let (_, next) = rd::dma_alloc(dev, DMA_PAGES).expect("another DMA buffer");
    check!(
        out,
        unmapped == Ok(()) && after == before && !overlaps(next, phys),
        "unmap drops only the mapping: still charged ({} -> {} pages) and never handed out again",
        before,
        after
    );

    // --- A child's DMA pages go back to its budget when it exits ------------------------------
    let child_budget =
        rd::create(rd::SYSTEM, &rd::spec(spawn::image().pages() as u64 + 96, 1, 10)).expect("C");
    let empty = rd::usage(child_budget).expect("C's usage").pages_usage;
    let entry = allocate_and_exit as *const () as usize;
    let spawned = spawn::spawn(&spawn::image(), child_budget, exit, entry, &[], &[dev]);
    let notice = rd::receive(Some(exit), WAIT, 0);
    let exited = matches!(notice, Ok(Received::Exit(n)) if n.cause == Cause::Exited && n.code == 0);
    let left = rd::usage(child_budget).expect("C's usage").pages_usage;
    check!(
        out,
        spawned.is_ok() && exited && left == empty,
        "a child that allocated {} DMA pages and exited leaves its budget as it was ({} -> {} pages)",
        DMA_PAGES,
        empty,
        left
    );

    say!(out, "[dma-rules] DMA RULES PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[dma-rules] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
