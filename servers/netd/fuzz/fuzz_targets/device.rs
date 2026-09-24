//! A virtio-net device that lies, driven by the fuzzer: every byte of the input picks what the
//! device does next. The claims, asserted here: `netd` never panics, never names an address
//! outside its two regions, never names one queue's memory in the other's region, and never
//! hands `ipd` a frame of a length it does not carry. `redoubt_netd::fake::exercise` is the same
//! run the randomized sweep in `tests/device.rs` makes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_netd::fake::exercise;

fuzz_target!(|data: &[u8]| {
    let mut rest = data;
    let mut next = || match rest.split_first() {
        Some((first, tail)) => {
            rest = tail;
            *first
        }
        None => 0,
    };
    let nic = exercise(&mut next);
    assert_eq!(nic.strayed(), 0);
    assert_eq!(nic.crossed(), 0);
    assert_eq!(nic.overread(), 0);
});
