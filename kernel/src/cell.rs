// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel global state.

/// A global that the kernel may mutate: the replacement for `static mut`.
///
/// # Why a run-time-checked cell, and not GhostCell or raw pointers
///
/// The kernel is single-hart and runs with interrupts disabled, so there is never a
/// second thread of execution touching these globals. That means the kernel is not
/// *truly re-entrant*: a nested borrow would be a logic bug (fetching a global twice in
/// one call chain), not a legitimate need. Genuine re-entrancy -- holding a live `&mut`
/// and needing another to the same data -- is undefined behaviour in Rust and has no
/// sound solution at any layer; the one place the kernel re-enters itself on purpose is
/// the swapper, which is handled explicitly, not through a shared cell.
///
/// So the job here is to catch that logic bug, not to permit aliasing. A compile-time
/// alternative exists (GhostCell / `qcell::LCell`: a branded token carries the mutable
/// permission), but it gives nothing for the multi-hart future, where this type instead
/// becomes a spinlock and every global it guards is covered at once. The run-time check
/// also matches the `RefCell` the hosted build already uses for the same globals.
///
/// On bare metal, borrows are checked at run time, so an accidental re-entrant access is
/// a clean panic rather than two live `&mut` to the same data. When the kernel becomes
/// multi-hart, this type becomes a spinlock, and every global that goes through it is
/// covered at once. In hosted mode the kernel is an ordinary multi-threaded program, and
/// this is a mutex.
pub struct KernelCell<T>(Inner<T>);

#[cfg(all(baremetal, not(feature = "smp")))]
type Inner<T> = core::cell::RefCell<T>;
#[cfg(all(baremetal, feature = "smp"))]
type Inner<T> = SpinLock<T>;
#[cfg(not(baremetal))]
type Inner<T> = std::sync::Mutex<T>;

/// A minimal test-and-set spinlock: the multi-hart form of a `KernelCell` guard. It is
/// **not reentrant** -- a second acquire, even by the hart that holds it, spins forever --
/// which matches the single-big-lock discipline (one acquire per kernel entry, never
/// nested). Correctness is checked by the multi-threaded stress test below.
#[cfg(any(test, all(baremetal, feature = "smp")))]
pub struct SpinLock<T> {
    locked: core::sync::atomic::AtomicBool,
    value: core::cell::UnsafeCell<T>,
}

// SAFETY: `with` is the only way to reach `value`, and it holds `locked` throughout, so at
// most one thread of execution ever has access at a time.
#[cfg(any(test, all(baremetal, feature = "smp")))]
unsafe impl<T: Send> Sync for SpinLock<T> {}

#[cfg(any(test, all(baremetal, feature = "smp")))]
impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        SpinLock {
            locked: core::sync::atomic::AtomicBool::new(false),
            value: core::cell::UnsafeCell::new(value),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        use core::sync::atomic::Ordering;
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        // SAFETY: the lock is held, so this is the only live reference to `value`.
        let result = f(unsafe { &mut *self.value.get() });
        self.locked.store(false, Ordering::Release);
        result
    }
}

// SAFETY: the bare-metal kernel runs on a single hart with interrupts disabled whenever
// it executes (`sstatus.SIE` is only set in the idle loop, which holds no borrow). So
// there is never a second thread of execution that could observe a `KernelCell`.
// `RefCell` takes care of re-entrancy on the one thread there is.
#[cfg(baremetal)]
unsafe impl<T> Sync for KernelCell<T> {}

impl<T> KernelCell<T> {
    pub const fn new(value: T) -> Self {
        KernelCell(Inner::new(value))
    }

    /// Exclusive access for the duration of `f`. Panics if the cell is already borrowed
    /// (bare metal); blocks if another thread holds it (hosted).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        #[cfg(all(baremetal, not(feature = "smp")))]
        {
            f(&mut self.0.borrow_mut())
        }
        #[cfg(all(baremetal, feature = "smp"))]
        {
            self.0.with(f)
        }
        #[cfg(not(baremetal))]
        {
            f(&mut self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
        }
    }
}

#[cfg(test)]
mod spinlock_tests {
    use super::SpinLock;

    /// Stress the spinlock under real preemptive concurrency (host threads, real cores):
    /// no two threads may be in the critical section at once, and no increment may be lost.
    /// A broken lock (wrong ordering, a CAS bug) trips the `occupied` assert or the count.
    #[test]
    fn mutual_exclusion_and_no_lost_updates() {
        use std::sync::Arc;
        const THREADS: usize = 8;
        const ITERS: usize = 200_000;

        // (count, occupied): `occupied` must never be seen true on entry.
        let lock = Arc::new(SpinLock::new((0usize, false)));
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let lock = Arc::clone(&lock);
                std::thread::spawn(move || {
                    for _ in 0..ITERS {
                        lock.with(|(count, occupied)| {
                            assert!(!*occupied, "two threads in the critical section at once");
                            *occupied = true;
                            *count += 1;
                            *occupied = false;
                        });
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        lock.with(|(count, _)| assert_eq!(*count, THREADS * ITERS, "increments were lost"));
    }
}
