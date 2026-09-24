//! `ipd`: the TCP/IP stack for one network or trust domain, serving `/net` over 9P
//! (IO-ARCHITECTURE.md, Networking; NAMESPACES.md, The network tree; answer 174).
//!
//! # What this is trusted for
//! `ipd` is a sink: cleared for nothing, it refuses every labelled caller before it looks at
//! anything else (CONTAINMENT.md). What it is written to guarantee:
//!
//! 1. **A connection reaches only what its scope allows**, IP prefixes and ports, and **never the box's own
//!    addresses**, whatever the scope says ([`scope`]). Both are checked before smoltcp sees the connect.
//! 2. **A scope only narrows**: a `grant` can never widen one or add `listen` to it.
//! 3. **A client can exhaust only its own bucket** (CONTAINMENT.md, the shared server library): fids, minted
//!    connections and parked calls through the skeleton's admission, sockets by `ipd`'s own count against the
//!    same bucket's cap.
//! 4. **Every ISN is drawn from the kernel's CSPRNG**, one fresh seed per connection, and never from
//!    smoltcp's own PRNG ([`stack`]).
//! 5. **No link fault stops `ipd`**: without a working `netd` it answers `unreachable` and asks again.
//!
//! `ipd` has no `unsafe` of its own (`forbid`). It runs smoltcp 0.14.0, vendored, with IPv4,
//! Ethernet and TCP only (vendor/README.md).
//!
//! # Shape
//! - [`args`]: the manifest's arguments, parsed strictly; [`sizing`]: the caps they give. [`scope`]: the
//!   capability and the box's own addresses.
//! - [`link`]: smoltcp's device over `netd`; [`netd`]: that link's calls on the machine.
//! - [`stack`]: the interfaces, the sockets and whose each is. [`fs`]: `/net` over 9P.
//! - [`server`]: what each event does, shared by the program and the host tests.
//! - `fake` (host only): a second smoltcp stack over a recording pipe, for the tests and the fuzz targets.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod args;
#[cfg(not(target_os = "none"))]
pub mod fake;
pub mod fs;
pub mod link;
pub mod netd;
pub mod scope;
pub mod server;
pub mod sizing;
pub mod stack;
