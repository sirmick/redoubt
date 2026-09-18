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
    crate::debug::shell::init(unsafe { &mut *(&raw mut CONSOLE) });
}

pub fn init() { rand::init(); }
