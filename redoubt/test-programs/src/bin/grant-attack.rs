//! Ambient-authority attack. This program is given no device grants and tries to take the
//! devices anyway. Whatever it gets, it uses, so that a breach shows up where it cannot fake
//! it (redoubt/README.md, "Writing an attack case"):
//!   - the UART: it writes a raw `BREACH` line, which only a UART owner can print unprefixed;
//!   - the power-off device: it powers the machine off, so the victim's later lines never come;
//!   - UART interrupt 10: it keeps it, so the victim (log-server) never hears the input.
//! Denials must be errors, never crashes. Its own reports can only fail the case.

#![no_std]
#![no_main]

use test_programs::{log, Logger};
use xous::{MemoryAddress, MemoryFlags, MemoryRange, SysCall};

// Devices on QEMU virt this program was NOT granted.
const UART: usize = 0x1000_0000;
const POWEROFF: usize = 0x0010_0000;

fn try_map(base: usize) -> Result<xous::Result, xous::Error> {
    xous::rsyscall(SysCall::MapMemory(
        MemoryAddress::new(base),
        None,
        xous::MemorySize::new(4096).unwrap(),
        MemoryFlags::R | MemoryFlags::W,
    ))
}

/// The page the kernel mapped, if it did.
fn mapped(result: &Result<xous::Result, xous::Error>) -> Option<MemoryRange> {
    match result {
        Ok(xous::Result::MemoryRange(range)) => Some(*range),
        _ => None,
    }
}

fn keep_it(_irq: usize, _arg: *mut usize) {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();

    let uart = try_map(UART);
    log!(logger, "[grant] mapping the UART -> {:?}", uart);
    if let Some(page) = mapped(&uart) {
        log!(logger, "BREACH: mapped the UART");
        for &byte in b"\nBREACH: an ungranted process wrote to the UART\n" {
            // SAFETY: the kernel mapped the UART's page here (the bug under test); its transmit
            // register is the first byte. Writing it is the attack.
            unsafe { page.as_mut_ptr().write_volatile(byte) };
        }
    }

    let poweroff = try_map(POWEROFF);
    log!(logger, "[grant] mapping the power-off device -> {:?}", poweroff);
    if let Some(page) = mapped(&poweroff) {
        log!(logger, "BREACH: mapped the power-off device");
        // SAFETY: as above, for the test device; 0x5555 in its first register powers off.
        unsafe { (page.as_mut_ptr() as *mut u32).write_volatile(0x5555) };
    }

    // Ordinary anonymous RAM is not a device and must still work.
    let ram = xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W);
    log!(logger, "[grant] anonymous RAM -> {:?}", ram.map(|_| "mapped"));

    // Claiming any interrupt without a grant must be denied. Irq 10 is the victim's.
    for irq in [0usize, 10, 6] {
        let result = xous::claim_interrupt(irq, keep_it, core::ptr::null_mut());
        log!(logger, "[grant] claiming irq {} -> {:?}", irq, result);
        if result.is_ok() {
            log!(logger, "BREACH: claimed irq {}", irq);
        }
    }

    log!(logger, "[grant] attempts done");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
