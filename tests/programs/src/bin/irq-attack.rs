//! Attacks the device objects (WP-K3): this program holds no device handle at all, and tries
//! to reach the victim's devices anyway -- its interrupt, its MMIO and the Reset right. Every
//! call must come back as an error, none may panic the kernel, and
//! the victim (`log-server`, the bundle's first program, which holds every device object) must
//! still hear the input the bench sends afterwards.
//!
//! It holds the boot endpoint (kernel `budget.rs`, `boot_endpoint`) and its log handle, so the
//! boot endpoint is also the wrong-kind handle every device call is offered.
//!
//! Its own reports can only fail the case: the verdict is the victim's
//! (docs/testbench.md, "Writing an attack case").

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, ResetKind};
use test_programs::{Logger, log};

/// An index no process holds: past every handle this program was given.
fn unheld() -> u32 { rd::first_free() }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let free = unheld();
    let mine = rd::BOOT_ENDPOINT;

    // Receiving on an interrupt this program does not hold. The victim's console IRQ is at
    // handle 6 in the *victim's* table; an index means nothing outside the table it indexes
    // (I1), so here it is simply a handle that does not exist. `0` cannot even be encoded.
    let refused = [
        rd::receive(Some(rd::CONSOLE_IRQ), 0, 0).err(),
        rd::receive(Some(rd::RESET), 0, 0).err(),
        rd::receive(Some(free), 0, 0).err(),
        rd::receive(Some(u32::MAX), 0, 0).err(),
    ];
    let ok = refused.iter().all(|e| *e == Some(Error::BadHandle));
    log!(logger, "[irq-attack] {}: receive on an irq handle it does not hold -> {:?}",
        if ok { "ok" } else { "FAIL" }, refused);

    // Mapping the victim's MMIO range without its handle. There is no call that takes a
    // physical address at all.
    let by_index = [rd::map_device(rd::CONSOLE_MMIO).err(), rd::map_device(free).err()];
    let ok = by_index.iter().all(|e| *e == Some(Error::BadHandle));
    log!(logger, "[irq-attack] {}: mapping the console's mmio -> {:?}", if ok { "ok" } else { "FAIL" }, by_index);

    // RAM cannot be named by physical address either (R11): `map_anon` takes no address, and
    // the pages it hands out are zero.
    let zero = rd::map_anon(rd::PAGE_SIZE, rd::rw()).map(rd::peek);
    log!(logger, "[irq-attack] {}: map_anon first word -> {:?}", if zero == Ok(0) { "ok" } else { "FAIL" }, zero);

    // A handle of the wrong kind is `WrongObject`, and one that does not exist `BadHandle`,
    // for every device call -- including `system_reset`, which would end the case early.
    let wrong = [
        rd::map_device(mine).err(),
        rd::dma_alloc(mine, 1).err(),
        rd::system_reset(mine, ResetKind::PowerOff).err(),
    ];
    let absent = [
        rd::dma_alloc(free, 1).err(),
        rd::system_reset(free, ResetKind::PowerOff).err(),
        rd::system_reset(rd::RESET, ResetKind::Reboot).err(),
    ];
    let ok = wrong.iter().all(|e| *e == Some(Error::WrongObject))
        && absent.iter().all(|e| *e == Some(Error::BadHandle));
    log!(logger, "[irq-attack] {}: wrong kind -> {:?}, not held -> {:?}",
        if ok { "ok" } else { "FAIL" }, wrong, absent);

    // The verdict is the victim's, not ours (docs/testbench.md, "Writing an attack case").
    log!(logger, "[irq-attack] attempts done");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
