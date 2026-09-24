//! `ipd`: the TCP/IP stack for one network or trust domain, serving `/net` over 9P
//! (IO-ARCHITECTURE.md, Networking; NAMESPACES.md, The network tree; answer 174).
//!
//! # What this is trusted for
//! `ipd` is a sink: cleared for nothing, it refuses every labelled caller before it looks at
//! anything else (CONTAINMENT.md). What it is written to guarantee:
//!
//! 1. **A connection reaches only what its scope allows**, IP prefixes and ports, and **never the
//!    box's own addresses**, whatever the scope says ([`scope`]).
//! 2. **A scope only narrows**: a `grant` can never widen one or add `listen` to it.
//!
//! `ipd` has no `unsafe` of its own (`forbid`).
//!
//! # Shape
//! - [`args`]: the manifest's arguments, parsed strictly. [`scope`]: the capability and the box's
//!   own addresses.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod args;
pub mod scope;
