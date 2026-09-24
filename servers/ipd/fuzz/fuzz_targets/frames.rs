//! Frames into `ipd`'s stack: well-formed TCP segments with every field from the input,
//! well-formed IPv4 that is not TCP (UDP, ICMP, other protocols, from every source), raw
//! bytes, the peer's own traffic, the clock and the stack's own operations
//! (`redoubt_ipd::fake::drive_frames`, the same run as the seeded sweep in `tests/sweep.rs`).
//! Claims, checked as it runs: no panic; every frame `ipd` sends is TCP or ARP, never a SYN to
//! one of the box's own addresses, never an ARP request for one but the gateway; and the socket
//! table's invariants hold after every step.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    redoubt_ipd::fake::drive_frames(data);
});
