// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2023 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_conhold::{Busy, Hold, Line};

use crate::args::KernelArguments;
use crate::io::SerialWrite;

/// The kernel console, which `print!` writes to.
pub static mut OUTPUT: Option<Output<'static>> = None;

/// The hart using `OUTPUT` (its boot index plus 1), or 0: one hart at a time holds the reference,
/// and a print or a panic inside its write, on that hart, finds it set and takes none. A print on
/// another hart waits for the line in progress, which is the kernel's own work, never user mode's.
static PRINTER: AtomicUsize = AtomicUsize::new(0);

/// Bytes of the kernel's lines that wait for the console's holder: about 107 kill lines, a
/// containment-sized destruction's. A burst past it goes out inside the holder's write.
const HOLD_BYTES: usize = 4096;

/// The kernel console: a serial port, lines optionally wrapped at 80 columns (`wrap-print`), and
/// the console's hold (kernel/devices.md, "The console's one writer").
pub struct Output<'a> {
    serial: &'a mut dyn SerialWrite,
    hold: Hold<HOLD_BYTES>,
    #[cfg(feature = "wrap-print")]
    character_count: usize,
}

impl<'a> Output<'a> {
    fn new(serial: &'a mut dyn SerialWrite) -> Output<'a> {
        Output {
            serial,
            hold: Hold::new(),
            #[cfg(feature = "wrap-print")]
            character_count: 0,
        }
    }

    /// Prints the kernel's lines that waited for the holder, after `before`, and forgets them.
    fn flush(&mut self, before: &str) {
        use fmt::Write;
        if self.hold.queued().is_empty() {
            return;
        }
        let _ = self.write_str(before);
        for i in 0..self.hold.queued().len() {
            let b = self.hold.queued()[i];
            self.serial.putc(b);
        }
        self.hold.clear();
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

/// This hart's mark in [`PRINTER`].
fn me() -> usize { crate::arch::hart::index() + 1 }

/// Runs `f` on the console, if it is set up, as the one hart using it. `None` if it is not set
/// up, or if this hart is using it already: a print inside a print, which the caller sends to the
/// stateless console.
fn with_output<R>(f: impl FnOnce(&mut Output<'static>) -> R) -> Option<R> {
    let me = me();
    loop {
        match PRINTER.compare_exchange_weak(0, me, Ordering::Acquire, Ordering::Relaxed) {
            Ok(_) => break,
            Err(owner) if owner == me => return None,
            Err(_) => core::hint::spin_loop(),
        }
    }
    // SAFETY: `OUTPUT` is written only by `init`, before the first `print!` and before any other
    // hart runs. Every other reference to it is taken here, by the one hart that set `PRINTER` to
    // its own mark above and keeps it until the store below, so this is the only live reference:
    // a print or a panic inside `f`, on this hart, finds its own mark and takes none.
    let result = unsafe { &mut *(&raw mut OUTPUT) }.as_mut().map(f);
    PRINTER.store(0, Ordering::Release);
    result
}

/// Write `args` to the console, if it is set up (`print!`). While a process holds the console
/// the line waits, whole, for it to give the hold back (kernel/devices.md, "The console's one
/// writer"); with no room left to wait in, it goes out now.
pub fn print(args: fmt::Arguments) {
    use fmt::Write;
    let printed = with_output(|stream| match stream.hold.line(|w| w.write_fmt(args)) {
        Line::Queued => {}
        // No room left to wait in: what waited goes out first, then this line.
        Line::Full => {
            stream.flush("");
            stream.write_fmt(args).unwrap();
        }
        Line::Free => stream.write_fmt(args).unwrap(),
    });
    // A print inside this one (a `Display` that prints) takes the stateless console instead.
    if printed.is_none() && PRINTER.load(Ordering::Relaxed) == me() {
        print_stateless(args);
    }
}

/// `console_hold`'s console side, for `pid`, once the kernel has checked its handle is the
/// console's: taking the hold, or giving it back and printing the kernel's lines that waited, so
/// they come before the holder's next write. With no console set up, nothing waits, and the hold
/// is nothing.
pub fn hold(pid: u32, take: bool) -> Result<(), Busy> {
    with_output(|stream| {
        if take {
            stream.hold.take(pid)
        } else {
            if stream.hold.release(pid) {
                stream.flush("");
            }
            Ok(())
        }
    })
    .unwrap_or(Ok(()))
}

/// The machine is stopping (`system_reset`): the hold ends, whoever has it, and the kernel's
/// lines that waited go out now, so neither they nor the lines printed after them are lost.
pub fn flush_held() {
    with_output(|stream| {
        if let Some(holder) = stream.hold.holder() {
            stream.hold.died(holder);
        }
        stream.flush("\r\n");
    });
}

/// `pid` has died: if it held the console it was cut off inside a write, so its line is ended
/// before the kernel's lines that waited for it go out.
pub fn died(pid: u32) {
    with_output(|stream| {
        if stream.hold.died(pid) {
            stream.flush("\r\n");
        }
    });
}

/// Write the panic handler's `args`. A panic that struck inside `print` must not take `OUTPUT`
/// while that print's reference is live, so it writes to the stateless console, and says so.
pub fn print_panic(args: fmt::Arguments) {
    use fmt::Write;
    if PRINTER.load(Ordering::Relaxed) == me() {
        print_stateless(format_args!("\r\n(while printing) {}\r\n", args));
    } else {
        // Never queued behind a holder: what waited goes out first, then the panic.
        with_output(|stream| {
            stream.flush("\r\n");
            let _ = stream.write_fmt(format_args!("{}\r\n", args));
        });
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
