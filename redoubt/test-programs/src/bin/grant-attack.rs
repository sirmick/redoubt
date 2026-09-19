//! Ambient-authority attack. This program is given no device grants, so the kernel must
//! refuse it every device page and every interrupt it asks for, while the granted
//! log-server keeps working. Denials must be errors, never crashes.

#![no_std]
#![no_main]

use test_programs::{log, Logger};
use xous::{MemoryAddress, MemoryFlags, SysCall};

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

fn nop_handler(_irq: usize, _arg: *mut usize) {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let mut failures = 0;

    // Devices belonging to other components, and the power-off register, must be denied.
    for (what, base) in [("the UART", UART), ("the power-off device", POWEROFF)] {
        let result = try_map(base);
        let ok = result == Err(xous::Error::AccessDenied);
        failures += !ok as usize;
        log!(logger, "[grant] {}: mapping {} -> {:?}", if ok { "ok" } else { "FAIL" }, what, result);
    }

    // Ordinary anonymous RAM is not a device and must still work.
    let ram = xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W);
    let ok = ram.is_ok();
    failures += !ok as usize;
    log!(logger, "[grant] {}: anonymous RAM still maps -> {:?}", if ok { "ok" } else { "FAIL" }, ram.map(|_| "ok"));

    // Claiming any interrupt without a grant must be denied.
    for irq in [0usize, 10, 6] {
        let result = xous::claim_interrupt(irq, nop_handler, core::ptr::null_mut());
        let ok = result == Err(xous::Error::AccessDenied);
        failures += !ok as usize;
        log!(logger, "[grant] {}: claiming irq {} -> {:?}", if ok { "ok" } else { "FAIL" }, irq, result);
    }

    if failures == 0 {
        log!(logger, "GRANT ATTACK PASSED");
    } else {
        log!(logger, "GRANT ATTACK FAILED: {} failure(s)", failures);
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
