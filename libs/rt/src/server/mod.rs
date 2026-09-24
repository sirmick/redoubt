//! The shared server library (CONTAINMENT.md): what every system server serving more than one
//! account links, so that admission and the label check are written once.
//!
//! - [`admit`]: per-(account, label set) limits on what a client holds in the server, with a fair share per
//!   badge.
//! - [`check`]: no read up, no write down.
//! - [`ninep`]: a 9P2000 server skeleton that applies both, and keeps `..` inside a fid's root.
//! - [`minted`]: the capabilities a server mints for its clients, and their release.
//! - [`typed`]: typed-message dispatch over the generated codecs.
//! - [`parked`]: calls held open for later, each with a deadline, resumed under `serve`.

pub mod admit;
pub mod label;
pub mod minted;
pub mod ninep;
pub mod parked;
pub mod typed;

pub use admit::{Admission, AdmitKey, Cost, Limits, Override, Refused, Resource, Unsized};

/// The reply words of a malformed request, in 9P calls and every typed protocol alike: status 1,
/// `Malformed` (answers 41 and 42), which the wire generator reserves in every error table.
pub const MALFORMED: crate::ipc::Words = redoubt_wire::typed::error_reply(redoubt_wire::typed::MALFORMED);
pub use label::{Access, Denied, check};
