//! The net cases (docs/testbench.md, "Peers, dials and the capture"): the real `netd` and `ipd`
//! under `init`, each case from a manifest of its own, with the clients as `servers` entries
//! (`tests/net/client`) and the judge (`src/bin/net-judge.rs`) as the reporter. What the bench
//! checks from outside (the peers' counts, the dials and the capture) is in each case's file,
//! and `tests/cases.rs` keeps each case file and its manifest in step.

#![no_std]

/// Every address `ipd` refuses whatever its arguments (servers/ipd.md, "The box's own addresses"),
/// as the bench's capture check names them.
pub const SELF_ALWAYS: &[&str] =
    &["0.0.0.0/8", "127.0.0.0/8", "224.0.0.0/4", "240.0.0.0/4", "255.255.255.255/32"];
