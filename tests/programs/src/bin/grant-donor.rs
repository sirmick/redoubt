//! A loader program with device grants that uses them and exits (`grant-pid-reuse`): its PID
//! comes free for `process_create` to draw, and its grants must not come with it.
//!
//! It exits only if its own grants work (the case's positive control): otherwise it parks, its
//! PID never comes free, and the launcher reports that.

#![no_std]
#![no_main]

use redoubt_abi::{MemoryAddress, MemoryFlags, SysCall};

/// Granted to this program by the case (an unused PLIC source, and the goldfish RTC's page).
const IRQ: usize = 40;
const MMIO: usize = 0x0010_1000;

fn never_called(_irq: usize, _arg: *mut usize) {}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let claimed = redoubt_abi::claim_interrupt(IRQ, never_called, core::ptr::null_mut()).is_ok();
    let mapped = redoubt_abi::rsyscall(SysCall::MapMemory(
        MemoryAddress::new(MMIO),
        None,
        redoubt_abi::MemorySize::new(4096).unwrap(),
        MemoryFlags::R | MemoryFlags::W,
    ))
    .is_ok();
    if claimed && mapped {
        // Exiting releases the interrupt; the grants stay in the manifest, naming this PID.
        test_programs::rd::process_exit(0)
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
