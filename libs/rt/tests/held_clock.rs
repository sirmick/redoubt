//! The fake kernel's held clock: a tick is a quiescent step. A server reads the time and is slow
//! to reach the receive that reading gave its timeout; the test's tick, taken as soon as the
//! server has read the time, must still end that receive. A tick taken before the receive would
//! leave it a deadline one timeout past the tick, which no later tick reaches, and its caller
//! waiting until the fake's stuck guard (`parked_write` under Miri, whose scheduler widens the
//! gap natively measured in microseconds).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::Error;
use redoubt_rt::handle::{self, Endpoint};

/// The receive's timeout, and the one tick the test takes.
const TIMEOUT: u64 = 300_000;

#[test]
fn a_tick_waits_for_the_receive_whose_deadline_it_ends() {
    let f = fake();
    f.hold_clock();
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
