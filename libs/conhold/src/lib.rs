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
//! - **A holder that dies** holding it was cut off inside its write, and one that dies with its line left
//!   unfinished leaves it open: the kernel ends that line before its own, and owes its PID nothing after
//!   ([`Hold::died`]).
//! - **A line left open between writes.** A holder's line can span its writes, and between them nobody holds.
//!   So a holder whose line is unfinished says so as it gives the hold back ([`Hold::release`]); a kernel
//!   line that then finds nobody holding ends that line first ([`Line::Ending`]), as do lines that waited and
//!   go out at that give-back, and that holder's next take, and only its, answers that the kernel wrote
//!   ([`Hold::take`]), so it starts its line again.
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
    /// Nobody holds the console, and the holder's last line is unfinished: print `\r\n`, then it,
    /// now. That ends the line, and the holder's next take says the kernel wrote.
    Ending,
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
    /// The PID whose last line is unfinished, or 0: set by a release that says so, cleared by one
    /// that does not and by the kernel's own `\r\n` ([`Line::Ending`], or a release that prints
    /// waiting lines).
    open_by: u32,
    /// The PID whose unfinished line a kernel line ended, or 0: cleared only by that PID's take,
    /// which it answers. Another process's take is answered false and changes nothing.
    wrote_for: u32,
}

impl<const N: usize> Default for Hold<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> Hold<N> {
    pub const fn new() -> Self {
        Hold { holder: 0, queue: [0; N], len: 0, spilled: 0, open_by: 0, wrote_for: 0 }
    }

    /// Who holds the console.
    pub fn holder(&self) -> Option<u32> { (self.holder != 0).then_some(self.holder) }

    /// `pid` takes the hold. The answer says whether a kernel line ended `pid`'s own unfinished
    /// line since it last gave the hold back, and only this answer clears that; another's
    /// unfinished line is not its to hear of. Its own again is no change; another's is [`Busy`].
    /// Nothing is queued while nobody holds it, so there is nothing to print first.
    pub fn take(&mut self, pid: u32) -> Result<bool, Busy> {
        match self.holder {
            0 => self.holder = pid,
            h if h == pid => {}
            _ => return Err(Busy),
        }
        let wrote = self.wrote_for == pid;
        if wrote {
            self.wrote_for = 0;
        }
        Ok(wrote)
    }

    /// `pid` gives the hold back, saying with `open` whether its last line is unfinished.
    /// `Some(end)` if it held it: then the caller prints [`Hold::queued`] and [`Hold::clear`]s it,
    /// before the holder's next write, after `\r\n` if `end`: lines that waited while the line
    /// was left unfinished end it, as a free line would, and the holder's next take is told so.
    /// A hold it does not have is no change, `None`.
    pub fn release(&mut self, pid: u32, open: bool) -> Option<bool> {
        if pid == 0 || self.holder != pid {
            return None;
        }
        self.holder = 0;
        let end = open && self.len > 0;
        self.open_by = if open && !end { pid } else { 0 };
        if end {
            self.wrote_for = pid;
        }
        Some(end)
    }

    /// `pid` has died. True if it held the console (it was inside a write) or its line was
    /// unfinished: the caller ends that line (`\r\n`), then prints what is queued. Nothing is owed
    /// to its PID after: neither an ending nor an answer, which a later process given the PID
    /// must not be told.
    pub fn died(&mut self, pid: u32) -> bool {
        let open = pid != 0 && self.open_by == pid;
        if open {
            self.open_by = 0;
        }
        if pid != 0 && self.wrote_for == pid {
            self.wrote_for = 0;
        }
        self.release(pid, false).is_some() || open
    }

    /// A kernel line, written by `write`. Queued whole while the console is held; otherwise the
    /// caller prints it now, after `\r\n` if a holder's last line is unfinished ([`Line::Ending`],
    /// once: the line is then finished, and that holder's next take is told), and if it does not
    /// fit, everything queued before it first. A line that does not fit leaves nothing of itself
    /// in the queue.
    pub fn line(&mut self, write: impl FnOnce(&mut dyn fmt::Write) -> fmt::Result) -> Line {
        if self.holder == 0 {
            if self.open_by != 0 {
                self.wrote_for = core::mem::take(&mut self.open_by);
                return Line::Ending;
            }
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
