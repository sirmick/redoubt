//! Attack test: ask for a user page that is writable but not readable (R11).
//!
//! The privileged architecture reserves that page-table encoding, so `map_anon`, `set_flags` and
//! `process_map` must each refuse it with `InvalidArgument` and leave the mapping as it was. This
//! program is the loader's trusted parent: the verdicts are kernel results, including the
//! kernel's own record check, which accepts an output record only in readable and writable RAM.
//! See `tests/write-only-attack.toml`.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, MemFlags, ResetKind};
use uart_16550::MmioSerialPort;

const PAGE: usize = rd::PAGE_SIZE;
const DEST: usize = 0x0800_0000;
static CONSOLE: AtomicUsize = AtomicUsize::new(0);

struct Checker(MmioSerialPort);
impl Checker {
    fn check(&mut self, ok: bool, label: &str) {
        writeln!(self.0, "[write-only] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
        assert!(ok, "{}", label);
    }
}

/// Whether the kernel takes `page` as an output record: it must be readable and writable RAM
/// of this process. Overwrites the page's first words with the system budget's usage.
fn writable_record(page: usize) -> bool { rd::usage_raw(rd::SYSTEM, page).is_ok() }

fn set_flags(c: &mut Checker) {
    c.check(
        rd::map_anon(PAGE, MemFlags::WRITE) == Err(Error::InvalidArgument),
        "map_anon refuses W without R",
    );

    let rw = rd::map_anon(2 * PAGE, rd::rw()).unwrap();
    rd::poke(rw, 0x1122);
    rd::poke(rw + PAGE, 0x3344);
    c.check(
        rd::set_flags(rw, PAGE, MemFlags::WRITE) == Err(Error::InvalidArgument),
        "set_flags refuses W without R",
    );
    c.check(
        rd::set_flags(rw, 2 * PAGE, MemFlags::WRITE) == Err(Error::InvalidArgument),
        "set_flags refuses W without R over a range",
    );
    // Still readable (the reads below would fault otherwise) and still a writable record.
    c.check(rd::peek(rw) == 0x1122 && rd::peek(rw + PAGE) == 0x3344, "refused set_flags kept the contents");
    c.check(
        writable_record(rw) && writable_record(rw + PAGE),
        "refused set_flags left the pages readable and writable",
    );

    let ro = rd::map_anon(PAGE, MemFlags::READ).unwrap();
    rd::peek(ro);
    c.check(
        rd::set_flags(ro, PAGE, MemFlags::WRITE) == Err(Error::InvalidArgument),
        "set_flags refuses W without R on a read-only page",
    );
    c.check(!writable_record(ro), "refused set_flags did not make a read-only page writable");
    rd::peek(ro);
    rd::unmap(rw, 2 * PAGE).unwrap();
    rd::unmap(ro, PAGE).unwrap();
}

/// The legacy `UpdateMemoryFlags` path (redoubt-abi, not `redoubt-sys`): permissions may
/// only be stripped, and stripping R while keeping W must not leave a writable-without-
/// readable page (arch/riscv/mem.rs update_page_flags).
fn legacy_update_flags(c: &mut Checker) {
    let rw = rd::map_anon(PAGE, rd::rw()).unwrap();
    rd::poke(rw, 0x7788);
    // SAFETY: `rw` is this process's own page-aligned, PAGE-sized mapping from map_anon above.
    let range = unsafe { redoubt_abi::MemoryRange::new(rw, PAGE) }.unwrap();
    c.check(
        redoubt_abi::update_memory_flags(range, redoubt_abi::MemoryFlags::W)
            == Err(redoubt_abi::Error::MemoryInUse),
        "update_memory_flags refuses W without R",
    );
    c.check(rd::peek(rw) == 0x7788, "refused update_memory_flags kept the contents");
    c.check(writable_record(rw), "refused update_memory_flags left the page readable and writable");
    rd::unmap(rw, PAGE).unwrap();
}

fn process_map(c: &mut Checker) {
    let exit = rd::endpoint_create().unwrap();
    let budget = rd::create(rd::SYSTEM, &rd::spec(32, 1, 10)).unwrap();
    let process = rd::process_create(budget, exit).unwrap();
    let src = rd::map_anon(PAGE, rd::rw()).unwrap();
    rd::poke(src, 0x5566);
    let before = rd::usage(budget).unwrap();
    c.check(
        rd::process_map(process, src, DEST, PAGE, MemFlags::WRITE) == Err(Error::InvalidArgument),
        "process_map refuses W without R",
    );
    c.check(rd::usage(budget) == Ok(before), "refused process_map charged the child nothing");
    c.check(rd::peek(src) == 0x5566, "refused process_map left the source in place");
    c.check(writable_record(src), "refused process_map left the source readable and writable");
    // The destination is still free in the child: occupied, this would be InvalidArgument.
    c.check(
        rd::process_map(process, src, DEST, PAGE, rd::rw()) == Ok(()),
        "refused process_map mapped nothing",
    );
    rd::destroy(budget).unwrap();
    rd::close(exit).unwrap();
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut c = Checker(unsafe { MmioSerialPort::new(uart) });
    c.0.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(c.0).ok();
    set_flags(&mut c);
    legacy_update_flags(&mut c);
    process_map(&mut c);
    writeln!(c.0, "[write-only] WRITE-ONLY ATTACK TEST PASSED").ok();
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
        writeln!(out, "[write-only] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
