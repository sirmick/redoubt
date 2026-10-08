//! What the volume servers share beside the 9P skeleton (servers/serving.md, "The 9P server
//! skeleton"): `littlefsd`, `walfsd` and `erofsd` serve one volume each, a range at `blkd`, and
//! `verityd` serves one range on `blkd`'s own protocol. Each once carried its own copy of the
//! same four pieces; one copy each lives here, with one set of tests.
//!
//! - [`args`]: the arguments `init` passes a volume server, `endpoint=NAME` and `labels=ID[,ID...]`, under
//!   the manifest's rules;
//! - [`range`]: the range at `blkd` (or at a `verityd`) as a format needs it, and the one client of `blkd`'s
//!   protocol;
//! - [`quota`]: the byte quotas per attach root that `littlefsd` and `walfsd` carve, a ledger their servers
//!   keep counted.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod args;
pub mod quota;
pub mod range;
