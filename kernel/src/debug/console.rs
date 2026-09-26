// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

use crate::args::KernelArguments;
use crate::io::SerialWrite;

/// The kernel console, which `print!` writes to.
pub static mut OUTPUT: Option<Output> = None;

/// The kernel console: a serial port, lines optionally wrapped at 80 columns (`wrap-print`).
pub struct Output {
    serial: &'static mut dyn SerialWrite,
    #[cfg(feature = "wrap-print")]
    character_count: usize,
}

impl Output {
    fn new(serial: &'static mut dyn SerialWrite) -> Output {
        Output {
            serial,
            #[cfg(feature = "wrap-print")]
            character_count: 0,
        }
    }
}

impl fmt::Write for Output {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            #[cfg(feature = "wrap-print")]
            if c == b'\n' {
                self.character_count = 0;
            } else if self.character_count > 80 {
                self.character_count = 0;
                self.serial.putc(b'\n');
                self.serial.putc(b'\r');
                self.serial.putc(b' ');
                self.serial.putc(b' ');
                self.serial.putc(b' ');
                self.serial.putc(b' ');
            } else {
                self.character_count += 1;
            }

            self.serial.putc(c);
        }
        Ok(())
    }
}

/// Write `args` to the console, if it is set up (`print!`).
pub fn print(args: fmt::Arguments) {
    use fmt::Write;
    // SAFETY: `OUTPUT` is written only by `init`, before the first `print!`, and only the boot
    // hart prints (the `smp` spike's `secondary_main` never does), so this is its only live
    // reference. Known residual: a panic inside this `write_fmt` re-enters through the panic
    // handler's `println!` while the first reference is live; the handler then powers off.
    if let Some(stream) = unsafe { &mut *(&raw mut OUTPUT) } {
        stream.write_fmt(args).unwrap();
    }
}

/// Take `serial` as the kernel console and print the kernel arguments.
///
/// This should be called in platform initialization code.
pub fn init(serial: &'static mut dyn SerialWrite) {
    // SAFETY: the one caller, `platform::sbi::early_init`, runs once on the boot hart before
    // interrupts, other harts or any `print!`, so nothing else refers to `OUTPUT` yet.
    unsafe { OUTPUT = Some(Output::new(serial)) }

    // Print the processed kernel arguments
    let args = KernelArguments::get();
    println!("Kernel arguments:");
    for arg in args.iter() {
        println!("    {}", arg);
    }
}
