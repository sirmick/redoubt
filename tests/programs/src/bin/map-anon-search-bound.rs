//! `map_anon`'s placement search (kernel/memory.md; memory-layout.md, "Regions"): every run it
//! returns lies inside the `map_anon` area, a run that fits only at the area's end is found, a
//! run over `MAX_RANGE_PAGES` is placed whole with its first range alone mapped, and a refusal
//! costs a read of the area's runs, a constant whatever the area holds, so a timer wake meanwhile
//! stays prompt (R12 and R22, kernel/scheduling.md).
//!
//! It is the one program the tester starts, in its own budget (slot 3), which is all of
//! `system`'s free pages bar one: a page in every range of the area is more than `root` keeps in
//! `init`'s place. It judges the kernel itself, with nothing else beside it (docs/testbench.md,
//! "Rule F (trusted verdicts)"): every `ok:` line is an address or a time the kernel returned. The
//! placement checks run before the logger has its page, while the area holds only the pins (that
//! page is placed in the same area, in a gap a range does not fit), and are printed once it has.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, MAX_RANGE_PAGES};
use test_programs::{Logger, checker, log};

/// The `map_anon` area: 256 MiB from here (memory-layout.md, "Regions").
const AREA: usize = 0x6000_0000;
const AREA_PAGES: usize = 0x1000_0000 / rd::PAGE_SIZE;
const AREA_END: usize = AREA + AREA_PAGES * rd::PAGE_SIZE;
/// The sleeping thread's stack, outside the area.
const STACK: usize = 0x5000_0000;

/// The refused search's bound, for the densest shape this program's budget affords: a page in every
/// range-long window, 4,096 pages, where a search that walked the tables for each start would try
/// 4,096 of them. Reading the area's runs, a refusal reads every group record twice whatever the
/// area holds (kernel/memory.md, R22). Twice the worst measured in a checked build under `icount`
/// (152 µs on rv64, 187 on rv32; 1 ms is 125,000 guest instructions, so the time does not depend
/// on the host), rounded up.
const REFUSAL_BOUND_US: u64 = 400;
/// The pages the program's budget holds at the start: room for a page in every range-long window
/// of the area and its page tables, not for one in every half-range window, so the shape it
/// builds is the densest it affords.
const AFFORDS: core::ops::Range<u64> = 4_500..8_400;
/// The sleeping thread's timeout, and how late its wake may be: twice the worst lateness measured
/// at this shape (1.9 ms on rv64, 2.0 on rv32, the search's share of it a tenth), rounded up.
const SLEEP_US: u64 = 1_000;
const WAKE_BOUND_US: u64 = 5_000;

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

/// The last page of every range-long window of the area but the last: each window keeps one page
/// short of a range free, so the only run of a range that fits is the area's last window.
fn pins() -> impl Iterator<Item = usize> {
    (AREA..AREA_END - pages(MAX_RANGE_PAGES))
        .step_by(pages(MAX_RANGE_PAGES))
        .map(|w| w + pages(MAX_RANGE_PAGES - 1))
}

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
    free: u64,
    over: Result<usize, Error>,
    /// Whether the page after the long run's first range was free once it was placed.
    rest_free: bool,
    first: Result<usize, Error>,
    end_fit: Result<usize, Error>,
    wrapped: Result<usize, Error>,
}

fn placement() -> Placement {
    let free = rd::free(rd::OWN);
    // A run over a range, in the empty area: placed at its start, its first range alone mapped.
    let over = rd::map_anon(pages(MAX_RANGE_PAGES + 1), rd::rw());
    let rest = over.map(|at| at + pages(MAX_RANGE_PAGES));
    let rest_free = rest.is_ok_and(|at| rd::map_fixed(at, rd::PAGE_SIZE, rd::rw()).is_ok());
    if let (Ok(at), Ok(after)) = (over, rest) {
        rd::unmap(at, pages(MAX_RANGE_PAGES)).unwrap();
        if rest_free {
            rd::unmap(after, rd::PAGE_SIZE).unwrap();
        }
    }
    // A range in the empty area: at its start.
    let first = rd::map_anon(pages(MAX_RANGE_PAGES), rd::rw());
    if let Ok(at) = first {
        rd::unmap(at, pages(MAX_RANGE_PAGES)).unwrap();
    }
    // A range that fits only at the area's very end: every window before it pinned.
    pins().for_each(pin);
    let end_fit = rd::map_anon(pages(MAX_RANGE_PAGES), rd::rw());
    if let Ok(at) = end_fit {
        rd::unmap(at, pages(MAX_RANGE_PAGES)).unwrap();
    }
    // The last placement is now high (a range from the end), its start the last start that fits.
    // One page pinned there blocks every start inside the area, so the request must be refused,
    // not placed from above the last start and out past the area's end.
    let last_start = AREA_END - pages(MAX_RANGE_PAGES);
    pin(last_start);
    let wrapped = rd::map_anon(pages(MAX_RANGE_PAGES), rd::rw());
    if let Ok(at) = wrapped {
        rd::unmap(at, pages(MAX_RANGE_PAGES)).unwrap();
    }
    Placement { free, over, rest_free, first, end_fit, wrapped }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let p = placement();

    let mut out = Logger::connect();
    assert!(AFFORDS.contains(&p.free), "its budget held {} pages", p.free);
    log!(
        out,
        "[search] ok: its budget affords a page in every range-long window, not in every half: {} pages",
        p.free
    );
    assert_eq!((p.over, p.rest_free), (Ok(AREA), true), "a run over the range cap");
    log!(
        out,
        "[search] ok: a run over the range cap is placed at the start of the area, its first range alone mapped"
    );
    assert_eq!(p.first, Ok(AREA), "a range in an empty area");
    log!(out, "[search] ok: a range in the empty area is placed at its start");
    assert_eq!(p.end_fit, Ok(AREA_END - pages(MAX_RANGE_PAGES)), "a range that fits only at the end");
    log!(out, "[search] ok: a run that fits only at the area's end is placed there");
    assert_eq!(p.wrapped, Err(Error::OutOfMemory), "a wrap after a high placement");
    log!(out, "[search] ok: after a high placement, a run that does not fit is refused");

    // The worst case for the scan, as the pins leave the area: a page taken in every window, so
    // every page table of the area is there to be read and no range fits. A thread sleeps
    // meanwhile, already asleep when the search starts: its wake waits for the search.
    let stack = rd::map_fixed(STACK, pages(4), rd::rw()).map(|()| STACK).expect("the sleeper's stack");
    rd::thread_create(sleeper as *const () as usize, stack + pages(4) - 16, 0).expect("the sleeper");
    while DUE.load(Ordering::Acquire) == 0 {
        rd::receive(None, 100, 0).ok();
    }
    let elapsed = refusal_time(pages(MAX_RANGE_PAGES));
    log!(out, "[search] a refused search took {elapsed} us");
    assert!(elapsed < REFUSAL_BOUND_US, "a refused search took {elapsed} us");
    log!(out, "[search] ok: a refused search took {elapsed} us, under 400 us");
    while WOKE.load(Ordering::Acquire) == 0 {
        rd::receive(None, SLEEP_US, 0).ok();
    }
    let late = (WOKE.load(Ordering::Acquire) - DUE.load(Ordering::Acquire)) as u64;
    log!(out, "[search] a timer wake during the search was {late} us late");
    assert!(late < WAKE_BOUND_US, "the sleeper woke {late} us late");
    log!(out, "[search] ok: a timer wake during the search was {late} us late, under 5 ms");

    // Placements that succeed land inside the area.
    pins().chain([AREA_END - pages(MAX_RANGE_PAGES)]).for_each(|at| rd::unmap(at, rd::PAGE_SIZE).unwrap());
    for npages in [1, 7, MAX_RANGE_PAGES] {
        let at = rd::map_anon(pages(npages), rd::rw()).expect("a run that fits");
        assert!(inside(at, npages), "{npages} pages at {at:#x}");
        rd::unmap(at, pages(npages)).unwrap();
    }
    log!(out, "[search] ok: every run placed lies inside the area");
    log!(out, "[search] MAP_ANON SEARCH BOUNDED");
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[search] FAIL: {info}");
    test_programs::park()
}
