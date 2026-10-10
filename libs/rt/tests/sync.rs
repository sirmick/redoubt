//! `sync`: a mutex excludes and hands itself over, a condvar loses no wake-up, waiters sleep on
//! an endpoint rather than spin, and `thread::scope` ends its threads before it returns. The
//! locks' threads are host threads acting as one fake process, so the file runs under Miri
//! (`rt-miri`); `scope`'s tests start threads with `thread_create`, whose closure crosses the
//! fake as an integer Miri cannot follow, so they run natively only, as `thread.rs` does. A lost
//! wake-up or a lock never handed over shows as a hang, so each test runs under a watchdog that
//! fails it instead. Locks made ready with `prepare` make no endpoint when contended, and a lock
//! whose endpoint the kernel refuses still works.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Call, Error, Return};
use redoubt_rt::handle::sleep;
use redoubt_rt::sync::{Condvar, Mutex, Semaphore};
use redoubt_rt::thread::scope;
use redoubt_rt::{Transport, install_transport};

/// Counts the `receive`s made on an endpoint: a waiter sleeping in the kernel.
struct Counting(AtomicUsize);

// SAFETY: forwards every call to the fake, unchanged.
unsafe impl Transport for Counting {
    fn call(&self, call: &Call) -> Result<Return, Error> {
        if matches!(call, Call::Receive { from: Some(_), .. }) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fake().call(call)
    }
}

static COUNTING: Counting = Counting(AtomicUsize::new(0));

/// Runs `body` as a new fake process, failing if it has not finished within a minute. `body`
/// is given the process, for [`threads`].
fn within_a_minute(body: impl FnOnce(usize) + Send + 'static) {
    install_transport(&COUNTING);
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let f = fake();
        let pid = f.process(1001, &[]);
        f.as_process(pid, || body(pid));
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(60))
        .expect("a waiter was never woken: a lost wake-up or a lock never handed over");
}

/// Runs each of `bodies` on a host thread of its own as process `pid`, and waits for them all.
fn threads<'a>(pid: usize, bodies: Vec<Box<dyn FnOnce() + Send + 'a>>) {
    std::thread::scope(|s| {
        for body in bodies {
            s.spawn(move || fake().as_process(pid, body));
        }
    });
}

/// Rounds per thread: fewer under Miri, which runs these in `rt-miri`.
fn rounds() -> usize { if cfg!(miri) { 50 } else { 2_000 } }

/// Four threads add to one count under the lock, each checking that nobody else is inside: no
/// two ever are, and no addition is lost.
#[test]
fn a_mutex_excludes_and_loses_nothing_under_contention() {
    within_a_minute(|pid| {
        let (count, inside) = (Mutex::new(0usize), AtomicBool::new(false));
        let add = || {
            for _ in 0..rounds() {
                let mut n = count.lock();
                assert!(!inside.swap(true, Ordering::SeqCst), "two threads held the lock");
                *n += 1;
                inside.store(false, Ordering::SeqCst);
            }
        };
        threads(pid, vec![Box::new(add), Box::new(add), Box::new(add), Box::new(add)]);
        assert_eq!(*count.lock(), 4 * rounds());
    });
}

/// A holder that sleeps in the kernel with the lock held: the threads waiting for it sleep on
/// the lock's endpoint rather than spin, and each takes the lock in turn once it is let go.
#[test]
fn waiters_for_a_held_lock_sleep_and_each_gets_it() {
    within_a_minute(|pid| {
        let (count, ready, holding) = (Mutex::new(0usize), AtomicUsize::new(0), AtomicBool::new(false));
        let before = COUNTING.0.load(Ordering::SeqCst);
        let wait = || {
            // The holder takes the lock before any waiter tries it.
            while !holding.load(Ordering::SeqCst) {
                sleep(1_000).unwrap();
            }
            ready.fetch_add(1, Ordering::SeqCst);
            *count.lock() += 1;
        };
        let hold = || {
            let held = count.lock();
            holding.store(true, Ordering::SeqCst);
            while ready.load(Ordering::SeqCst) < 3 {
                sleep(1_000).unwrap();
            }
            // Long enough for each waiter to pass its spin and sleep.
            sleep(50_000).unwrap();
            assert!(COUNTING.0.load(Ordering::SeqCst) > before, "the waiters spun instead of sleeping");
            assert_eq!(*held, 0, "a waiter took the lock from its holder");
        };
        threads(pid, vec![Box::new(hold), Box::new(wait), Box::new(wait), Box::new(wait)]);
        assert_eq!(*count.lock(), 3);
    });
}

/// A mutex and a condvar prepared first make no endpoint however their threads contend: what a
/// program near its budget's limit needs of its locks. The process's handles stay as they were.
#[test]
fn prepared_locks_make_no_endpoint_when_contended() {
    within_a_minute(|pid| {
        let (turn, changed) = (Mutex::new(0usize), Condvar::new());
        assert!(turn.prepare() && changed.prepare(2));
        let before = fake().held(pid).0;
        let last = 2 * rounds();
        let player = |me: usize| {
            let (turn, changed) = (&turn, &changed);
            move || loop {
                let mut t = turn.lock();
                while *t < last && *t % 2 != me {
                    t = changed.wait(t);
                }
                if *t >= last {
                    return;
                }
                *t += 1;
                changed.notify_all();
            }
        };
        threads(pid, vec![Box::new(player(0)), Box::new(player(1))]);
        assert_eq!(*turn.lock(), last);
        assert_eq!(fake().held(pid).0, before, "a contended lock made an endpoint");
    });
}

/// A lock whose endpoint the kernel refuses still excludes, and its waiters still get it: they
/// poll for the hand-over rather than ending the process.
#[test]
fn a_refused_endpoint_leaves_a_working_lock() {
    within_a_minute(|pid| {
        fake().refuse(pid, "endpoint_create", Error::OutOfMemory);
        let (count, inside) = (Mutex::new(0usize), AtomicBool::new(false));
        assert!(!count.prepare(), "the kernel refused the endpoint");
        let add = || {
            for _ in 0..rounds() / 10 {
                let mut n = count.lock();
                assert!(!inside.swap(true, Ordering::SeqCst), "two threads held the lock");
                *n += 1;
                inside.store(false, Ordering::SeqCst);
            }
        };
        threads(pid, vec![Box::new(add), Box::new(add), Box::new(add)]);
        assert_eq!(*count.lock(), 3 * (rounds() / 10));
    });
}

/// Two threads take turns through one condvar, each waiting for the other's change: a wake-up
/// lost between a waiter's unlock and its sleep, or taken by a later waiter, would stop both.
#[test]
fn a_condvar_ping_pong_loses_no_wake_up() {
    within_a_minute(|pid| {
        let (turn, changed) = (Mutex::new(0usize), Condvar::new());
        let last = 2 * rounds();
        let player = |me: usize| {
            let (turn, changed) = (&turn, &changed);
            move || loop {
                let mut t = turn.lock();
                while *t < last && *t % 2 != me {
                    t = changed.wait(t);
                }
                if *t >= last {
                    return;
                }
                *t += 1;
                changed.notify_all();
            }
        };
        threads(pid, vec![Box::new(player(0)), Box::new(player(1))]);
        assert_eq!(*turn.lock(), last);
    });
}

/// Three waiters and a waker that wakes one at a time: every wake-up reaches a waiter.
#[test]
fn notify_one_wakes_each_waiter_once() {
    within_a_minute(|pid| {
        let (tokens, changed, woken) = (Mutex::new(0usize), Condvar::new(), AtomicUsize::new(0));
        let each = rounds() / 10;
        let take = || {
            for _ in 0..each {
                let mut t = tokens.lock();
                while *t == 0 {
                    t = changed.wait(t);
                }
                *t -= 1;
                woken.fetch_add(1, Ordering::SeqCst);
            }
        };
        let give = || {
            for _ in 0..3 * each {
                *tokens.lock() += 1;
                changed.notify_one();
            }
        };
        threads(pid, vec![Box::new(take), Box::new(take), Box::new(take), Box::new(give)]);
        assert_eq!(woken.load(Ordering::SeqCst), 3 * each);
    });
}

/// A semaphore's releases and acquires racing from two threads: every token is taken once.
#[test]
fn semaphore_tokens_are_neither_lost_nor_doubled() {
    within_a_minute(|pid| {
        let tokens = Semaphore::new(0);
        let give = || {
            for _ in 0..rounds() {
                tokens.release();
            }
        };
        let take = || {
            for _ in 0..rounds() {
                tokens.acquire();
            }
        };
        threads(pid, vec![Box::new(give), Box::new(take)]);
        tokens.release();
        tokens.acquire();
    });
}

/// `scope` returns only once its threads have ended, so what they borrowed is theirs until then.
#[test]
#[cfg_attr(miri, ignore = "thread_create's closure crosses the fake as an integer")]
fn scope_waits_for_its_threads() {
    within_a_minute(|_| {
        let done = AtomicUsize::new(0);
        scope(1, |s| {
            for _ in 0..3 {
                s.spawn(|| {
                    sleep(20_000).unwrap();
                    done.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
            }
        });
        assert_eq!(done.load(Ordering::SeqCst), 3);
    });
}

/// A thread the kernel refuses never runs, and `scope` does not wait for it.
#[test]
#[cfg_attr(miri, ignore = "thread_create's closure crosses the fake as an integer")]
fn a_refused_thread_is_not_waited_for() {
    install_transport(&COUNTING);
    let f = fake();
    let pid = f.process(1001, &[]);
    f.refuse(pid, "thread_create", Error::OutOfMemory);
    let ran = AtomicBool::new(false);
    let got = f.as_process(pid, || scope(1, |s| s.spawn(|| ran.store(true, Ordering::SeqCst))));
    assert_eq!(got, Err(Error::OutOfMemory));
    assert!(!ran.load(Ordering::SeqCst));
}
