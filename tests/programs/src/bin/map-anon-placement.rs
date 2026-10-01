//! Where the kernel places pages (kernel/memory.md, "Where `map_anon` puts pages"): a `map_anon`
//! request larger than its 256 MiB area is `OutOfMemory` with nothing charged, even when the
//! budget could pay for it, and a lend or a transfer a process receives lands inside the 4 MiB
//! message area. The search's worst case is `map-anon-search-bound`'s.
//!
//! It runs as the bundle's first program, alone, and judges the kernel itself (docs/testbench.md,
//! "Rule F (trusted verdicts)"): every `ok:` line is an error, a usage or an address the kernel
//! returned.

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, FOREVER, MessageKind, Received, ResetKind};
use uart_16550::MmioSerialPort;

static UART: AtomicUsize = AtomicUsize::new(0);

/// The `map_anon` area's size (memory-layout.md, "Regions").
const AREA_PAGES: usize = 0x1000_0000 / rd::PAGE_SIZE;
/// The message area (memory-layout.md, "Regions").
const MESSAGES: usize = 0x4000_0000;
const MESSAGES_END: usize = MESSAGES + (4 << 20);

static ENDPOINT: AtomicUsize = AtomicUsize::new(0);
/// Where the receiving thread was handed the lend and the transfer; 0 until it was.
static LENT_AT: AtomicUsize = AtomicUsize::new(0);
static TRANSFERRED_AT: AtomicUsize = AtomicUsize::new(0);

fn check(ok: bool, label: &str) {
    // SAFETY: `UART` is this process's granted console page, mapped before the first check.
    let mut out = unsafe { MmioSerialPort::new(UART.load(Ordering::Relaxed)) };
    writeln!(out, "[placement] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
    assert!(ok, "{}", label);
}

fn usage_pages() -> u64 { rd::usage(rd::SYSTEM).unwrap().pages_usage }

/// The receiving thread: records where the kernel mapped each page it was given.
fn receiver(_: usize) {
    let endpoint = ENDPOINT.load(Ordering::Acquire) as u32;
    loop {
        let Ok(Received::Message(m)) = rd::receive(Some(endpoint), FOREVER, 1) else { continue };
        match m.kind {
            MessageKind::Call { lend: Some(lend) } => {
                LENT_AT.store(lend.addr, Ordering::Release);
                rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
            }
            MessageKind::Send { transfer: Some(t) } => TRANSFERRED_AT.store(t.addr, Ordering::Release),
            _ => {}
        }
    }
}

fn in_messages(at: usize) -> bool { (MESSAGES..MESSAGES_END).contains(&at) }

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    writeln!(out).ok();
    UART.store(uart, Ordering::Relaxed);

    // An oversize request, which the budget alone would not refuse.
    check(rd::free(rd::SYSTEM) > AREA_PAGES as u64 + 1, "the budget could pay for more than the area");
    let before = usage_pages();
    let r = rd::map_anon((AREA_PAGES + 1) * rd::PAGE_SIZE, rd::rw());
    check(r == Err(Error::OutOfMemory), "a request larger than the area is refused");
    check(usage_pages() == before, "nothing charged");

    // The message area.
    ENDPOINT.store(rd::endpoint_create().expect("an endpoint") as usize, Ordering::Release);
    rd::thread(receiver, 0).expect("the receiving thread");
    let endpoint = ENDPOINT.load(Ordering::Acquire) as u32;
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("a page to lend");
    rd::call(endpoint, &rd::body([0; rd::WORDS]), rd::pages(page, 1), FOREVER).expect("the lend");
    check(in_messages(LENT_AT.load(Ordering::Acquire)), "a received lend lands in the message area");
    rd::send(endpoint, &rd::body([0; rd::WORDS]), rd::pages(page, 1), FOREVER).expect("the transfer");
    while TRANSFERRED_AT.load(Ordering::Acquire) == 0 {
        test_programs::wait_ms(1);
    }
    check(
        in_messages(TRANSFERRED_AT.load(Ordering::Acquire)),
        "a received transfer lands in the message area",
    );

    writeln!(out, "[placement] MAP-ANON PLACEMENT TEST PASSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    loop {
        rd::receive(None, FOREVER, 0).ok();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: only this process sets `UART`, to its granted console page.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[placement] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
