// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform support for any machine where the kernel runs under SBI firmware: QEMU
//! `virt` and, later, FPGA softcores. Nothing here is specific to a board or to XLEN.
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

/// `system_reset` through the SBI SRST extension. The firmware does not return from either;
/// this returns only when it refused (a machine with no SRST implementation), and the caller
/// then fails closed.
pub fn reset(reboot: bool) {
    if reboot {
        sbi_rt::system_reset(sbi_rt::ColdReboot, sbi_rt::NoReason);
    } else {
        sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
    }
}

/// `SysCall::PlatformSpecific` for SBI platforms. There are no platform calls any more: the hart
/// timer, whose two calls these were, is the kernel's (`time.rs`). The legacy call itself goes
/// with WP-K6.
pub fn platform_call(
    _pid: redoubt_abi::PID,
    _op: usize,
    _a2: usize,
    _a3: usize,
) -> Result<redoubt_abi::Result, redoubt_abi::Error> {
    Err(redoubt_abi::Error::UnhandledSyscall)
}
