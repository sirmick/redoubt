//! `fsd`: the file server for one volume, a range at `blkd` holding a littlefs filesystem,
//! served over 9P (servers/fsd.md).
//!
//! **One volume, one label set.** An `fsd` holds one range badge at `blkd` and nothing else, so
//! a parser exploit reaches that medium alone (R47). Every node reports the volume's labels, so
//! the skeleton's label check runs on every request; there are no per-file labels, owners or
//! permission bits.
//!
//! **Mounting.** A range whose superblock pair reads all zero has never been written and is
//! formatted; any other range that does not mount is served as corrupt, every attach refused
//! with `corrupt`, and `fsd` stays up, so a damaged or hostile medium never becomes a restart
//! loop ([`volume::mount`]). An I/O error from `blkd` later makes the volume corrupt until it is
//! mounted again.
//!
//! **Nodes.** A fid rests on a path built from the names its client walked and the id the file
//! had there ([`server::Node`]): every request finds the file again and checks the id, so a
//! removed file's other fids get `removed`, and never reach a file that took its place.
//!
//! **Quotas.** Each root a connection is minted at has a byte quota carved from the live root
//! above it, counted when the first connection is minted there and never stored; every change is
//! charged to the nearest live root above it (R48, [`quota`]).
//!
//! **Typed operations.** `rename`, `copy_file`, `set_attr` and `get_attr` on the 9P endpoint
//! ([`typed`]), naming the caller's own fids through the skeleton.
//!
//! **Packing.** On the host, [`pack`] writes a tree into a volume's bytes through this same code,
//! for the disk image, so no second writer has to keep the id rule.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod blkd;
#[cfg(not(target_os = "none"))]
pub mod pack;
mod quota;
pub mod server;
pub mod typed;
pub mod volume;

pub use server::{Args, BUDGET, BadArgs, COST, Fsd, limits, parse_args};
pub use volume::{Mounted, NoVolume, Range, mount};
