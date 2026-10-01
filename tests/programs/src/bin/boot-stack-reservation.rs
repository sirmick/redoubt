//! A boot process's stack is the 32 pages below `0x8000_0000` the loader reserves, and nothing
//! more (kernel/memory-layout.md, "Regions").
//!
//! From the process itself: `map_fixed` of the lowest of the 32 is refused as an overlap and
//! charges nothing, and `map_fixed` of the page just below them succeeds. The kernel once
//! reserved the stack a second time, 128 KiB down from `sp`, which took that page too. This
//! program is the loader's trusted first process, holding UART and Reset directly (as
//! `map-fixed-attack.rs` does), so every verdict is a kernel result, not this program's own claim.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, PAGE_SIZE, ResetKind};
use uart_16550::MmioSerialPort;

static CONSOLE: AtomicUsize = AtomicUsize::new(0);

/// The top of a boot process's stack, the same on both widths.
const STACK_TOP: usize = 0x8000_0000;
/// The pages the loader reserves for it.
const STACK_PAGES: usize = 32;

struct Checker(MmioSerialPort);
impl Checker {
    fn check(&mut self, ok: bool, label: &str) {
        writeln!(self.0, "[boot-stack] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
        assert!(ok, "{}", label);
    }
}

fn pages_used() -> u64 { rd::usage(rd::SYSTEM).unwrap().pages_usage }

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut c = Checker(unsafe { MmioSerialPort::new(uart) });
    c.0.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(c.0).ok();

    let lowest = STACK_TOP - STACK_PAGES * PAGE_SIZE;
    let before = pages_used();
    let r = rd::map_fixed(lowest, PAGE_SIZE, rd::rw());
    c.check(r == Err(Error::InvalidArgument), "the lowest stack page is reserved");
    c.check(pages_used() == before, "nothing charged");

    let below = lowest - PAGE_SIZE;
    let r = rd::map_fixed(below, PAGE_SIZE, rd::rw());
    c.check(r.is_ok(), "the page below the stack is free");
    c.check(pages_used() > before, "it is charged as a mapping");
    rd::unmap(below, PAGE_SIZE).expect("unmap the page below the stack");
    c.check(pages_used() == before, "unmap returns usage to baseline");

    writeln!(c.0, "[boot-stack] BOOT STACK TEST PASSED").ok();
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
        writeln!(out, "[boot-stack] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
