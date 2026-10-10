//! Locks for the threads of one process (userland/native.md, "redoubt-rt"): a [`Semaphore`], a
//! [`Mutex`] and a [`Condvar`] built on it, without a futex or thread-locals, which the kernel
//! does not have.
//!
//! - **A semaphore's waiters sleep in the kernel.** Each semaphore has an endpoint of its own, made the first
//!   time a thread must wait on it, and a count. A thread that takes the count below zero `receive`s on the
//!   endpoint; a thread that raises it from below zero `send`s one empty message there, which one waiter
//!   takes. Waiters are alike, so any one may take it, and the endpoint serves them in the order they began
//!   to wait (kernel/ipc.md). The send blocks only until a waiter that has already taken the count below zero
//!   reaches its `receive`.
//! - **A mutex hands itself over.** A thread spins a little on an uncontended lock, then counts itself among
//!   the lock's holders and waiters and sleeps on the lock's semaphore; the holder's unlock wakes one
//!   sleeper, which then holds the lock, so nobody overtakes a sleeper. A thread waiting for a lock whose
//!   holder the kernel preempted sleeps rather than spins.
//! - **A condvar** queues each waiter on a semaphore of its own, so a wake-up reaches the waiter it was for:
//!   one that began waiting later cannot take it. A wake-up between a waiter's unlock and its sleep is not
//!   lost, since the waiter then finds its semaphore's count already raised. The semaphores are kept for
//!   later waiters, so a condvar makes an endpoint for each thread that ever waited on it at once, not for
//!   each wait.
//!
//! - **What it costs.** A semaphore's endpoint is a page of the process's budget and two handles, made the
//!   first time a thread must wait on it, and given back when the semaphore is dropped. So a lock costs
//!   nothing until it is contended. A program that must not allocate when its locks are contended, as a VM
//!   near its budget's limit, makes them first with `prepare`. An endpoint the kernel refuses does not end
//!   the process: its waiters poll every [`POLL_US`] for a released token instead of sleeping in the kernel.
//! - **A release can wait.** Waking a sleeper is a `send`, which waits until the woken thread reaches its
//!   `receive`: an unlock can wait for that thread to be scheduled. The woken thread holds nothing then, so
//!   this never deadlocks.
//! - **One thread makes an endpoint.** Another that needs it meanwhile spins briefly and then sleeps between
//!   looks, so a maker the kernel preempted costs the others little.
//!
//! Taking a lock the thread already holds is a deadlock, not a panic: there is no thread-local to
//! say which thread holds it.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::num::NonZeroU64;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicIsize, AtomicU32, Ordering, fence};

use redoubt_sys::{FOREVER, Handle, WORDS};

use crate::handle::{Endpoint, sleep};
use crate::ipc::Event;

/// How many times a lock is tried before its taker sleeps: long enough for a holder on another
/// hart to finish a short section, short against a kernel entry.
const SPIN: usize = 64;

/// How long a thread waits between looks when it cannot sleep on an endpoint: while another
/// thread makes a semaphore's endpoint, or for a token of a semaphore the kernel refused one.
const POLL_US: u64 = 100;

/// A semaphore's endpoint: not made yet, being made by one thread, made, or refused by the
/// kernel.
const NONE: u32 = 0;
const MAKING: u32 = 1;
const READY: u32 = 2;
const REFUSED: u32 = 3;

/// A counting semaphore whose waiters sleep in the kernel.
pub struct Semaphore {
    /// Tokens available, or less than zero: minus the threads waiting or about to.
    count: AtomicIsize,
    /// Its endpoint's state: [`NONE`], [`MAKING`], [`READY`] or [`REFUSED`].
    state: AtomicU32,
    /// The endpoint's receive right and a handle that sends on it, stored before `state` says
    /// [`READY`].
    receive: AtomicU32,
    send: AtomicU32,
    /// Tokens released to waiters that poll, once the kernel refused the endpoint.
    posted: AtomicIsize,
}

impl Semaphore {
    /// A semaphore holding `tokens` tokens.
    pub const fn new(tokens: isize) -> Semaphore {
        Semaphore {
            count: AtomicIsize::new(tokens),
            state: AtomicU32::new(NONE),
            receive: AtomicU32::new(0),
            send: AtomicU32::new(0),
            posted: AtomicIsize::new(0),
        }
    }

    /// Makes the endpoint now, so no later wait or release makes one: `false` if the kernel
    /// refused it, and the semaphore's waiters will poll.
    pub fn prepare(&self) -> bool { self.ends().is_some() }

    /// Takes a token, sleeping until one is released if there is none.
    pub fn acquire(&self) {
        if self.count.fetch_sub(1, Ordering::AcqRel) > 0 {
            return;
        }
        match self.ends() {
            Some((receive, _)) => loop {
                match receive.receive(FOREVER, 0) {
                    Ok(Event::Send(_)) => break,
                    // Only this semaphore holds the handle that sends here, and nothing calls it.
                    Ok(_) => continue,
                    Err(e) => panic!("a semaphore's wait failed: {e:?}"),
                }
            },
            None => {
                while self
                    .posted
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n > 0).then(|| n - 1))
                    .is_err()
                {
                    let _ = sleep(POLL_US);
                }
            }
        }
        fence(Ordering::Acquire);
    }

    /// Releases a token, waking one sleeper if any waits. The wake-up is a `send`, which waits
    /// until the woken thread, which holds nothing, reaches its `receive`.
    pub fn release(&self) {
        if self.count.fetch_add(1, Ordering::AcqRel) < 0 {
            match self.ends() {
                Some((_, send)) => {
                    if let Err((e, _)) = send.send(&[0; WORDS], &[], None, FOREVER) {
                        panic!("a semaphore's wake-up failed: {e:?}");
                    }
                }
                None => {
                    self.posted.fetch_add(1, Ordering::AcqRel);
                }
            }
        }
    }

    /// The endpoint and a handle that sends on it, made now if no thread has made them yet, or
    /// `None` if the kernel refused them. One thread makes them; another that needs them
    /// meanwhile waits for it, spinning briefly, then sleeping between looks.
    fn ends(&self) -> Option<(Endpoint, Endpoint)> {
        let mut looks = 0;
        loop {
            match self.state.load(Ordering::Acquire) {
                READY => {
                    let (receive, send) =
                        (self.receive.load(Ordering::Relaxed), self.send.load(Ordering::Relaxed));
                    return Some((endpoint(receive), endpoint(send)));
                }
                REFUSED => return None,
                NONE if self
                    .state
                    .compare_exchange(NONE, MAKING, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok() =>
                {
                    let Some((receive, send)) = make() else {
                        self.state.store(REFUSED, Ordering::Release);
                        return None;
                    };
                    self.receive.store(receive.handle().index(), Ordering::Relaxed);
                    self.send.store(send.handle().index(), Ordering::Relaxed);
                    self.state.store(READY, Ordering::Release);
                    return Some((receive, send));
                }
                _ => {
                    looks += 1;
                    if looks < SPIN {
                        core::hint::spin_loop();
                    } else {
                        let _ = sleep(POLL_US);
                    }
                }
            }
        }
    }
}

/// A new endpoint and a handle that sends on it, or `None` if the kernel refuses either.
fn make() -> Option<(Endpoint, Endpoint)> {
    let receive = Endpoint::create().ok()?;
    match receive.mint(NonZeroU64::MIN, None) {
        Ok(send) => Some((receive, send)),
        Err(_) => {
            let _ = receive.close();
            None
        }
    }
}

/// The endpoint a stored handle index names.
fn endpoint(index: u32) -> Endpoint { Endpoint::from_handle(Handle::new(index).expect("a stored handle")) }

impl Drop for Semaphore {
    /// Closes the endpoint's handles: handles are not closed on drop ([`crate::handle`]).
    fn drop(&mut self) {
        if *self.state.get_mut() == READY {
            for index in [*self.send.get_mut(), *self.receive.get_mut()] {
                if let Some(handle) = Handle::new(index) {
                    let _ = crate::handle::close(handle);
                }
            }
        }
    }
}

impl Default for Semaphore {
    fn default() -> Semaphore { Semaphore::new(0) }
}

/// A lock around a `T`, whose waiters sleep in the kernel ([module docs](self)).
pub struct Mutex<T> {
    /// 0 free; otherwise the holder and every thread waiting or about to.
    takers: AtomicIsize,
    /// Where waiters sleep; the unlock hands the lock to one.
    handover: Semaphore,
    value: UnsafeCell<T>,
}

// SAFETY: the value is reached only through a `MutexGuard`, and `lock` gives one to a single
// thread at a time (the holder counted in `takers` is unique until it unlocks), so sharing the
// mutex shares the `T` between threads one at a time: what `T: Send` allows.
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Mutex<T> {
        Mutex { takers: AtomicIsize::new(0), handover: Semaphore::new(0), value: UnsafeCell::new(value) }
    }

    /// Waits until the lock is free and takes it.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        for _ in 0..SPIN {
            if self.takers.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return MutexGuard { mutex: self, _not_send: PhantomData };
            }
            core::hint::spin_loop();
        }
        if self.takers.fetch_add(1, Ordering::Acquire) != 0 {
            // Held: sleep until the holder hands it over.
            self.handover.acquire();
        }
        MutexGuard { mutex: self, _not_send: PhantomData }
    }

    /// Makes the endpoint its waiters sleep on now ([`Semaphore::prepare`]).
    pub fn prepare(&self) -> bool { self.handover.prepare() }

    /// Access through an exclusive reference, which needs no locking.
    pub fn get_mut(&mut self) -> &mut T { self.value.get_mut() }

    pub fn into_inner(self) -> T { self.value.into_inner() }

    fn unlock(&self) {
        if self.takers.fetch_sub(1, Ordering::AcqRel) != 1 {
            self.handover.release();
        }
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Mutex<T> { Mutex::new(T::default()) }
}

/// The lock, held until this is dropped.
pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
    /// The lock is the holding thread's to release, as for `std`'s guard.
    _not_send: PhantomData<*const ()>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this guard is the lock's one holder (`Mutex::lock`), and the borrow is tied to
        // the guard's, so no `&mut T` exists beside it.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`, through `&mut self`, so this is the one borrow of the value.
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) { self.mutex.unlock(); }
}

/// Where threads wait for a change another thread makes under a [`Mutex`].
#[derive(Default)]
pub struct Condvar {
    queue: Mutex<Queue>,
}

/// A condvar's waiters, each on a semaphore of its own, and the semaphores no waiter holds.
#[derive(Default)]
struct Queue {
    waiting: VecDeque<Arc<Semaphore>>,
    spare: Vec<Arc<Semaphore>>,
}

impl Condvar {
    pub const fn new() -> Condvar {
        Condvar { queue: Mutex::new(Queue { waiting: VecDeque::new(), spare: Vec::new() }) }
    }

    /// Makes now what `waiters` threads waiting at once need, so none of their waits makes an
    /// endpoint ([`Semaphore::prepare`]): `false` if the kernel refused one.
    pub fn prepare(&self, waiters: usize) -> bool {
        let mut ready = self.queue.prepare();
        let mut queue = self.queue.lock();
        while queue.spare.len() < waiters {
            let one = Arc::new(Semaphore::new(0));
            ready &= one.prepare();
            queue.spare.push(one);
        }
        ready
    }

    /// Releases `guard`'s lock, sleeps until woken, and takes the lock again. A wake-up may
    /// come without the change waited for: the caller checks again.
    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        let mutex = guard.mutex;
        let mine = {
            let mut queue = self.queue.lock();
            let mine = queue.spare.pop().unwrap_or_default();
            queue.waiting.push_back(Arc::clone(&mine));
            mine
        };
        drop(guard);
        mine.acquire();
        self.queue.lock().spare.push(mine);
        mutex.lock()
    }

    /// Wakes the longest waiter, if any.
    pub fn notify_one(&self) {
        let first = self.queue.lock().waiting.pop_front();
        if let Some(waiter) = first {
            waiter.release();
        }
    }

    /// Wakes every waiter.
    pub fn notify_all(&self) {
        let waiting = core::mem::take(&mut self.queue.lock().waiting);
        for waiter in waiting {
            waiter.release();
        }
    }
}
