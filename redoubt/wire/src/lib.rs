//! Redoubt's wire formats (planning/redoubt/WIRE.md), shared by every server:
//!
//! - [`ninep`]: plain 9P2000 with a fixed `msize` of 64 KiB;
//! - [`typed`]: the typed-message framing, used by the codecs in [`proto`] that
//!   `redoubt-wire-gen` generates from the owning servers' tables;
//! - [`json`]: the strict JSON (I-JSON) profile for files people write.
//!
//! All three parse untrusted bytes. The rules they share: no `unsafe`, no panics (every
//! slice access is checked, every length is bounded by the input it claims to describe),
//! no unbounded loops, and strict decoding, so that a value has exactly one encoding.
//! 9P and typed messages decode into borrowed views of the input and encode into a
//! caller-supplied buffer, so they need no allocator; only JSON allocates.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod codec;
pub mod json;
pub mod ninep;
pub mod proto;
pub mod typed;

pub use codec::Error;

/// The 9P `msize` and the largest typed-message buffer: `MAX_LEND_PAGES` (16) pages of
/// 4 KiB (KERNEL-SPEC.md, WIRE.md).
pub const MSIZE: usize = 64 * 1024;
