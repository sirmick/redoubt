//! Attacks the device objects (WP-K3): this program holds no device handle at all, and tries
//! to reach the victim's devices anyway -- its interrupt, its MMIO, the Reset right, and RAM
//! by physical address. Every call must come back as an error, none may panic the kernel, and
//! the victim (`log-server`, the bundle's first program, which holds every device object) must
//! still hear the input the bench sends afterwards.
//!
//! The one handle this program holds is the boot endpoint (kernel `budget.rs`,
//! `boot_endpoint`), so it is also the wrong-kind handle every device call is offered.
//!
//! Its own reports can only fail the case: the verdict is the victim's
//! (docs/testbench.md, "Writing an attack case").

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, ResetKind};
use test_programs::{Logger, log};
use redoubt_abi::{MemoryAddress, MemoryFlags, SysCall};

/// Devices on QEMU virt this program was given no handle to, and a page of RAM. The UART's
/// interrupt is the victim's; QEMU's `virt` wires it to PLIC source 10.
const UART: usize = 0x1000_0000;
const UART_IRQ: usize = 10;
const RAM: usize = 0x8400_0000;

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

    // Mapping the victim's MMIO range without its handle: by handle index, and by physical
    // address through the legacy call, which the grants still deny (DEVICE-GRANTS.md). The
    // Redoubt interface has no call that takes a physical address at all.
    let by_index = [rd::map_device(rd::CONSOLE_MMIO).err(), rd::map_device(free).err()];
    let by_address = map_legacy(UART).err();
    let ok = by_index.iter().all(|e| *e == Some(Error::BadHandle))
        && by_address == Some(redoubt_abi::Error::AccessDenied);
    log!(logger, "[irq-attack] {}: mapping the console's mmio -> {:?}, by address -> {:?}",
        if ok { "ok" } else { "FAIL" }, by_index, by_address);

    // Naming RAM by physical address (R11). The legacy call refuses it outright, and for
    // *that* reason: this program is granted nothing, so a kernel that had dropped the RAM
    // check would still refuse it, with `AccessDenied` from the grant scan. `map_anon` takes
    // no address at all, and the pages it hands out are zero.
    let ram = map_legacy(RAM).err();
    let anon = rd::map_anon(4096, rd::rw());
    let zero = anon.map(rd::peek);
    let ok = ram == Some(redoubt_abi::Error::InvalidArgument) && zero == Ok(0);
    log!(logger, "[irq-attack] {}: RAM by address -> {:?}, map_anon first word -> {:?}",
        if ok { "ok" } else { "FAIL" }, ram, zero);

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

    // The legacy interrupt interface, until WP-K6 deletes it: out-of-range numbers, the
    // victim's interrupt, and one claimed twice. No grant covers any of them.
    let mut survived = true;
    for irq in [UART_IRQ, 32, 33, 64, 1 << 20, usize::MAX] {
        let freed = redoubt_abi::rsyscall(SysCall::FreeInterrupt(irq));
        let claimed = redoubt_abi::claim_interrupt(irq, never_called, core::ptr::null_mut());
        survived &= freed.is_err() && claimed.is_err();
    }
    let verdict = if survived { "ok" } else { "FAIL" };
    log!(logger, "[irq-attack] {}: every legacy claim and free refused", verdict);

    // The hart timer is the kernel's (WP-K5): interrupt 0 does not exist, whatever grants say,
    // and the platform calls that used to program the timer are gone.
    let zero = redoubt_abi::claim_interrupt(0, never_called, core::ptr::null_mut());
    let timer_calls =
        [1, 2].map(|op| redoubt_abi::rsyscall(SysCall::PlatformSpecific(op, 0, 0, 0, 0, 0, 0)).err());
    let ok = zero == Err(redoubt_abi::Error::InterruptNotFound)
        && timer_calls.iter().all(|e| *e == Some(redoubt_abi::Error::UnhandledSyscall));
    log!(logger, "[irq-attack] {}: interrupt 0 -> {:?}, the old timer calls -> {:?}",
        if ok { "ok" } else { "FAIL" }, zero, timer_calls);

    // The verdict is the victim's, not ours (docs/testbench.md, "Writing an attack case").
    log!(logger, "[irq-attack] attempts done");
    test_programs::park()
}

fn never_called(_irq: usize, _arg: *mut usize) {}

fn map_legacy(base: usize) -> Result<redoubt_abi::Result, redoubt_abi::Error> {
    redoubt_abi::rsyscall(SysCall::MapMemory(
        MemoryAddress::new(base),
        None,
        redoubt_abi::MemorySize::new(4096).unwrap(),
        MemoryFlags::R | MemoryFlags::W,
    ))
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
