//! `erofsd`: the file server for one read-only volume, a range at `blkd` or at a `verityd` holding
//! an EROFS volume in the subset `erofs` reads, served over 9P (servers/erofsd.md).
//!
//! **One volume, one label set.** An `erofsd` holds one range badge and nothing else, so a parser
//! exploit reaches that medium alone (R47). Every node reports the volume's labels, so the
//! skeleton's label check runs on every request.
//!
//! **Corrupt, not a crash.** A volume whose superblock or root does not parse is served as
//! corrupt: every attach refused with `corrupt`, and `erofsd` stays up (R49). A range read that
//! fails makes the volume corrupt until `erofsd` starts again.
//!
//! **Nodes.** A fid rests on an inode, read and checked once when it was walked to, and its name;
//! every request on the fid uses that inode and finds nothing again ([`server::Node`]).
//!
//! **Read-only.** Every way of writing is refused with `read-only volume`, before the range sees
//! anything; `erofsd` sends no write.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod blkd;
pub mod server;
#[cfg(feature = "boot-stats")]
pub mod stats;

pub use server::{Args, BUDGET, BadArgs, COST, Erofsd, Fault, Range, limits, parse_args};
