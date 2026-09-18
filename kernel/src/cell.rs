// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel global state.

/// A global that the kernel may mutate: the replacement for `static mut`.
///
/// On bare metal, borrows are checked at run time, so an accidental re-entrant access is
/// a clean panic rather than two live `&mut` to the same data. When the kernel becomes
/// multi-hart, this type becomes a spinlock, and every global that goes through it is
/// covered at once. In hosted mode the kernel is an ordinary multi-threaded program, and
/// this is a mutex.
pub struct KernelCell<T>(Inner<T>);

#[cfg(baremetal)]
type Inner<T> = core::cell::RefCell<T>;
#[cfg(not(baremetal))]
type Inner<T> = std::sync::Mutex<T>;

// SAFETY: the bare-metal kernel runs on a single hart with interrupts disabled whenever
// it executes (`sstatus.SIE` is only set in the idle loop, which holds no borrow). So
// there is never a second thread of execution that could observe a `KernelCell`.
// `RefCell` takes care of re-entrancy on the one thread there is.
#[cfg(baremetal)]
unsafe impl<T> Sync for KernelCell<T> {}

impl<T> KernelCell<T> {
    pub const fn new(value: T) -> Self { KernelCell(Inner::new(value)) }

    /// Exclusive access for the duration of `f`. Panics if the cell is already borrowed
    /// (bare metal); blocks if another thread holds it (hosted).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        #[cfg(baremetal)]
        let mut guard = self.0.borrow_mut();
        #[cfg(not(baremetal))]
        let mut guard = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&mut guard)
    }
}
