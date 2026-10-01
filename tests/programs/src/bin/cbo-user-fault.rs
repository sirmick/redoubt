//! No cache-block operation runs in user mode (kernel/memory.md, R11): the kernel writes `senvcfg`
//! 0 on every hart, so `cbo.inval`, which could discard the zeroes the kernel wrote to a fresh page,
//! traps, and `cbo.clean`, `cbo.flush` and `cbo.zero` with it.
//!
//! It runs as the bundle's first program, alone, so it holds the console and judges the kernel itself
//! (docs/testbench.md, "Rule F (trusted verdicts)"). Each child maps a page of its own, runs one
//! operation on it and exits 0. The verdict is the exit notice the kernel writes: faulted with an
//! illegal instruction (cause 2). A child whose operation ran exits 0 instead, which the kernel
//! reports as `exited`; a child cannot write a `faulted` notice by exiting, since it holds no open
//! call. The program goes on past a child whose operation ran, so a run reports all four, and it
//! prints the closing verdict line only if all four faulted. The children get no console.

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

static UART: AtomicUsize = AtomicUsize::new(0);

/// The illegal instruction cause.
const ILLEGAL_INSTRUCTION: u32 = 2;

/// Run operation `op` on the cache block at `at`. The operations are `MISC-MEM` (opcode 0x0f),
/// funct3 2, `rd` x0, `rs1` the address, and the immediate names the operation (0 inval, 1 clean,
/// 2 flush, 4 zero); they are written with `.insn` because the targets' assemblers do not enable
/// Zicbom and Zicboz.
fn cbo(op: u8, at: usize) {
    // SAFETY: none needed by this program: each operation is on a page the child has just mapped
    // read-write, and the kernel's verdict (an illegal-instruction fault) is the point.
    unsafe {
        match op {
            0 => core::arch::asm!(".insn i 0x0f, 2, x0, {0}, 0", in(reg) at),
            1 => core::arch::asm!(".insn i 0x0f, 2, x0, {0}, 1", in(reg) at),
            2 => core::arch::asm!(".insn i 0x0f, 2, x0, {0}, 2", in(reg) at),
            _ => core::arch::asm!(".insn i 0x0f, 2, x0, {0}, 4", in(reg) at),
        }
    }
}

/// A child: maps a page, runs the operation its startup byte names on it, and exits 0 only if the
/// operation ran; 1 if it could not map the page.
extern "C" fn child(arg: usize) -> ! {
    let Ok(at) = rd::map_anon(rd::PAGE_SIZE, rd::rw()) else { rd::process_exit(1) };
    cbo(spawn::startup_byte(arg, 0), at);
    rd::process_exit(0)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    writeln!(out).ok();

    let image = spawn::image();
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let budget = rd::create(rd::USERS, &rd::spec(500, 2, 50)).expect("a budget for the children");
    let mut faulted = 0;
    for (op, name) in [(0u8, "cbo.inval"), (1, "cbo.clean"), (2, "cbo.flush"), (4, "cbo.zero")] {
        spawn::spawn(&image, budget, exit, child as *const () as usize, &[op], &[]).expect("a child");
        let Ok(Received::Exit(n)) = rd::receive(Some(exit), 2_000_000, 0) else { panic!("no notice") };
        if (n.cause, n.code) == (Cause::Faulted, ILLEGAL_INSTRUCTION) {
            faulted += 1;
            writeln!(out, "[cbo-user-fault] ok: {name} by a child faulted with cause 2").ok();
        } else {
            writeln!(
                out,
                "[cbo-user-fault] FAIL: {name} by a child ended {:?} with code {}",
                n.cause, n.code
            )
            .ok();
        }
    }
    if faulted == 4 {
        writeln!(out, "[cbo-user-fault] CBO USER FAULT").ok();
    }
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: this program mapped the UART and has stopped normal execution.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[cbo-user-fault] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
