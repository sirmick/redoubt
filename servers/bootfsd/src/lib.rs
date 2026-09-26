//! `bootfsd`: the read-only 9P server over the verified boot bundle, mounted at `/boot`
//! (servers/bootfsd.md; servers/init.md, "What `/boot` shows").
//!
//! **It serves exactly the entries the boot manifest's `public` list names, and never the
//! manifest.** That is not a filter it applies: `bootfsd` never sees the bundle. `init` reads
//! the archive and hands over the public entries' bytes, one at a time, through the `bootfs`
//! protocol (servers/bootfsd.md's table, generated into [`redoubt_rt::wire::proto::bootfs`]), so
//! the manifest — which carries `keyd`'s seeds and every principal's keys — never enters this
//! process's address space at all (servers/bootfsd.md R46). A walk to any other name is "does
//! not exist", the same answer as a name the bundle never held, so `/boot` says nothing about
//! the rest of the bundle.
//!
//! **What it refuses.** Everything that writes: `Twrite`, `Tcreate`, `Tremove`, `Twstat`, and
//! opening for writing or with `OTRUNC`. There is no code here that can change an entry after
//! `seal`, so a compromised client reaches nothing but the bytes `init` already published.
//!
//! **Setup.** Its arguments are the `public` list, one name per argument, in the manifest's
//! order ([`BootFs::new`]); the bytes follow as `add` messages, and `seal` ends setup. Before
//! `seal` the directory is empty and every walk is "does not exist", so no client can see a
//! half-written entry; after it, `add` and `seal` are refused for good. Both are refused from
//! any connection `new_connection` minted, so only the holder of the founding handle can fill
//! `/boot`.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod server;

pub use server::{BUDGET, BootFs, COST, LIMITS, MAX_BYTES, MAX_ENTRIES, SetupError};
