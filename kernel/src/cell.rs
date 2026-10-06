// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel global state, and the one lock that guards it on every hart.

use core::sync::atomic::{AtomicU32, Ordering};

/// A global that the kernel may mutate: the replacement for `static mut`.
///
/// # One big lock, and a run-time-checked cell under it
///
/// Every hart runs the kernel, but only one at a time: a hart holds [`KERNEL_LOCK`] from its trap
/// entry, once the interrupted registers are saved, to just before its `sret` to user mode or its
/// idle `wfi` (kernel/scheduling.md, R78). So a `KernelCell` is reached by one thread of execution
/// at a time, and a checked build asserts that the hart reaching it holds the lock.
///
/// Inside one kernel section the kernel is not *truly re-entrant*: a nested borrow would be a
/// logic bug (fetching a global twice in one call chain), not a legitimate need. Genuine
/// re-entrancy -- holding a live `&mut` and needing another to the same data -- is undefined
/// behaviour in Rust and has no sound solution at any layer; the one place the kernel re-enters
/// itself on purpose is the swapper, which is handled explicitly, not through a shared cell. So
/// the cell's job is to catch that logic bug, not to permit aliasing: borrows are checked at run
/// time, and an accidental re-entrant access is a clean panic rather than two live `&mut` to the
/// same data.
///
/// The intended multi-hart form is a compile-time one: a branded token owned by the lock's guard
/// (GhostCell / `qcell::LCell`), so that every global is a token-guarded cell, reaching one
/// without the lock does not compile, and a lock order exists if the lock is ever split. It is
/// deferred until the lock's hold time and contention are measured, not rejected.
pub struct KernelCell<T>(core::cell::RefCell<T>);

// SAFETY: a `KernelCell` is reached only by the hart holding `KERNEL_LOCK` (taken at every trap
// entry from user mode and held by `kmain`; a checked build asserts it in `with`), and the lock's
// Acquire and Release order every access by one hart before the next hart's. So there is never a
// second thread of execution observing the `RefCell`; it takes care of re-entrancy on the one
// there is.
unsafe impl<T> Sync for KernelCell<T> {}

impl<T> KernelCell<T> {
    pub const fn new(value: T) -> Self { KernelCell(core::cell::RefCell::new(value)) }

    /// Exclusive access for the duration of `f`. Panics if the cell is already borrowed, and in a
    /// checked build if this hart does not hold the kernel lock.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        #[cfg(debug_assertions)]
        assert!(KERNEL_LOCK.held_here(), "a KernelCell reached without the kernel lock");
        f(&mut self.0.borrow_mut())
    }
}

/// The big kernel lock: one for every global, so no two locks nest and none needs an order.
pub static KERNEL_LOCK: TicketLock = TicketLock::new();

/// A ticket lock, so kernel entry is FIFO (R78): a hart that has drawn a ticket is passed by no
/// other hart twice, so it waits behind at most `MAX_HARTS` - 1 kernel sections. A test-and-set
/// lock is unfair, and could starve a hart of kernel entry for ever, and with it the budget running
/// there. It is **not reentrant**: a second acquire by the hart that holds it waits for ever.
///
/// The test-only `sched-test-and-set-entry` swaps in test-and-set, which `smp-boot`'s FIFO check
/// must catch. A hart waiting for it serves any shootdown asked of it on every turn
/// (`arch::hart::serve`), so the holder's wait for its acknowledgement cannot deadlock.
pub struct TicketLock {
    /// The next ticket to draw.
    next: AtomicU32,
    /// The ticket being served; under test-and-set, the count of sections served.
    serving: AtomicU32,
    /// Test-and-set only: 1 while held.
    #[cfg(feature = "sched-test-and-set-entry")]
    taken: AtomicU32,
    /// A checked build's record of the holder: its hart index plus one, 0 for none.
    #[cfg(debug_assertions)]
    holder: AtomicU32,
    /// A checked build's FIFO evidence: the most sections any acquisition waited behind, counted
    /// from its draw (`smp-boot` fails if it exceeds the harts less one).
    #[cfg(debug_assertions)]
    most_waited: AtomicU32,
}

impl TicketLock {
    pub const fn new() -> Self {
        TicketLock {
            next: AtomicU32::new(0),
            serving: AtomicU32::new(0),
            #[cfg(feature = "sched-test-and-set-entry")]
            taken: AtomicU32::new(0),
            #[cfg(debug_assertions)]
            holder: AtomicU32::new(0),
            #[cfg(debug_assertions)]
            most_waited: AtomicU32::new(0),
        }
    }

    /// Take the lock, waiting in turn.
    pub fn acquire(&self) {
        #[cfg(not(feature = "sched-test-and-set-entry"))]
        let waited = {
            let ticket = self.next.fetch_add(1, DRAW);
            // The checked build's count: the tickets ahead, read after the draw. The draw, this
            // read and every release are sequentially consistent there, so the read sees `serving`
            // at or after the draw: it can count fewer sections than were ahead, never more, and a
            // count above the bound is a real one.
            let waited = ticket.wrapping_sub(self.serving.load(DRAW));
            while self.serving.load(Ordering::Acquire) != ticket {
                crate::arch::hart::serve();
                wait_for_change(&self.serving);
            }
            waited
        };
        #[cfg(feature = "sched-test-and-set-entry")]
        let waited = {
            let drawn = self.serving.load(Ordering::Relaxed);
            while self.taken.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
                crate::arch::hart::serve();
                pause();
            }
            self.serving.load(Ordering::Relaxed).wrapping_sub(drawn)
        };
        #[cfg(debug_assertions)]
        {
            self.holder.store(crate::arch::hart::index() as u32 + 1, Ordering::Relaxed);
            self.most_waited.fetch_max(waited, Ordering::Relaxed);
            // R78: no hart is passed by another twice (`smp-boot`; the test-and-set mutation
            // must trip this).
            let harts = crate::arch::hart::started() as u32;
            assert!(
                waited < harts,
                "the kernel lock: waited behind {} sections, {} hart(s) (R78)",
                waited,
                harts
            );
        }
        #[cfg(not(debug_assertions))]
        let _ = waited;
    }

    /// Give the lock to the next ticket. The caller holds it.
    pub fn release(&self) {
        #[cfg(debug_assertions)]
        {
            assert!(self.held_here(), "the kernel lock released by a hart that does not hold it");
            self.holder.store(0, Ordering::Relaxed);
        }
        let serving = self.serving.load(Ordering::Relaxed);
        #[cfg(not(feature = "sched-test-and-set-entry"))]
        self.serving.store(serving.wrapping_add(1), SERVE);
        #[cfg(feature = "sched-test-and-set-entry")]
        {
            self.serving.store(serving.wrapping_add(1), Ordering::Relaxed);
            self.taken.store(0, Ordering::Release);
        }
    }

    /// Whether this hart holds the lock (a checked build's record).
    #[cfg(debug_assertions)]
    pub fn held_here(&self) -> bool {
        self.holder.load(Ordering::Relaxed) == crate::arch::hart::index() as u32 + 1
    }

    /// The most kernel sections any acquisition has waited behind (a checked build's record).
    #[cfg(debug_assertions)]
    pub fn most_waited(&self) -> u32 { self.most_waited.load(Ordering::Relaxed) }
}

/// The draw's ordering: Relaxed, as the lock needs; sequentially consistent in a checked build,
/// for its FIFO count ([`TicketLock::acquire`]).
const DRAW: Ordering = if cfg!(debug_assertions) { Ordering::SeqCst } else { Ordering::Relaxed };
/// The release's ordering: Release, as the lock needs; sequentially consistent in a checked build.
const SERVE: Ordering = if cfg!(debug_assertions) { Ordering::SeqCst } else { Ordering::Release };

/// One turn of a spin: the pause hint (`zihintpause`; a FENCE hint, which a core without the
/// extension runs as a no-op), which on a core whose harts share issue slots gives them to the
/// sibling.
#[inline(always)]
pub fn pause() { core::hint::spin_loop(); }

/// One turn of the wait for the lock word `word` to change: the Zawrs hook. On a core with Zawrs
/// this becomes a `wrs.nto` reservation wait on `word`, which stalls the hart until another hart
/// writes it; none is emitted now, so it is one [`pause`].
#[inline(always)]
fn wait_for_change(word: &AtomicU32) {
    let _ = word;
    pause();
}
