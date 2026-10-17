//! The console queue (servers/steward.md, "Failure and restart"): the lines the steward says,
//! written in the order they were said through one fid kept open at `consoled` for the steward's
//! life, a line one write within the steward's own send bound. A console that is busy costs the
//! loop one bound per turn, never a round trip per line inside an event's step, and never the
//! attach, open and close a fresh connection per line used to cost.
//!
//! A turn ([`Queue::run`]) opens the fid if it is not open, then writes the lines queued, in
//! order, up to the first the console did not take within the bound (`Timeout`): that line is
//! tried again no sooner than [`RETRY_US`] later, up to [`TRIES`] attempts, then dropped, and the
//! turn ends with the lines behind it still queued, so the loop receives once between two waits.
//! Any other failure (the console refused the write, or died: a restarted `consoled` knows none
//! of its clients' connections, servers/init.md "Residual risks") closes the fid and drops every
//! line queued; the next turn opens the fid again. A line is never dropped silently: the lines
//! dropped are counted, and the first line written once the console answers again says the
//! count. A write the console took but did not answer within the bound is written again, so a
//! line may appear twice; it does not vanish.
//!
//! The queue's bound is the lines a turn raises: an event says at most two lines (an audit
//! record and the usage line of the one domain it changed; a login or a lease's start, end or
//! exit each one record), a call's time one, a cleanup step's drop one, and a turn raises one
//! event and the steps due. The bound is argued, not enforced: the queue has no cap; it drains
//! at every turn the console answers, and a console that refuses drops what it is offered.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;

use redoubt_rt::abi::Error;

pub use crate::cleanup::{RETRY_US, TRIES};

/// The console's side: a fid opened once and written through, stood in for by host tests.
pub trait Sink {
    /// Opens the fid the lines are written through, on the kept connection.
    fn open(&mut self) -> Result<(), Error>;
    /// One write of `bytes` on the fid: how many the console took.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error>;
}

/// The lines not yet written, in the order they were said.
#[derive(Default)]
pub struct Queue {
    lines: VecDeque<String>,
    /// How much of the front line is written: a line longer than the lend's `iounit` goes in
    /// several writes.
    done: usize,
    /// Attempts at the front line the console did not take, and when the next may run.
    tries: u8,
    due: u64,
    open: bool,
    /// Lines dropped since the console last took a line: said first when it answers again.
    dropped: u64,
}

impl Queue {
    /// Queues `line`, due at once.
    pub fn push(&mut self, line: String) { self.lines.push_back(line); }

    /// The lines waiting.
    pub fn len(&self) -> usize { self.lines.len() }

    pub fn is_empty(&self) -> bool { self.lines.is_empty() }

    /// The lines dropped and not yet said.
    pub fn dropped(&self) -> u64 { self.dropped }

    /// A turn at `now`: the lines due, written in order through `sink` up to the first the
    /// console did not take. Returns when the next attempt may run: `None` with nothing left to
    /// write, `now` when the turn stopped short with lines due.
    pub fn run<S: Sink>(&mut self, now: u64, sink: &mut S) -> Option<u64> {
        if self.lines.is_empty() && self.dropped == 0 {
            return None;
        }
        if self.due > now {
            return Some(self.due);
        }
        self.write(now, sink)
    }

    /// The last turn, at the steward's exit: every line, whatever is due, up to the first the
    /// console did not take.
    pub fn flush<S: Sink>(&mut self, sink: &mut S) {
        self.due = 0;
        let _ = self.write(0, sink);
    }

    fn write<S: Sink>(&mut self, now: u64, sink: &mut S) -> Option<u64> {
        if !self.open {
            match sink.open() {
                Ok(()) => self.open = true,
                Err(Error::Timeout) => return self.timed_out(now),
                Err(_) => return self.refused(now),
            }
        }
        if self.dropped > 0 {
            let notice =
                format!("steward: {} console lines dropped: the console did not take them\n", self.dropped);
            match sink.write(notice.as_bytes()) {
                Ok(_) => self.dropped = 0,
                Err(Error::Timeout) => return self.timed_out(now),
                Err(_) => return self.refused(now),
            }
        }
        while let Some(line) = self.lines.front() {
            match sink.write(&line.as_bytes()[self.done..]) {
                Ok(n) if n > 0 => {
                    self.done += n;
                    if self.done >= line.len() {
                        self.lines.pop_front();
                        self.done = 0;
                        self.tries = 0;
                    }
                }
                Ok(_) | Err(Error::Timeout) => return self.timed_out(now),
                Err(_) => return self.refused(now),
            }
        }
        None
    }

    /// The console did not take the front line (or the dropped lines' notice) in time: it waits
    /// [`RETRY_US`], and after [`TRIES`] attempts the line is dropped and the next is due now.
    fn timed_out(&mut self, now: u64) -> Option<u64> {
        self.tries += 1;
        if self.tries < TRIES {
            self.due = now.saturating_add(RETRY_US);
            return Some(self.due);
        }
        if self.lines.pop_front().is_some() {
            self.dropped += 1;
        }
        self.done = 0;
        self.tries = 0;
        self.due = now;
        Some(now)
    }

    /// The console refused or is gone: the fid is closed, every line queued is dropped and
    /// counted, and the next turn opens the fid again no sooner than [`RETRY_US`] later.
    fn refused(&mut self, now: u64) -> Option<u64> {
        self.dropped += self.lines.len() as u64;
        self.lines.clear();
        self.open = false;
        self.done = 0;
        self.tries = 0;
        self.due = now.saturating_add(RETRY_US);
        Some(self.due)
    }
}
