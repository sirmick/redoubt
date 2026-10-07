//! The terminal beamlet runs on: raw mode for as long as the VM reads the console, and the
//! terminal's own settings back on every way out (docs/userland/beamlet.md, "The console on a
//! host").
//!
//! Raw mode is entered when a process of the VM first reads the console and stdin is a tty: no
//! echo, no line discipline, no signal keys, so every byte typed reaches the VM as it is and the
//! shell's driver decides what Ctrl+C means. Output processing stays on, so a line written by
//! anything but the shell's encoder (OTP's logger, `standard_error`) still lands where a line
//! does on a host; the encoder ends its lines with CR LF itself, as it must on Redoubt.
//!
//! The settings come back by three ways: the [`Guard`] made at the start of `main` restores them
//! when `main` returns, however it returns; a panic hook restores them before the panic is
//! reported; and a handler for SIGINT, SIGTERM and SIGHUP restores them and lets the signal end
//! the process as it would have. Restoring is idempotent, so two of these may run.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::fd::{BorrowedFd, OwnedFd};
use rustix::termios::{self, InputModes, LocalModes, OptionalActions, SpecialCodeIndex, Termios};

/// The tty raw mode was entered on, and its settings before: set once, by [`enter`].
static ORIGINAL: OnceLock<(OwnedFd, Termios)> = OnceLock::new();
/// Whether the tty is in raw mode now.
static RAW: AtomicBool = AtomicBool::new(false);

/// Restores the terminal when dropped. Made at the start of `main`, so every return from it,
/// a result printed or an error, puts the terminal back.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) { restore(); }
}

/// Puts stdin in raw mode, if it is a tty and is not already, and arms the restoration on a
/// panic or a signal. Without a tty (input from a pipe, a test's runner) nothing changes.
pub fn enter() {
    let stdin = rustix::stdio::stdin();
    if termios::isatty(stdin) {
        enter_on(stdin);
    }
}

fn enter_on(fd: BorrowedFd) {
    let (Ok(original), Ok(own)) = (termios::tcgetattr(fd), fd.try_clone_to_owned()) else {
        return;
    };
    let mut raw = original.clone();
    if ORIGINAL.set((own, original)).is_err() {
        return;
    }
    raw.local_modes &= !(LocalModes::ICANON | LocalModes::ECHO | LocalModes::ISIG | LocalModes::IEXTEN);
    raw.input_modes &=
        !(InputModes::ICRNL | InputModes::IXON | InputModes::BRKINT | InputModes::INPCK | InputModes::ISTRIP);
    raw.special_codes[SpecialCodeIndex::VMIN] = 1;
    raw.special_codes[SpecialCodeIndex::VTIME] = 0;
    if termios::tcsetattr(fd, OptionalActions::Flush, &raw).is_ok() {
        RAW.store(true, Ordering::SeqCst);
        arm();
    }
}

/// Puts the terminal's own settings back, if raw mode was entered. Safe to call twice, and from
/// a signal handler: it takes no lock and allocates nothing.
pub fn restore() {
    if let Some((fd, original)) = ORIGINAL.get() {
        if RAW.swap(false, Ordering::SeqCst) {
            let _ = termios::tcsetattr(fd, OptionalActions::Flush, original);
        }
    }
}

/// The terminal's size as `(cols, rows)`, if stdout is a terminal that knows it.
pub fn size() -> Option<(u16, u16)> {
    let size = termios::tcgetwinsize(rustix::stdio::stdout()).ok()?;
    (size.ws_col > 0 && size.ws_row > 0).then_some((size.ws_col, size.ws_row))
}

/// Arms the panic hook and the signal handlers, once.
fn arm() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `sigaction` is given a zeroed, then filled, `sigaction` struct of libc's own
        // layout and a null pointer for the old action it may write; `sigemptyset` is given that
        // struct's mask. The handler installed runs only `restore`, which makes one `tcsetattr`
        // call (async-signal-safe) over a file descriptor and settings set before the handler was
        // armed and never changed after, and `raise`.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as *const () as usize;
            // The default disposition is back once the handler runs, so raising the signal again
            // ends the process as the signal would have.
            action.sa_flags = libc::SA_RESETHAND;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}

extern "C" fn on_signal(signal: libc::c_int) {
    restore();
    // SAFETY: `raise` takes a signal number and touches no memory of ours.
    unsafe {
        libc::raise(signal);
    }
}

#[cfg(test)]
mod tests {
    use rustix::fd::AsFd;

    use super::*;

    /// Open a pseudo-terminal and return its slave end, the terminal under test.
    fn pty() -> (OwnedFd, OwnedFd) {
        let master =
            rustix::pty::openpt(rustix::pty::OpenptFlags::RDWR | rustix::pty::OpenptFlags::NOCTTY).unwrap();
        rustix::pty::grantpt(&master).unwrap();
        rustix::pty::unlockpt(&master).unwrap();
        let name = rustix::pty::ptsname(&master, Vec::new()).unwrap();
        let slave = std::fs::OpenOptions::new().read(true).write(true).open(name.to_str().unwrap()).unwrap();
        (master, slave.into())
    }

    fn modes(fd: impl AsFd) -> (u32, u32, u32, u32) {
        let t = termios::tcgetattr(fd).unwrap();
        (t.input_modes.bits(), t.output_modes.bits(), t.control_modes.bits(), t.local_modes.bits())
    }

    /// A panic anywhere in beamlet, a scheduler's thread included, puts the terminal back before
    /// it is reported. The other ways out are `tests/tty.rs`'s, on the binary.
    #[test]
    fn a_panic_restores_the_terminal() {
        let (_master, slave) = pty();
        let before = modes(&slave);
        assert!(termios::tcgetattr(&slave).unwrap().local_modes.contains(LocalModes::ICANON));
        enter_on(slave.as_fd());
        let raw = termios::tcgetattr(&slave).unwrap();
        assert!(!raw.local_modes.contains(LocalModes::ICANON));
        assert!(!raw.local_modes.contains(LocalModes::ECHO));
        assert!(!raw.local_modes.contains(LocalModes::ISIG));
        assert!(raw.output_modes.contains(termios::OutputModes::OPOST));

        let _ = std::panic::catch_unwind(|| panic!("a thread of beamlet panicked"));

        assert_eq!(modes(&slave), before);
    }
}
