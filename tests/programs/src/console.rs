//! The console UART, for the program that owns it: the bundle's first (docs/testbench.md,
//! rule F), which holds the console's device handles. Its threads share it, one whole line at a
//! time.
//!
//! Attack cases take their verdict from lines no other program can write. So the only way to
//! print another program's bytes is [`relay`], which starts every line with the sender's name as
//! the kernel's badge gives it; the owner's own lines are `logsrv::Line`'s closed set of
//! templates.

use core::cell::UnsafeCell;
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

use uart_16550::MmioSerialPort;

use crate::logsrv::Sender;

struct Port {
    busy: AtomicBool,
    port: UnsafeCell<Option<MmioSerialPort>>,
}

// SAFETY: `port` is only reached through `locked`, which holds `busy` for as long as the reference
// lives, so no two threads ever hold it at once.
unsafe impl Sync for Port {}

static PORT: Port = Port { busy: AtomicBool::new(false), port: UnsafeCell::new(None) };

fn locked<R>(f: impl FnOnce(&mut Option<MmioSerialPort>) -> R) -> R {
    while PORT.busy.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    // SAFETY: `busy` is held until the store below, so this is the only reference.
    let result = f(unsafe { &mut *PORT.port.get() });
    PORT.busy.store(false, Ordering::Release);
    result
}

fn with<R>(f: impl FnOnce(&mut MmioSerialPort) -> R) -> R {
    locked(|port| f(port.as_mut().expect("console::init first")))
}

/// Take the UART mapped at `base` (the console's MMIO handle, mapped by the caller).
pub fn init(base: usize) {
    // SAFETY: `base` is the UART's register page, mapped for this process by the kernel.
    let mut port = unsafe { MmioSerialPort::new(base) };
    port.init();
    // `init` leaves a byte of its own on the wire: a newline puts it on a line of its own, so the
    // first real line matches an anchored pattern.
    writeln!(port).ok();
    locked(|slot| *slot = Some(port));
}

/// One line of the owner's own (`logsrv::Line`).
pub(crate) fn line(args: fmt::Arguments) {
    with(|port| {
        port.write_fmt(args).ok();
        writeln!(port).ok();
    });
}

/// The one way to print another program's bytes: each line starts with its [`Sender`] in
/// brackets, `[pid N] ` or `[badge N] `. A sender cannot choose N, and every newline in its text
/// starts a new prefixed line, so it can never begin a line of its own. Control characters
/// become '?' only to keep the log readable.
pub(crate) fn relay(badge: u64, text: &str) {
    for line in text.split('\n') {
        with(|port| {
            write!(port, "[{}] ", Sender(badge)).ok();
            for c in line.chars() {
                port.write_char(if c.is_control() && c != '\t' { '?' } else { c }).ok();
            }
            writeln!(port).ok();
        });
    }
}

/// Whatever arrived on the UART.
pub fn receive() -> Option<u8> { with(|port| port.try_receive().ok()) }
