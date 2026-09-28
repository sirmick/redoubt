// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::args::KernelArguments;
use crate::io::SerialWrite;

/// The kernel console, which `print!` writes to.
pub static mut OUTPUT: Option<Output<'static>> = None;

/// Set while `print` holds its reference to `OUTPUT`, so that a print or a panic inside the write
/// takes no second one.
static IN_PRINT: AtomicBool = AtomicBool::new(false);

/// The kernel console: a serial port, lines optionally wrapped at 80 columns (`wrap-print`).
pub struct Output<'a> {
    serial: &'a mut dyn SerialWrite,
    #[cfg(feature = "wrap-print")]
    character_count: usize,
}

impl<'a> Output<'a> {
    fn new(serial: &'a mut dyn SerialWrite) -> Output<'a> {
        Output {
            serial,
            #[cfg(feature = "wrap-print")]
            character_count: 0,
        }
    }
}

impl fmt::Write for Output<'_> {
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
    // A print inside this one (a `Display` that prints) takes the stateless console instead.
    if IN_PRINT.swap(true, Ordering::Relaxed) {
        print_stateless(args);
        return;
    }
    // SAFETY: `OUTPUT` is written only by `init`, before the first `print!`, and only the boot
    // hart prints (the `smp` spike's `secondary_main` never does), so this is its only live
    // reference: a print or a panic inside this `write_fmt` finds `IN_PRINT` set and takes none.
    if let Some(stream) = unsafe { &mut *(&raw mut OUTPUT) } {
        stream.write_fmt(args).unwrap();
    }
    IN_PRINT.store(false, Ordering::Relaxed);
}

/// Write the panic handler's `args`. A panic that struck inside `print` must not take `OUTPUT`
/// while that print's reference is live, so it writes to the stateless console, and says so.
pub fn print_panic(args: fmt::Arguments) {
    if IN_PRINT.load(Ordering::Relaxed) {
        print_stateless(format_args!("\r\n(while printing) {}\r\n", args));
    } else {
        println!("{}", args);
    }
}

/// Write `args` to the firmware's console, which keeps no state of its own, so any number of
/// writers may use it at once.
fn print_stateless(args: fmt::Arguments) {
    use fmt::Write;
    let _ = Output::new(&mut crate::platform::sbi::SbiConsole).write_fmt(args);
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
