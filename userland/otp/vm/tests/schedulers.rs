//! Several schedulers (the `std` build's threads): work crossing them through the system lock
//! loses nothing, and schedulers going offline and online again never leave the VM with nobody to
//! fire its timers. A hang fails the test after a minute rather than stalling the suite. The
//! fixture's source is `src/schedulers.erl`.
#![cfg(feature = "std")]

use std::sync::mpsc;
use std::time::Duration;

use beamlet_vm::Vm;
use beamlet_vm::platform::{Lookup, Platform, PlatformError};

/// Time moves only when the VM idles, to its deadline: a timer always fires.
struct TestPlatform {
    now: u64,
}

impl Platform for TestPlatform {
    fn monotonic_us(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    fn system_time_us(&mut self) -> Option<u64> { None }

    fn idle(&mut self, deadline: Option<u64>) {
        if let Some(d) = deadline {
            self.now = self.now.max(d);
        }
    }

    fn console_write(&mut self, _bytes: &[u8]) {}

    fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

    fn load_module(&mut self, module: &str) -> Lookup {
        match module {
            "schedulers" => Lookup::Found(include_bytes!("fixtures/schedulers.beam").to_vec()),
            _ => Lookup::Absent,
        }
    }
}

/// `schedulers:function()` on `n` schedulers, as text, failing if it has not ended in a minute.
fn run(function: &'static str, n: usize) -> String {
    let (done, ended) = mpsc::channel();
    std::thread::spawn(move || {
        let mut vm = Vm::new(Box::new(TestPlatform { now: 0 }));
        vm.set_schedulers(n);
        let pid = vm.spawn("schedulers", function, |_| Vec::new()).expect("spawn");
        let text = match vm.run(pid) {
            Ok(Ok(value)) => format!("{value}"),
            other => format!("{other:?}"),
        };
        let _ = done.send(text);
    });
    ended.recv_timeout(Duration::from_secs(60)).expect("the VM stopped with work left: nobody ran it")
}

#[test]
fn two_schedulers_lose_nothing_across_the_system_lock() {
    for _ in 0..10 {
        assert_eq!(run("busy", 2), "{2,8,[0,0,0,0,0,0,0,0],[2000,2000,2000,2000],400,tick}");
    }
}

/// A helper going offline may be the last scheduler running while another sleeps: the sleeper
/// takes over the timers, so each round's sleep ends.
#[test]
fn schedulers_go_offline_and_online_again() {
    for _ in 0..20 {
        assert_eq!(run("online", 2), "{2,20}");
    }
}
