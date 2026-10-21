//! The kernel lock's waits (kernel/scheduling.md, R78): which hart a release wakes.
//!
//! The kernel lock is a ticket lock, and a hart waiting for its turn halts. A release serves one
//! ticket, so it wakes one hart: the one whose ticket is now served, if it is halted. Every other
//! waiter's turn has not come, and waking it would only make it halt again.
//!
//! - **A waiter's turn** ([`Waits::wait`]) records its ticket, marks itself, reads `serving` again, and halts
//!   only if its turn has still not come; woken, it clears its mark.
//! - **A release** ([`Waits::release`]) advances `serving`, then reads the marks, and names the marked hart
//!   whose ticket is the one now served.
//!
//! Every access is sequentially consistent, so the two sides' accesses fall in one order: either
//! the release's read of the marks comes after the waiter's mark, and it sees the mark and the
//! ticket recorded before it, or it comes before, and then the waiter's read of `serving`, after
//! its mark, comes after the release's advance and sees its turn. No hart halts past its turn.
//! The host test (`src/tests.rs`) runs the two sides over every interleaving of those accesses.
//!
//! The halt, the interrupt and the hart's index are the kernel's (`cell.rs`, `arch::hart`).

#![no_std]
#![forbid(unsafe_code)]

use core::sync::atomic::{AtomicU32, Ordering::SeqCst};

#[cfg(test)]
mod tests;

/// The waits on one ticket lock of up to `N` harts, a bit each in a 32-bit word.
pub struct Waits<const N: usize> {
    /// The harts marked waiting for their turn, a bit each by index.
    halted: AtomicU32,
    /// Each hart's ticket while it waits; stale otherwise, and read only beside its mark.
    tickets: [AtomicU32; N],
}

impl<const N: usize> Default for Waits<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> Waits<N> {
    pub const fn new() -> Self {
        assert!(N <= 32, "a bit each in a u32");
        Waits { halted: AtomicU32::new(0), tickets: [const { AtomicU32::new(0) }; N] }
    }

    /// One turn of hart `hart`'s wait for `ticket` while `serving` has not reached it: record the
    /// ticket, mark the hart, read `serving` again, and `halt` unless the turn came meanwhile;
    /// then clear the mark. The caller reads `serving` again after.
    pub fn wait(&self, hart: usize, ticket: u32, serving: &AtomicU32, halt: impl FnOnce()) {
        self.wait_stepped(hart, ticket, serving, halt, &|| {})
    }

    /// Advance `serving` by one, the lock's release, which only its holder makes; the hart to wake:
    /// the marked hart whose ticket is now served, if any.
    pub fn release(&self, serving: &AtomicU32) -> Option<usize> { self.release_stepped(serving, &|| {}) }

    /// [`Self::wait`], with `step` before each shared access (the host test's interleaving).
    fn wait_stepped(
        &self,
        hart: usize,
        ticket: u32,
        serving: &AtomicU32,
        halt: impl FnOnce(),
        step: &impl Fn(),
    ) {
        #[cfg(feature = "read-before-mark")]
        let halts = {
            step();
            serving.load(SeqCst) != ticket
        };
        step();
        self.tickets[hart].store(ticket, SeqCst);
        step();
        self.halted.fetch_or(1 << hart, SeqCst);
        #[cfg(not(feature = "read-before-mark"))]
        let halts = {
            step();
            serving.load(SeqCst) != ticket
        };
        if halts {
            halt();
        }
        step();
        self.halted.fetch_and(!(1 << hart), SeqCst);
    }

    /// [`Self::release`], with `step` before each shared access.
    fn release_stepped(&self, serving: &AtomicU32, step: &impl Fn()) -> Option<usize> {
        step();
        let next = serving.load(SeqCst).wrapping_add(1);
        step();
        serving.store(next, SeqCst);
        step();
        let halted = self.halted.load(SeqCst);
        if cfg!(feature = "wake-none") {
            return None;
        }
        (0..N).filter(|h| halted & 1 << h != 0).find(|&hart| {
            step();
            self.tickets[hart].load(SeqCst) == next
        })
    }
}
