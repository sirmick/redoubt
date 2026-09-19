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

    // One past the end of the kernel's table, further out, and as far out as possible.
    // FreeInterrupt(32) used to pass an off-by-one bounds check and index out of range.
    for irq in [32, 33, 64, 1 << 20, usize::MAX] {
        let freed = xous::rsyscall(SysCall::FreeInterrupt(irq));
        let claimed = xous::claim_interrupt(irq, never_called, core::ptr::null_mut());
        let ok = freed.is_err() && claimed.is_err();
        log!(logger, "[irq-attack] {}: irq {:#x}: free -> {:?}, claim -> {:?}", if ok { "ok" } else { "FAIL" }, irq, freed, claimed);
    }

    // Freeing an interrupt that exists but belongs to nobody, or to somebody else: irq 10 is
    // the victim's (log-server), which must still hear the input the bench sends afterwards.
    let freed = xous::rsyscall(SysCall::FreeInterrupt(5));
    let ok = freed.is_err();
    log!(logger, "[irq-attack] {}: freeing an unclaimed irq -> {:?}", if ok { "ok" } else { "FAIL" }, freed);
    let freed = xous::rsyscall(SysCall::FreeInterrupt(10));
    let ok = freed.is_err();
    log!(logger, "[irq-attack] {}: freeing the victim's irq 10 -> {:?}", if ok { "ok" } else { "FAIL" }, freed);

    // Claiming one twice.
    let first = xous::claim_interrupt(6, never_called, core::ptr::null_mut());
    let second = xous::claim_interrupt(6, never_called, core::ptr::null_mut());
    let ok = first.is_ok() && second == Err(xous::Error::InterruptInUse);
    log!(logger, "[irq-attack] {}: double claim -> {:?}, {:?}", if ok { "ok" } else { "FAIL" }, first, second);

    // The verdict is the victim's, not ours (redoubt/README.md, "Writing an attack case").
    log!(logger, "[irq-attack] attempts done");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
