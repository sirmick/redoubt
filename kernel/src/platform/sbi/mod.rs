// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform support for any machine where the kernel runs under SBI firmware: QEMU
//! `virt`, softcores, SBCs. Nothing here is specific to a board or to XLEN.
//!
//! The kernel owns no devices here. Its console goes through the SBI debug console, and
//! every real device (UART, virtio, ...) belongs to a userspace server.

pub mod rand;

use crate::io::SerialWrite;

struct SbiConsole;

impl SerialWrite for SbiConsole {
    fn putc(&mut self, b: u8) { sbi_rt::console_write_byte(b); }
}

static mut CONSOLE: SbiConsole = SbiConsole;

/// The SBI console needs no setup, so bring it up before anything that might panic.
pub fn early_init() {
    #[cfg(any(feature = "debug-print", feature = "print-panics"))]
    // SAFETY: `early_init` runs once, at boot, before anything else can refer to `CONSOLE`,
    // so this is the only reference to it that ever exists. (`SbiConsole` has no state.)
    crate::debug::shell::init(unsafe { &mut *(&raw mut CONSOLE) });
}

pub fn init() { rand::init(); }

/// `SysCall::PlatformSpecific` for SBI platforms. Numbers are in `xous::arch::platform_call`.
pub fn platform_call(pid: xous_kernel::PID, op: usize, a2: usize, _a3: usize) -> Result<xous_kernel::Result, xous_kernel::Error> {
    use xous_kernel::arch::platform_call::*;

    use crate::arch::irq::timer;
    match op {
        TIMER_TIMEBASE => Ok(xous_kernel::Result::Scalar1(timer::timebase() as usize)),
        TIMER_SET_DEADLINE => {
            // The hart timer belongs to whoever claimed its interrupt.
            if crate::irq::interrupt_owner(timer::IRQ) != Some(pid) {
                return Err(xous_kernel::Error::AccessDenied);
            }
            timer::set_deadline(a2 as u64);
            Ok(xous_kernel::Result::Ok)
        }
        _ => Err(xous_kernel::Error::UnhandledSyscall),
    }
}
