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
/// becomes a spinlock and every global it guards is covered at once.
///
/// Borrows are checked at run time, so an accidental re-entrant access is a clean panic
/// rather than two live `&mut` to the same data. With `smp`, this type is a spinlock, and
/// every global that goes through it is covered at once.
pub struct KernelCell<T>(Inner<T>);

#[cfg(not(feature = "smp"))]
type Inner<T> = core::cell::RefCell<T>;
#[cfg(feature = "smp")]
type Inner<T> = SpinLock<T>;

/// A minimal test-and-set spinlock: the multi-hart form of a `KernelCell` guard. It is
/// **not reentrant** -- a second acquire, even by the hart that holds it, spins forever --
/// which matches the single-big-lock discipline (one acquire per kernel entry, never
/// nested).
#[cfg(feature = "smp")]
pub struct SpinLock<T> {
    locked: core::sync::atomic::AtomicBool,
    value: core::cell::UnsafeCell<T>,
}

// SAFETY: `with` is the only way to reach `value`, and it holds `locked` throughout, so at
// most one thread of execution ever has access at a time.
#[cfg(feature = "smp")]
unsafe impl<T: Send> Sync for SpinLock<T> {}

#[cfg(feature = "smp")]
impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        SpinLock { locked: core::sync::atomic::AtomicBool::new(false), value: core::cell::UnsafeCell::new(value) }
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
unsafe impl<T> Sync for KernelCell<T> {}

impl<T> KernelCell<T> {
    pub const fn new(value: T) -> Self { KernelCell(Inner::new(value)) }

    /// Exclusive access for the duration of `f`. Panics if the cell is already borrowed;
    /// with `smp`, spins while another hart holds it.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        #[cfg(not(feature = "smp"))]
        {
            f(&mut self.0.borrow_mut())
        }
        #[cfg(feature = "smp")]
        {
            self.0.with(f)
        }
    }
}

