//! The D3 rig (answer 174; plan section 6.3): the real `netd` and `ipd`, booted through the loader
//! stub by a launcher that stands in for WP-R3's `init`, with clients, a victim and attackers
//! beside them. What each case checks is in `src/rig.rs`; what the bench checks from outside (the
//! peers' counts and the capture) is in its `tests/d3-net-*.toml`.

#![no_std]

extern crate alloc;

#[cfg(target_os = "none")]
pub mod rig;

/// The prefixes the rig's `ipd` lists as its own (`self=`), beyond its own address and network.
/// 10.0.2.0/24 is slirp's network, every address of which but the resolver leads to the host's
/// loopback; 10.0.9.102 is a peer standing in for an address that routes back to the box.
pub const SELF_ARGS: &[&str] = &["10.0.2.0/24", "10.0.9.102/32"];

/// Every address `ipd` refuses whatever its arguments (NAMESPACES.md, the box's own addresses),
/// as the bench's capture check names them.
pub const SELF_ALWAYS: &[&str] =
    &["0.0.0.0/8", "127.0.0.0/8", "224.0.0.0/4", "240.0.0.0/4", "255.255.255.255/32"];
