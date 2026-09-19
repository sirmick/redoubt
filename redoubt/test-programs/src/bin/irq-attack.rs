//! Attacks the interrupt syscalls with arguments chosen to break bounds checks and
//! ownership checks. Every call must come back as an error. None may panic the kernel.

#![no_std]
#![no_main]

use test_programs::{log, Logger};
use xous::SysCall;

fn never_called(_irq: usize, _arg: *mut usize) {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let mut failures = 0;

    // One past the end of the kernel's table, further out, and as far out as possible.
    // FreeInterrupt(32) used to pass an off-by-one bounds check and index out of range.
    for irq in [32, 33, 64, 1 << 20, usize::MAX] {
        let freed = xous::rsyscall(SysCall::FreeInterrupt(irq));
        let claimed = xous::claim_interrupt(irq, never_called, core::ptr::null_mut());
        let ok = freed.is_err() && claimed.is_err();
        failures += !ok as usize;
        log!(logger, "[irq-attack] {}: irq {:#x}: free -> {:?}, claim -> {:?}", if ok { "ok" } else { "FAIL" }, irq, freed, claimed);
    }

    // Freeing an interrupt that exists but belongs to nobody, or to somebody else.
    let freed = xous::rsyscall(SysCall::FreeInterrupt(5));
    let ok = freed.is_err();
    failures += !ok as usize;
    log!(logger, "[irq-attack] {}: freeing an unclaimed irq -> {:?}", if ok { "ok" } else { "FAIL" }, freed);

    // Claiming one twice.
    let first = xous::claim_interrupt(6, never_called, core::ptr::null_mut());
    let second = xous::claim_interrupt(6, never_called, core::ptr::null_mut());
    let ok = first.is_ok() && second == Err(xous::Error::InterruptInUse);
    failures += !ok as usize;
    log!(logger, "[irq-attack] {}: double claim -> {:?}, {:?}", if ok { "ok" } else { "FAIL" }, first, second);

    if failures == 0 {
        log!(logger, "IRQ ATTACK TEST PASSED");
    } else {
        log!(logger, "IRQ ATTACK TEST FAILED: {} failure(s)", failures);
    }
    // The verdict is the checker's, not ours (redoubt/README.md, "Writing an attack case").
    test_programs::checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
