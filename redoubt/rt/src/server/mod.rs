//! The shared server library (CONTAINMENT.md): what every system server serving more than one
//! account links, so that admission and the label check are written once.
//!
//! - [`admit`]: per-account limits on what a client holds in the server.
//! - [`check`]: no read up, no write down.
//! - [`ninep`]: a 9P2000 server skeleton that applies both, and keeps `..` inside a fid's root.
//! - [`typed`]: typed-message dispatch over the generated codecs.

pub mod admit;
pub mod label;
pub mod ninep;
pub mod typed;

pub use admit::{Admission, AdmitKey, Limits, Refused, Resource};
pub use label::{Access, Denied, check};
