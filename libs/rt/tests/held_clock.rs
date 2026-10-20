//! The fake kernel's held clock: a tick is a quiescent step. A server reads the time and is slow
//! to reach the receive that reading gave its timeout; the test's tick, taken as soon as the
//! server has read the time, must still end that receive. A tick taken before the receive would
//! leave it a deadline one timeout past the tick, which no later tick reaches, and its caller
//! waiting until the fake's stuck guard (`parked_write` under Miri, whose scheduler widens the
//! gap natively measured in microseconds). And a thread a change has woken is not in a wait,
//! whether or not the host has run it yet: a tick taken straight after the change waits for it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use redoubt_fake_kernel::{answer, fake};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::ipc::Event;

/// The receive's timeout, and the one tick the test takes.
const TIMEOUT: u64 = 300_000;

/// A test's turn: the clock is the binary's, and a tick steps every test's deadlines at once, so
/// the tests run one at a time, each taking its own.
fn turn() -> MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    fake().hold_clock();
    turn
}

#[test]
fn a_tick_waits_for_the_receive_whose_deadline_it_ends() {
    let _turn = turn();
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);

    // Whether the server has read the time its receive's deadline runs from.
    let read = Arc::new(AtomicBool::new(false));
    let has_read = read.clone();
    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let before = handle::time_now().unwrap();
        has_read.store(true, Ordering::Release);
        // The gap between the reading and the receive, made wide: the tick falls in it.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let answer = ep.receive(TIMEOUT, 0);
        let waited = handle::time_now().unwrap() - before;
        assert!(matches!(answer, Err(Error::Timeout)), "the receive did not time out: {answer:?}");
        assert_eq!(waited, TIMEOUT, "the receive ended after {waited} µs, not with the one tick");
        0
    });

    while !read.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    f.tick(TIMEOUT);
    server_thread.join().unwrap();
}

/// A client is in a call; the server's reply wakes it, and the test ticks at once, before the
/// host has run the client. The client reads the time as its call returns: it must be the time
/// of the reply, not one tick later. A tick that counted the woken client as still waiting would
/// pass it by, and a server's answer would reach its client one tick late each time (which,
/// repeated across a test's steps, lets a retry fall past the test's last tick). Under Miri the
/// woken thread never runs before the ticker blocks, so the tick is taken first unless it waits.
#[test]
fn a_tick_waits_for_the_thread_a_change_woke() {
    let _turn = turn();
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let client = f.process(1, &[]);
    let conn = f.grant(server, receive, client, 7);

    // The time the client's call returned at.
    let returned = Arc::new(AtomicU64::new(0));
    let seen = returned.clone();
    let client_thread = f.run(client, move || {
        let (reply, _) =
            Endpoint::from_handle(conn).call(&[1, 0, 0, 0], &[], None, FOREVER).into_result().unwrap();
        assert_eq!(reply.words[1], 42);
        seen.store(handle::time_now().unwrap(), Ordering::Release);
        0
    });

    // The server takes the call and replies; the reply is the change that wakes the client.
    let replied = f.as_process(server, || {
        let Ok(Event::Call(request)) = Endpoint::from_handle(receive).receive(FOREVER, 0) else {
            panic!("no call")
        };
        answer(request, [0, 42, 0, 0]).unwrap();
        handle::time_now().unwrap()
    });
    f.tick(TIMEOUT);
    client_thread.join().unwrap();
    assert_eq!(returned.load(Ordering::Acquire), replied, "the client saw the tick before its reply");
}

/// The tick itself wakes the threads whose deadlines it reaches: a second tick, taken at once,
/// waits for them as for any woken thread, so a thread the first tick woke reads the first
/// tick's time, not the second's.
#[test]
fn a_tick_waits_for_the_thread_the_last_tick_woke() {
    let _turn = turn();
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);

    // Whether the server has started, and the time its receive timed out at.
    let started = Arc::new(AtomicBool::new(false));
    let has_started = started.clone();
    let returned = Arc::new(AtomicU64::new(0));
    let seen = returned.clone();
    let server_thread = f.run(server, move || {
        has_started.store(true, Ordering::Release);
        let answer = Endpoint::from_handle(receive).receive(TIMEOUT, 0);
        assert!(matches!(answer, Err(Error::Timeout)), "the receive did not time out: {answer:?}");
        seen.store(handle::time_now().unwrap(), Ordering::Release);
        0
    });

    // Once the server is a thread of the fake, a tick of nothing is taken when its receive
    // waits, so the next tick is the one that ends it.
    while !started.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    f.tick(0);
    let before = f.as_process(server, || handle::time_now().unwrap());
    f.tick(TIMEOUT);
    f.tick(TIMEOUT);
    server_thread.join().unwrap();
    assert_eq!(
        returned.load(Ordering::Acquire),
        before + TIMEOUT,
        "the thread the first tick woke saw the second"
    );
}

/// A woken thread need not wait again: it may end instead, and its end is what the tick was
/// waiting for. Here the change is the endpoint's destruction, which ends the receive and the
/// thread with it; the tick after it returns rather than waiting for a thread that is not coming
/// back (the fake's stuck guard would be the only end of such a wait).
#[test]
fn a_tick_does_not_wait_for_a_woken_thread_that_ends() {
    let _turn = turn();
    let f = fake();
    let owner = f.process(0, &[]);
    let receive = f.endpoint(owner);
    let started = Arc::new(AtomicBool::new(false));
    let has_started = started.clone();
    let waiter = f.run(owner, move || {
        has_started.store(true, Ordering::Release);
        let answer = Endpoint::from_handle(receive).receive(FOREVER, 0);
        assert!(matches!(answer, Err(Error::Dead)), "the receive did not end with its endpoint: {answer:?}");
        0
    });

    // Once the waiter is a thread of the fake, a tick of nothing is taken when its receive
    // waits, so the destruction wakes it.
    while !started.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    f.tick(0);
    f.destroy(owner, receive);
    f.tick(TIMEOUT);
    waiter.join().unwrap();
}
