//! The one lock type, and what may be shared between schedulers.
//!
//! With the `std` feature the VM may run several schedulers on threads (DESIGN.md, "Terms and
//! heaps", stage 2): [`Lock`] is a mutex and shared values must be `Send + Sync`. Without it
//! there is one scheduler: [`Lock`] is a `RefCell` and nothing needs to cross threads. Code
//! is written once against this module and is correct in both.

#[cfg(feature = "std")]
mod imp {
    extern crate std;

    /// Exclusive access to a `T` for the holder of the guard.
    pub struct Lock<T>(std::sync::Mutex<T>);

    impl<T> Lock<T> {
        pub const fn new(value: T) -> Lock<T> {
            Lock(std::sync::Mutex::new(value))
        }

        /// Wait for exclusive access. A panic while holding a lock does not poison it: the VM
        /// never panics on purpose, and a value left half-changed is no worse than losing it.
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    /// Values that may be shared between schedulers.
    pub trait Shared: Send + Sync {}
    impl<T: Send + Sync + ?Sized> Shared for T {}

    /// A value of any type that may be shared between schedulers (a resource's value).
    pub type AnyShared = dyn core::any::Any + Send + Sync;
}

#[cfg(not(feature = "std"))]
mod imp {
    /// Exclusive access to a `T` for the holder of the guard.
    pub struct Lock<T>(core::cell::RefCell<T>);

    impl<T> Lock<T> {
        pub const fn new(value: T) -> Lock<T> {
            Lock(core::cell::RefCell::new(value))
        }

        /// Exclusive access. With one scheduler nothing else can hold it, unless this code
        /// already does: that is a bug, and panics.
        pub fn lock(&self) -> core::cell::RefMut<'_, T> {
            self.0.borrow_mut()
        }
    }

    /// Values that may be shared between schedulers: with one scheduler, any.
    pub trait Shared {}
    impl<T: ?Sized> Shared for T {}

    /// A value of any type (a resource's value).
    pub type AnyShared = dyn core::any::Any;
}

pub use imp::{AnyShared, Lock, Shared};

impl<T: Default> Default for Lock<T> {
    fn default() -> Lock<T> {
        Lock::new(T::default())
    }
}

/// What several schedulers will share must be `Sync`; what moves between them, `Send`.
#[cfg(feature = "std")]
#[allow(dead_code)]
fn assert_thread_safe() {
    fn shared<T: Send + Sync>() {}
    fn moves<T: Send>() {}
    shared::<crate::term::Resource>();
    shared::<crate::term::OwnedTerm>();
    shared::<crate::term::Literals>();
    shared::<crate::module::Module>();
    moves::<crate::term::Heap>();
    moves::<crate::process::Process>();
}
