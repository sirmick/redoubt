// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform support for any machine where the kernel runs under SBI firmware: QEMU
//! `virt` and, later, FPGA softcores. Nothing here is specific to a board or to XLEN.
//!
//! The kernel owns no devices here. Its console goes through the SBI debug console, and
//! every real device (UART, virtio, ...) belongs to a userspace server.

pub mod rand;

use crate::io::SerialWrite;

/// The SBI debug console. It keeps no state, so a print or a panic that strikes inside `print!`
/// may write to a fresh one at any time (`debug::console`).
pub struct SbiConsole;

impl SerialWrite for SbiConsole {
    fn putc(&mut self, b: u8) { sbi_rt::console_write_byte(b); }
}

static mut CONSOLE: SbiConsole = SbiConsole;

/// The SBI console needs no setup, so bring it up before anything that might panic.
pub fn early_init() {
    #[cfg(any(feature = "debug-print", feature = "print-panics"))]
    // SAFETY: `early_init` runs once, at boot, before anything else can refer to `CONSOLE`,
    // so this is the only reference to it that ever exists. (`SbiConsole` has no state.)
    crate::debug::console::init(unsafe { &mut *(&raw mut CONSOLE) });
}

pub fn init() { rand::init(); }

/// `system_reset` through the SBI SRST extension. Only `PowerOffFailure` gives a reason,
/// `SystemFailure`, which QEMU reports as exit status 255. The firmware does not return;
/// this returns only when it refused (a machine with no SRST implementation), and the caller
/// then fails closed.
pub fn reset(kind: redoubt_sys::ResetKind) {
    use redoubt_sys::ResetKind;
    match kind {
        ResetKind::PowerOff => sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason),
        ResetKind::Reboot => sbi_rt::system_reset(sbi_rt::ColdReboot, sbi_rt::NoReason),
        ResetKind::PowerOffFailure => sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::SystemFailure),
    };
}
