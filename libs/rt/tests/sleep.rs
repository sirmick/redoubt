//! `sleep(0)` is the yield (docs/kernel/timer.md, "Timeouts and `FOREVER`"): exactly one
//! `receive` from no handle with timeout 0, which returns `Ok` at once. Against a transport that
//! records each call and forwards it to the fake kernel. One test per binary: the transport is
//! installed once.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Call, Error, Return};
use redoubt_rt::handle::sleep;
use redoubt_rt::{Transport, install_transport};

/// Every call, as the runtime made it.
struct Recording(Mutex<Vec<Call>>);

// SAFETY: forwards to the fake.
unsafe impl Transport for Recording {
    fn call(&self, call: &Call) -> Result<Return, Error> {
        self.0.lock().unwrap().push(*call);
        fake().call(call)
    }
}

#[test]
fn sleep_zero_is_one_receive_from_nothing_that_returns_at_once() {
    static RECORDING: Recording = Recording(Mutex::new(Vec::new()));
    assert!(install_transport(&RECORDING), "another transport was installed first");

    let f = fake();
    let pid = f.process(0, &[]);
    let began = Instant::now();
    assert_eq!(f.as_process(pid, || sleep(0)), Ok(()));
    // A poll: nothing to wait for, so no wait at all (a blocked sleep would wait for ever).
    assert!(began.elapsed() < Duration::from_secs(5), "sleep(0) took {:?}", began.elapsed());
    let calls = RECORDING.0.lock().unwrap().clone();
    let [Call::Receive { from: None, timeout: 0, max_transfer: 0, .. }] = calls[..] else {
        panic!("sleep(0) made {calls:?}");
    };
}
