//! `/net` through the 9P skeleton, statefully: requests (hostile and well-formed), `ctl`
//! operations, data, grants, `new_connection` and `disconnect` from several callers, interleaved
//! with frames, the peer and the clock (`redoubt_ipd::fake::drive_session`, the same run as the
//! seeded sweep in `tests/sweep.rs`). Claims, checked as it runs: no panic; every reply decodes;
//! every frame `ipd` sends keeps the frame invariant; the socket table's invariants hold after
//! every step; and once everything is disconnected and the clock is past every linger, nothing is
//! still minted.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    redoubt_ipd::fake::drive_session(data);
});
