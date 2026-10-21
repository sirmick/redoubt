//! A budget's deadline passes while a process in it is being launched (kernel/budgets.md,
//! "Deadlines" and R10): between `process_create` and `process_start`, with its image already
//! mapped, the lease `L` expires. The kernel destroys `L` at the next entry as for any deadline:
//! the child, which has no thread yet, is killed with the rest, its exit notice is `killed`
//! blaming nobody, and the launcher's next `process_map` and `process_start` answer
//! `NotPermitted`. Nothing leaks: once the notice is taken, the handles to the child and to `L`
//! are gone and `system`'s usage is what it was before `L` (I10); the child's PID is not drawn
//! again while its notice is pending (R20).
//!
//! This is what the steward's `Launch` step meets when a lease's term is shorter than its launch;
//! the kernel's part of it is pinned here.
//!
//! This program is the bundle's only one: it prints on the UART itself and powers off.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// The lease's term: room to create the child and map its image, not to start it.
const LEASE_US: u64 = 300_000;

fn now() -> u64 { rd::time_now().expect("time_now") }

fn ok(b: bool) -> &'static str { if b { "ok" } else { "FAIL" } }

fn spec(pages: u64, processes: u32, weight: u32, deadline: u64) -> rd::BudgetSpec {
    rd::BudgetSpec { deadline, ..rd::spec(pages, processes, weight) }
}

/// The witness for R20: a child that exits at once, so its notice names the PID it drew.
extern "C" fn witness(_arg: usize) -> ! { rd::process_exit(7) }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    // SAFETY: this program is the only one, and owns the UART it just mapped.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();
    let exit = rd::endpoint_create().unwrap();

    // The lease, and a child created and given its image inside the term.
    let before = rd::usage(rd::SYSTEM).unwrap();
    let deadline = now() + LEASE_US;
    let lease = rd::create(rd::SYSTEM, &spec(700, 2, 10, deadline)).unwrap();
    let process = rd::process_create(lease, exit).unwrap();
    let mapped = spawn::give_image(process, &image);
    let mid = rd::usage(lease).unwrap();
    writeln!(
        out,
        "[launch] {}: the child was created and given its image before the deadline ({} pages, {} process)",
        ok(mapped.is_ok() && mid.pages_usage >= image.pages() as u64 && mid.processes_usage == 1),
        mid.pages_usage,
        mid.processes_usage
    )
    .ok();

    // The deadline passes before the launch continues: the entry that finds it passed destroys
    // the lease (this process runs outside it and keeps running).
    while now() <= deadline {}
    let scratch = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap();
    let map = rd::process_map(process, scratch, spawn::STACK_TOP - rd::PAGE_SIZE, rd::PAGE_SIZE, rd::rw());
    let start = rd::process_start(process, witness as *const () as usize, spawn::STACK_TOP - 16, 0, &[]);
    writeln!(
        out,
        "[launch] {}: after the deadline the launcher's map and start answer NotPermitted ({:?}, {:?})",
        ok(map == Err(Error::NotPermitted) && start == Err(Error::NotPermitted)),
        map,
        start
    )
    .ok();
    let revoked = rd::usage(lease).err();
    writeln!(
        out,
        "[launch] {}: the lease's handle is revoked ({:?}); the child's still names its notice",
        ok(revoked == Some(Error::Dead) && rd::usage(process) == Err(Error::WrongObject)),
        revoked
    )
    .ok();

    // R20: while the killed child's notice is pending, a process started elsewhere does not draw
    // its PID. The witness exits at once; its notice says which PID it drew.
    let other = rd::create(rd::SYSTEM, &spec(700, 2, 10, rd::FOREVER)).unwrap();
    spawn::spawn(&image, other, exit, witness as *const () as usize, &[], &[]).unwrap();
    let killed = rd::receive(Some(exit), 2_000_000, 0);
    let exited = rd::receive(Some(exit), 2_000_000, 0);
    let (killed_pid, good) = match &killed {
        Ok(Received::Exit(n)) => (
            n.pid,
            n.cause == Cause::Killed
                && n.code == 0
                && n.blamed_account == 0
                && n.blamed_labels.as_slice().is_empty(),
        ),
        _ => (0, false),
    };
    writeln!(
        out,
        "[launch] {}: the child that never ran gets a killed notice blaming nobody (PID {})",
        ok(good),
        killed_pid
    )
    .ok();
    let witness_pid = match &exited {
        Ok(Received::Exit(n)) if n.cause == Cause::Exited && n.code == 7 => n.pid,
        _ => 0,
    };
    writeln!(
        out,
        "[launch] {}: a process started while the notice was pending drew another PID ({})",
        ok(witness_pid != 0 && witness_pid != killed_pid),
        witness_pid
    )
    .ok();

    // Nothing leaks: the notice taken, the handle to the child is dead (I17), and once the
    // witness's budget goes too, `system`'s usage is what it was before the lease (I10).
    rd::destroy(other).unwrap();
    let gone = rd::usage(process).err();
    let after = rd::usage(rd::SYSTEM).unwrap();
    writeln!(
        out,
        "[launch] {}: the child's handle is gone ({:?}) and system's usage restored (before {:?}, after {:?})",
        ok(gone == Some(Error::Dead) && after == before),
        gone,
        before,
        after
    )
    .ok();

    writeln!(out, "DEADLINE-IN-LAUNCH TEST PASSED").ok();
    let _ = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // The UART is mapped at a kernel-chosen address; without it, park and let the bench time out.
    let _ = info;
    test_programs::park()
}
