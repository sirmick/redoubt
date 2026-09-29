//! Every budget full at once (R6; kernel/budgets.md, "R6 (charging)"): `root`'s limit leaves out
//! `root`'s own page, which no budget pays for, so the charges the tree promises never exceed the
//! free frames. A call that checks the budget and then takes frames it was promised
//! (`map_fixed`) must then always find them, and the last one past every limit is refused with
//! `OutOfMemory` instead of stopping the kernel.
//!
//! The loader's trusted first program, in `system`, holding the three boot budgets (`root` keeps
//! no free pages; its limit is `system`'s and `users`' limits and pages). A child in `users` fills
//! `users`, then calls; this program then fills `system`. Each fill ends with single pages into a
//! page table that already exists, each costing exactly one page, so the budget ends at exactly
//! its limit. The verdicts are the kernel's: the budgets' usage, the last `map_fixed`'s error,
//! and a reply the kernel still carries afterwards.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, FOREVER, PAGE_SIZE, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

static CONSOLE: AtomicUsize = AtomicUsize::new(0);

/// `users`, in this program's table (the boot order: root, system, users).
const USERS: u32 = 3;
/// Where each process opens the page table its last single pages go into: nothing else of a
/// test program or a spawned child is there.
const TABLE_AT: usize = 0x5000_0000;
/// The slots of that table after its first page (Sv39 leaf tables hold 512 entries, Sv32 1024).
const SLOTS: usize = if cfg!(target_pointer_width = "64") { 511 } else { 1023 };

fn check(out: &mut MmioSerialPort, ok: bool, label: &str) {
    writeln!(out, "[exhaust] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
    assert!(ok, "{}", label);
}

/// Fill `budget`, the one this process runs in, to its limit. Big `map_anon` runs first, each
/// leaving room for its page tables, then single pages into the table at `TABLE_AT`. Returns the
/// error of the first single page refused.
fn fill(budget: u32) -> Error {
    rd::map_fixed(TABLE_AT, PAGE_SIZE, rd::rw()).expect("open the last table");
    loop {
        let free = rd::free(budget) as usize;
        if free <= 32 {
            break;
        }
        // A run of n pages needs at most n / 512 + 3 page tables.
        let n = free - free / 256 - 8;
        rd::map_anon(n * PAGE_SIZE, rd::rw()).expect("a run the budget can pay for");
    }
    for slot in 1..=SLOTS {
        if let Err(e) = rd::map_fixed(TABLE_AT + slot * PAGE_SIZE, PAGE_SIZE, rd::rw()) {
            return e;
        }
    }
    panic!("the last table's slots outlasted the budget");
}

/// The child, in `users`: it fills `users` and calls, and waits there for good.
extern "C" fn child(_arg: usize) -> ! {
    // Slot 1: the parent's endpoint; slot 2: `users`.
    let e = fill(2);
    let ok = usize::from(e == Error::OutOfMemory);
    rd::call(1, &rd::body([ok, 0, 0, 0]), None, FOREVER).ok();
    loop {
        rd::receive(None, FOREVER, 0).ok();
    }
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(out).ok();
    let out = &mut out;

    let endpoint = rd::endpoint_create().expect("endpoint");
    let exit = rd::endpoint_create().expect("exit endpoint");
    let image = spawn::image();
    spawn::spawn(&image, USERS, exit, child as *const () as usize, &[], &[endpoint, USERS]).expect("child");
    let Ok(Received::Message(m)) = rd::receive(Some(endpoint), FOREVER, 0) else {
        panic!("expected the child's call")
    };
    check(
        out,
        m.body.words[0] == 1 && rd::free(USERS) == 0,
        "users filled to its limit; its last map_fixed is OutOfMemory",
    );

    let e = fill(rd::SYSTEM);
    check(
        out,
        e == Error::OutOfMemory && rd::free(rd::SYSTEM) == 0,
        "system filled to its limit; its last map_fixed is OutOfMemory",
    );
    check(out, rd::reply(m.msg_id.get(), &rd::body([0; 4])).is_ok(), "the kernel still carries a reply");

    writeln!(out, "[exhaust] PAGES EXHAUSTION PASSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    loop {
        rd::receive(None, FOREVER, 0).ok();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = CONSOLE.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: only this process initializes CONSOLE, to its granted UART page.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[exhaust] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
