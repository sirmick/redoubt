//! The console's one writer (kernel/devices.md, "The console's one writer").
//!
//! The kernel's lines and a program's go to one UART: the kernel's through the firmware, a
//! program's through the device's registers. On several harts they would interleave byte by byte.
//! So the process that writes the UART takes the console's hold around each write, and while it
//! holds it the kernel's lines wait here, whole, and go out when it gives the hold back, before
//! its next write.
//!
//! - **The kernel never waits for user mode.** A line with nobody holding goes out at once. A line that does
//!   not fit what is queued goes out at once too, after everything queued ([`Line::Full`]), inside the
//!   holder's write: a holder that keeps the hold costs the kernel's lines their place between the holder's
//!   writes, at most a queue's worth at a time, never their delivery and never a wait.
//! - **One holder.** Another process asking is [`Busy`]. The holder taking it again, or giving back a hold it
//!   does not have, changes nothing.
//! - **A holder that dies** holding it was cut off inside its write: the kernel ends that line before its own
//!   ([`Hold::died`]).
//!
//! This crate keeps the rules and the queue, so they can be host-tested (`src/tests.rs`). Its
//! lock, the firmware's writes and the handle check are the kernel's (`debug/console.rs`,
//! `device.rs`).

#![no_std]
#![forbid(unsafe_code)]

use core::fmt;

#[cfg(test)]
mod tests;

/// Another process holds the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy;

/// Where a kernel line went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// Nobody holds the console: print it now.
    Free,
    /// Queued whole: it goes out when the hold is given back.
    Queued,
    /// Held, and the queue has no room for it: print what is queued, then it, now, and
    /// [`Hold::clear`] the queue (counted in [`Hold::spilled`]).
    Full,
}

/// The console's hold and the kernel's lines waiting on it. `N` bytes of lines at most.
pub struct Hold<const N: usize> {
    /// The holder's PID; 0 is nobody (no process has PID 0).
    holder: u32,
    queue: [u8; N],
    len: usize,
    spilled: u64,
}

impl<const N: usize> Default for Hold<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> Hold<N> {
    pub const fn new() -> Self { Hold { holder: 0, queue: [0; N], len: 0, spilled: 0 } }

    /// Who holds the console.
    pub fn holder(&self) -> Option<u32> { (self.holder != 0).then_some(self.holder) }

    /// `pid` takes the hold. Its own again is no change; another's is [`Busy`]. Nothing is
    /// queued while nobody holds it, so there is nothing to print first.
    pub fn take(&mut self, pid: u32) -> Result<(), Busy> {
        match self.holder {
            0 => {
                self.holder = pid;
                Ok(())
            }
            h if h == pid => Ok(()),
            _ => Err(Busy),
        }
    }

    /// `pid` gives the hold back. True if it held it: then the caller prints [`Hold::queued`]
    /// and [`Hold::clear`]s it, before the holder's next write. A hold it does not have is no
    /// change.
    pub fn release(&mut self, pid: u32) -> bool {
        if pid == 0 || self.holder != pid {
            return false;
        }
        self.holder = 0;
        true
    }

    /// `pid` has died. True if it held the console: it was inside a write, so the caller ends
    /// that line (`\r\n`) before it prints what is queued.
    pub fn died(&mut self, pid: u32) -> bool { self.release(pid) }

    /// A kernel line, written by `write`. Queued whole while the console is held; otherwise the
    /// caller prints it now, and if it does not fit, everything queued before it first. A line
    /// that does not fit leaves nothing of itself in the queue.
    pub fn line(&mut self, write: impl FnOnce(&mut dyn fmt::Write) -> fmt::Result) -> Line {
        if self.holder == 0 {
            return Line::Free;
        }
        let start = self.len;
        let mut tail = Tail { queue: &mut self.queue, len: &mut self.len };
        if write(&mut tail).is_err() {
            self.len = start;
            self.spilled += 1;
            return Line::Full;
        }
        Line::Queued
    }

    /// The kernel's lines waiting, oldest first.
    pub fn queued(&self) -> &[u8] { &self.queue[..self.len] }

    /// Forgets the queued lines, once printed.
    pub fn clear(&mut self) { self.len = 0; }

    /// How many times the queue was full, so its lines went out inside a holder's write.
    pub fn spilled(&self) -> u64 { self.spilled }
}

/// The free end of the queue, as a writer that refuses what does not fit.
struct Tail<'q> {
    queue: &'q mut [u8],
    len: &'q mut usize,
}

impl fmt::Write for Tail<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.queue.get_mut(*self.len..end).ok_or(fmt::Error)?.copy_from_slice(s.as_bytes());
        *self.len = end;
        Ok(())
    }
}
