//! Attack test: a refused `process_map` whose source is an untouched reservation charges nothing
//! (kernel/memory.md, "Failure and restart").
//!
//! The source is pages of this process's stack reservation it has never touched
//! (`rd::untouched_stack_page`): the kernel backs them only for a call that goes ahead. Three
//! calls are refused, each from a source of its own, so no refusal sees pages another backed:
//! two after the source check, because the child's destination is taken and because the child's
//! budget cannot pay, and one at it, because the caller's own budget cannot pay for backing the
//! source. Only the process the loader starts holds a reservation, so that caller is this program,
//! in `init`'s place, its budget (`root`) filled first. After each refusal, the caller's usage (R6) and the
//! child's are what they were. Last, a source of the same kind is moved for real: backed, then
//! charged to the child, and nothing is left charged to the caller.
//!
//! This program is the loader's trusted first process, holding UART and Reset directly (as
//! `map-fixed-attack.rs` does), so every verdict is a kernel result, a budget's usage, not this
//! program's own claim.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, PAGE_SIZE, ResetKind};
use uart_16550::MmioSerialPort;

static CONSOLE: AtomicUsize = AtomicUsize::new(0);

/// Pages in each call.
const PAGES: usize = 2;
/// Where the calls map in the child: free in a fresh process on both widths.
const DST: usize = 0x5000_0000;
/// Where `root` is filled: a free area of this process on each width (as `map-fixed-attack`'s
/// filler).
#[cfg(target_pointer_width = "64")]
const FILL: usize = 0x8000_0000;
#[cfg(target_pointer_width = "32")]
const FILL: usize = 0x3000_0000;

struct Checker(MmioSerialPort);
impl Checker {
    fn check(&mut self, ok: bool, label: &str) {
        writeln!(self.0, "[process-map] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
        assert!(ok, "{}", label);
    }
}

fn pages_used(budget: u32) -> u64 { rd::usage(budget).unwrap().pages_usage }

/// Map pages at `FILL` until `root` has at most one free page; the length mapped. Each call
/// leaves room for the page tables it needs (one per 512 or 1024 pages, and a few above them), so
/// none is refused.
fn fill_root() -> usize {
    let mut len = 0;
    loop {
        let free = rd::free(rd::ROOT) as usize;
        if free <= 1 {
            return len;
        }
        let pages = if free > 64 { free - free / 256 - 8 } else { 1 };
        rd::map_fixed(FILL + len, pages * PAGE_SIZE, rd::rw()).expect("fill root");
        len += pages * PAGE_SIZE;
    }
}

/// A child in a budget with nothing to spare: the smallest page limit `process_create` accepts.
fn tight_child(exit: u32) -> (u32, u32) {
    for limit in 1..64 {
        let budget = rd::create(rd::USERS, &rd::spec(limit, 1, 10)).expect("a child budget");
        match rd::process_create(budget, exit) {
            Ok(child) => return (budget, child),
            Err(Error::OutOfMemory) => rd::destroy(budget).expect("destroy the budget"),
            Err(e) => panic!("process_create: {:?}", e),
        }
    }
    panic!("no budget of under 64 pages holds a process")
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut c = Checker(unsafe { MmioSerialPort::new(uart) });
    c.0.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(c.0).ok();

    // Three sources, all inside the 32 reserved pages and far below any page this program uses.
    let untouched = rd::untouched_stack_page();
    let first = untouched - (PAGES - 1) * PAGE_SIZE;
    let second = first - PAGES * PAGE_SIZE;
    let third = second - PAGES * PAGE_SIZE;
    let len = PAGES * PAGE_SIZE;
    let exit = rd::endpoint_create().expect("an exit endpoint");

    // The destination is taken: one page at `DST` is already the child's.
    let budget = rd::create(rd::USERS, &rd::spec(16, 1, 10)).expect("a child budget");
    let child = rd::process_create(budget, exit).expect("a child");
    let page = rd::map_anon(PAGE_SIZE, rd::rw()).expect("map_anon");
    rd::poke(page, 1);
    rd::process_map(child, page, DST, PAGE_SIZE, rd::rw()).expect("take the child's page at DST");
    let before = (pages_used(rd::ROOT), pages_used(budget));
    let r = rd::process_map(child, first, DST, len, rd::rw());
    c.check(r == Err(Error::InvalidArgument), "a taken destination is refused");
    c.check((pages_used(rd::ROOT), pages_used(budget)) == before, "nothing charged");

    // The child's budget cannot pay: it has fewer free pages than the call moves.
    let (tight, poor) = tight_child(exit);
    assert!(rd::free(tight) < PAGES as u64, "the tight budget has room to spare");
    let before = (pages_used(rd::ROOT), pages_used(tight));
    let r = rd::process_map(poor, second, DST, len, rd::rw());
    c.check(r == Err(Error::OutOfMemory), "a child budget that cannot pay is refused");
    c.check((pages_used(rd::ROOT), pages_used(tight)) == before, "nothing charged");

    // The caller's own budget cannot pay for backing the source: refused at the source check.
    let filled = fill_root();
    let before = (pages_used(rd::ROOT), pages_used(budget));
    let r = rd::process_map(child, third, DST + 0x10_0000, len, rd::rw());
    c.check(r == Err(Error::InvalidArgument), "a caller that cannot back its source is refused");
    c.check((pages_used(rd::ROOT), pages_used(budget)) == before, "nothing charged");
    rd::unmap(FILL, filled).expect("unmap the filler");

    // The call that goes ahead: the first source, still untouched, moves next to the taken page,
    // in the leaf table that already maps it, so the child pays for the pages and nothing else.
    let before = (pages_used(rd::ROOT), pages_used(budget));
    let r = rd::process_map(child, first, DST + PAGE_SIZE, len, rd::rw());
    c.check(r.is_ok(), "an untouched source moves");
    c.check(
        (pages_used(rd::ROOT), pages_used(budget)) == (before.0, before.1 + PAGES as u64),
        "the child pays for the pages, the caller for nothing",
    );

    writeln!(c.0, "[process-map] PROCESS-MAP UNTOUCHED TEST PASSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    loop {
        rd::receive(None, rd::FOREVER, 0).ok();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = CONSOLE.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: only this process initializes CONSOLE, to its granted UART page.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[process-map] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
