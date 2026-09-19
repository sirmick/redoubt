//! The one lock type, and what may be shared between schedulers.
//!
//! With the `std` feature the VM may run several schedulers on threads (DESIGN.md, "Terms and
//! heaps", stage 2): [`Lock`] is a mutex and shared values must be `Send + Sync`. Without it
//! there is one scheduler: [`Lock`] is a `RefCell` and nothing needs to cross threads. Code
//! is written once against this module and is correct in both.

#[cfg(feature = "std")]
mod imp {
    extern crate std;

    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Exclusive access to a `T` for the holder of the guard. Taking a lock this thread already
    /// holds is a bug in the VM: it panics (as the one-scheduler `RefCell` does) rather than
    /// deadlocking.
    pub struct Lock<T> {
        inner: std::sync::Mutex<T>,
        /// The thread holding it (see [`me`]), or 0.
        owner: AtomicUsize,
    }

    /// This thread, as a number: the address of a thread-local.
    fn me() -> usize {
        std::thread_local!(static ID: u8 = const { 0 });
        ID.with(|id| id as *const u8 as usize)
    }

    impl<T> Lock<T> {
        pub const fn new(value: T) -> Lock<T> {
            Lock {
                inner: std::sync::Mutex::new(value),
                owner: AtomicUsize::new(0),
            }
        }

        /// Wait for exclusive access. A panic while holding a lock does not poison it: the VM
        /// never panics on purpose, and a value left half-changed is no worse than losing it.
        pub fn lock(&self) -> Guard<'_, T> {
            let me = me();
            assert!(
                self.owner.load(Ordering::Relaxed) != me,
                "a lock taken twice by one thread"
            );
            let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            self.owner.store(me, Ordering::Relaxed);
            Guard {
                guard: Some(guard),
                owner: &self.owner,
            }
        }

        /// Access through an exclusive reference, which needs no locking.
        pub fn get_mut(&mut self) -> &mut T {
            self.inner.get_mut().unwrap_or_else(|e| e.into_inner())
        }
    }

    pub struct Guard<'a, T> {
        /// Always `Some`, except while a [`Wakeup::wait`] has released it.
        guard: Option<std::sync::MutexGuard<'a, T>>,
        owner: &'a AtomicUsize,
    }

    impl<T> core::ops::Deref for Guard<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            self.guard.as_ref().expect("held")
        }
    }

    impl<T> core::ops::DerefMut for Guard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            self.guard.as_mut().expect("held")
        }
    }

    impl<T> Drop for Guard<'_, T> {
        fn drop(&mut self) {
            self.owner.store(0, Ordering::Relaxed);
        }
    }

    /// Where idle schedulers wait for work.
    #[derive(Default)]
    pub struct Wakeup(std::sync::Condvar);

    impl Wakeup {
        /// Release `guard`, wait to be woken, and lock again.
        pub fn wait<'a, T>(&self, mut guard: Guard<'a, T>) -> Guard<'a, T> {
            let inner = guard.guard.take().expect("held");
            guard.owner.store(0, Ordering::Relaxed);
            let inner = self.0.wait(inner).unwrap_or_else(|e| e.into_inner());
            guard.owner.store(me(), Ordering::Relaxed);
            guard.guard = Some(inner);
            guard
        }

        pub fn wake_one(&self) {
            self.0.notify_one();
        }

        pub fn wake_all(&self) {
            self.0.notify_all();
        }
    }

    /// Values that may be shared between schedulers.
    pub trait Shared: Send + Sync {}
    impl<T: Send + Sync + ?Sized> Shared for T {}

    /// Values that may move between schedulers.
    pub trait Sendable: Send {}
    impl<T: Send + ?Sized> Sendable for T {}

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
        pub fn lock(&self) -> Guard<'_, T> {
            self.0.borrow_mut()
        }

        /// Access through an exclusive reference, which needs no locking.
        pub fn get_mut(&mut self) -> &mut T {
            self.0.get_mut()
        }
    }

    pub type Guard<'a, T> = core::cell::RefMut<'a, T>;

    /// Where idle schedulers wait for work: with one scheduler, nobody ever waits.
    #[derive(Default)]
    pub struct Wakeup(());

    impl Wakeup {
        pub fn wait<'a, T>(&self, guard: Guard<'a, T>) -> Guard<'a, T> {
            guard
        }

        pub fn wake_one(&self) {}

        pub fn wake_all(&self) {}
    }

    /// Values that may be shared between schedulers: with one scheduler, any.
    pub trait Shared {}
    impl<T: ?Sized> Shared for T {}

    /// Values that may move between schedulers: with one scheduler, any.
    pub trait Sendable {}
    impl<T: ?Sized> Sendable for T {}

    /// A value of any type (a resource's value).
    pub type AnyShared = dyn core::any::Any;
}

pub use imp::{AnyShared, Guard, Lock, Sendable, Shared, Wakeup};

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
    moves::<crate::vm::System>();
}
