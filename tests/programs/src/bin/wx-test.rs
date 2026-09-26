//! Trusted W^X checker. Children have no device grants; only kernel fault notices determine
//! whether writable data could execute or executable code could be modified.
#![no_std]
#![no_main]
use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, MemFlags, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;
static UART: AtomicUsize = AtomicUsize::new(0);
extern "C" fn attack(arg: usize) -> ! {
    if spawn::startup_byte(arg, 0) == 1 {
        let page = rd::page();
        // SAFETY: the attacker owns this writable page; the word is RISC-V `ret`.
        unsafe { (page as *mut u32).write_volatile(0x0000_8067) };
        // Adding X while the page is writable must fail. The verdict is the ensuing kernel
        // instruction-page fault, not the result reported by this attacker.
        rd::set_flags(page, rd::PAGE_SIZE, rd::rw() | MemFlags::EXECUTE).ok();
        // SAFETY: deliberately hostile execution of non-executable memory; must fault.
        let jump: extern "C" fn() = unsafe { core::mem::transmute(page as *const u8) };
        jump();
    } else {
        let code = attack as *const () as usize;
        // SAFETY: deliberately hostile write into executable memory; must fault.
        unsafe { (code as *mut u8).write_volatile(0) };
    }
    rd::process_exit(99)
}
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: only this checker owns the mapped UART.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    writeln!(out).ok();
    let page = rd::page();
    for flags in [rd::rw() | MemFlags::EXECUTE, MemFlags::WRITE | MemFlags::EXECUTE, MemFlags::NONE] {
        assert!(rd::map_anon(rd::PAGE_SIZE, flags).is_err());
        assert!(rd::set_flags(page, rd::PAGE_SIZE, flags).is_err());
    }
    writeln!(out, "[wx] checker: kernel refuses W+X and empty permissions").ok();
    let image = spawn::image();
    let exit = rd::endpoint_create().unwrap();
    let budget = rd::create(rd::USERS, &rd::spec(500, 2, 50)).unwrap();
    for (role, expected) in [(1, 12), (2, 15)] {
        spawn::spawn(&image, budget, exit, attack as *const () as usize, &[role], &[]).unwrap();
        let Received::Exit(n) = rd::receive(Some(exit), 2_000_000, 0).unwrap() else { panic!("no notice") };
        assert_eq!((n.cause, n.code), (Cause::Faulted, expected));
        assert_eq!(n.blamed_account, 0);
        assert!(n.blamed_labels.as_slice().is_empty());
        writeln!(out, "[wx] checker: attack {role} faulted with kernel cause {expected}").ok();
    }
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: the checker mapped this UART and has stopped normal execution.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[wx] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
