//! `map_anon`'s placement search (kernel/memory.md; memory-layout.md, "Regions"): every run it
//! returns lies inside the `map_anon` area, a run that fits only at the area's end is found, a
//! request for the whole of an empty area succeeds, and a refusal costs a scan of the area, not
//! the area times the request, so a timer wake meanwhile stays prompt (R12, kernel/scheduling.md).
//!
//! It runs as the bundle's first program, alone, and judges the kernel itself (docs/testbench.md,
//! "Rule F (trusted verdicts)"): every `ok:` line is an address or a time the kernel returned.
//! The placement checks run before the console is mapped, while the area is still empty (the
//! console's registers are placed in the same area), and are printed once it is.

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, ResetKind};
use uart_16550::MmioSerialPort;

static UART: AtomicUsize = AtomicUsize::new(0);

/// The `map_anon` area: 256 MiB from here (memory-layout.md, "Regions").
const AREA: usize = 0x6000_0000;
const AREA_PAGES: usize = 0x1000_0000 / rd::PAGE_SIZE;
const AREA_END: usize = AREA + AREA_PAGES * rd::PAGE_SIZE;
/// Half the area, in pages.
const HALF: usize = AREA_PAGES / 2;
/// A leaf page table's span on Sv39 (half of one on Sv32).
const SPAN: usize = 2 << 20;
/// The sleeping thread's stack, outside the area.
const STACK: usize = 0x5000_0000;

/// The refused search's bound. The old search tested about 5 x 10^8 pages past a middle page. A
/// scan of the area reads each of its 65536 leaf entries at most once: about 9.6 ms in a checked
/// build when every page table of the area is there, the worst case, under `icount` (1 ms is
/// 125,000 guest instructions, so the time does not depend on the host).
const REFUSAL_BOUND_US: u64 = 12_000;
/// The sleeping thread's timeout, and how late its wake may be: the timer-wake target of
/// kernel/scheduling.md, "Responsiveness" (p50 15 ms).
const SLEEP_US: u64 = 1_000;
const WAKE_BOUND_US: u64 = 15_000;

/// When the sleeper's timeout was due, and when it ran, in microseconds since boot (which fits
/// a `usize` on rv32 for over an hour); 0 until it has.
static DUE: AtomicUsize = AtomicUsize::new(0);
static WOKE: AtomicUsize = AtomicUsize::new(0);

fn pages(n: usize) -> usize { n * rd::PAGE_SIZE }

/// How long a `map_anon` of `len` bytes takes to be refused, which it must be.
fn refusal_time(len: usize) -> u64 {
    let t0 = rd::time_now().unwrap();
    let refused = rd::map_anon(len, rd::rw());
    let elapsed = rd::time_now().unwrap() - t0;
    assert_eq!(refused, Err(Error::OutOfMemory), "a request of {len:#x} bytes");
    elapsed
}

fn inside(at: usize, npages: usize) -> bool { at >= AREA && at + pages(npages) <= AREA_END }

/// Map one page at `at`, which must be free.
fn pin(at: usize) { rd::map_fixed(at, rd::PAGE_SIZE, rd::rw()).expect("a pinned page") }

extern "C" fn sleeper(_: usize) -> ! {
    let due = rd::time_now().unwrap() + SLEEP_US;
    DUE.store(due as usize, Ordering::Release);
    rd::receive(None, SLEEP_US, 0).ok();
    WOKE.store(rd::time_now().unwrap() as usize, Ordering::Release);
    rd::thread_exit().ok();
    test_programs::park()
}

/// What the placement checks found, printed once the console is mapped.
struct Placement {
    whole: Result<usize, Error>,
    end_fit: Result<usize, Error>,
    wrapped: Result<usize, Error>,
}

fn placement() -> Placement {
    // The whole of the empty area.
    let whole = rd::map_anon(pages(AREA_PAGES), rd::rw());
    if let Ok(at) = whole {
        rd::unmap(at, pages(AREA_PAGES)).unwrap();
    }
    // A run of half the area that fits only at its very end: one page pinned just below it
    // leaves half less a page below.
    let pinned = AREA_END - pages(HALF + 1);
    pin(pinned);
    let end_fit = rd::map_anon(pages(HALF), rd::rw());
    if let Ok(at) = end_fit {
        rd::unmap(at, pages(HALF)).unwrap();
    }
    rd::unmap(pinned, rd::PAGE_SIZE).unwrap();
    // The last placement is now high (half the area from the end). A run of half plus two pages
    // has its last start two pages below that; one page pinned at that last start blocks every
    // start inside the area, so the request must be refused, not placed from above the last
    // start and out past the area's end.
    let more = HALF + 2;
    let last_start = AREA_END - pages(more);
    pin(last_start);
    let wrapped = rd::map_anon(pages(more), rd::rw());
    if let Ok(at) = wrapped {
        rd::unmap(at, pages(more)).unwrap();
    }
    rd::unmap(last_start, rd::PAGE_SIZE).unwrap();
    Placement { whole, end_fit, wrapped }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let p = placement();

    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    writeln!(out).ok();
    assert_eq!(p.whole, Ok(AREA), "a whole-area request in an empty area");
    writeln!(out, "[search] ok: a request for the whole empty area is placed at its start").ok();
    assert_eq!(p.end_fit, Ok(AREA_END - pages(HALF)), "a run that fits only at the end");
    writeln!(out, "[search] ok: a run that fits only at the area's end is placed there").ok();
    assert_eq!(p.wrapped, Err(Error::OutOfMemory), "a wrap after a high placement");
    writeln!(out, "[search] ok: after a high placement, a run that does not fit is refused").ok();

    // One page taken in the middle (the console's registers may already be there, placed after
    // the last run above); a request for more than half then fits on neither side.
    let middle = AREA + pages(HALF);
    assert!(matches!(rd::map_fixed(middle, rd::PAGE_SIZE, rd::rw()), Ok(()) | Err(Error::InvalidArgument)));
    let elapsed = refusal_time(pages(HALF + 1));
    assert!(elapsed < REFUSAL_BOUND_US, "a refused search past a middle page took {elapsed} us");
    writeln!(out, "[search] ok: a refused search past a middle page took {elapsed} us, under 12 ms").ok();

    // The worst case for the scan: a page taken in every 2 MiB span, so every page table of the
    // area is there to be read and no run of a span's length fits. A thread sleeps meanwhile,
    // already asleep when the search starts: its wake waits for the search.
    for span in (AREA..AREA_END).step_by(SPAN) {
        assert!(matches!(rd::map_fixed(span, rd::PAGE_SIZE, rd::rw()), Ok(()) | Err(Error::InvalidArgument)));
    }
    let stack = rd::map_fixed(STACK, pages(4), rd::rw()).map(|()| STACK).expect("the sleeper's stack");
    rd::thread_create(sleeper as *const () as usize, stack + pages(4) - 16, 0).expect("the sleeper");
    while DUE.load(Ordering::Acquire) == 0 {
        rd::receive(None, 100, 0).ok();
    }
    let elapsed = refusal_time(SPAN);
    assert!(elapsed < REFUSAL_BOUND_US, "a refused search over every page table took {elapsed} us");
    writeln!(out, "[search] ok: a refused search over every page table took {elapsed} us, under 12 ms").ok();
    while WOKE.load(Ordering::Acquire) == 0 {
        rd::receive(None, SLEEP_US, 0).ok();
    }
    let late = (WOKE.load(Ordering::Acquire) - DUE.load(Ordering::Acquire)) as u64;
    assert!(late < WAKE_BOUND_US, "the sleeper woke {late} us late");
    writeln!(out, "[search] ok: a timer wake during the search was {late} us late, under 15 ms").ok();

    // Placements that succeed land inside the area.
    for npages in [1, 7, 64, SPAN / rd::PAGE_SIZE - 1] {
        let at = rd::map_anon(pages(npages), rd::rw()).expect("a run that fits");
        assert!(inside(at, npages), "{npages} pages at {at:#x}");
        rd::unmap(at, pages(npages)).unwrap();
    }
    writeln!(out, "[search] ok: every run placed lies inside the area").ok();
    writeln!(out, "[search] MAP_ANON SEARCH BOUNDED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: this program mapped the UART and has stopped normal execution.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[search] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
