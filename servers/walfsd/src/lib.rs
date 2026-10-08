//! `walfsd`: the file server for one volume, a range at `blkd` holding a walfs filesystem, served
//! over 9P (servers/walfsd.md).
//!
//! **One volume, one label set.** A `walfsd` holds one range badge at `blkd` and nothing else, so
//! a parser exploit reaches that medium alone (R47). Every node reports the volume's labels, so
//! the skeleton's label check runs on every request; there are no per-file labels, owners or
//! permission bits.
//!
//! **Mounting.** A range whose superblock and log header read all zero has never been written
//! and is formatted; any other range that does not mount is served as corrupt, every attach
//! refused with `corrupt`, and `walfsd` stays up, so a damaged or hostile medium never becomes a
//! restart loop ([`volume::mount`]). The mount recovers a transaction a power cut left in the log.
//! A block that fails its hash later is `corrupt` for the request that read it; an I/O error from
//! `blkd` makes the volume corrupt until it is mounted again.
//!
//! **Nodes.** A fid rests on a path built from the names its client walked, and the inode and
//! generation the file had there ([`server::Node`]): every request finds the file again and checks
//! the pair, so a removed file's other fids get `removed`, and never reach a file that took its
//! inode.
//!
//! **Quotas.** Each root a connection is minted at has a byte quota carved from the live root
//! above it, counted when the first connection is minted there and never stored; every change is
//! charged to the nearest live root above it (R48, [`quota`]). An entry holds its blocks and a
//! share of the volume for its inode, so no root's quota promises an inode the volume lacks.
//!
//! **Typed operations.** `littlefsd`'s `rename`, `copy_file`, `set_attr` and `get_attr` on the
//! 9P endpoint ([`typed`]), naming the caller's own fids through the skeleton.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "cut-after-write")]
pub mod cut;
#[cfg(feature = "one-volume-probe")]
pub mod one_volume;
mod quota;
pub mod server;
pub mod typed;
pub mod volume;

pub use redoubt_fileserver::args::{Args, BadArgs, parse_args};
pub use server::{BUDGET, COST, Walfsd, limits};
pub use volume::{Mounted, NoVolume, Range, mount};
