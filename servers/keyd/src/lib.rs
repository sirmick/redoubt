//! `keyd`: the process that holds every private key on the box. It signs; it never exports.
//!
//! The design is servers/keyd.md; the timing rule is TENETS.md, "Side channels". What is here:
//!
//! - [`keys`]: the keys, where they come from (the boot manifest, one per argument) and what a *purpose* is —
//!   the one message shape a badge may ask for.
//! - [`ssh`]: the SSH exchange hash `keyd` computes for itself, which is the session identifier.
//! - [`sha256`]: the hash that needs.
//! - [`server`]: the typed protocol, with `admit` and `check` on every request.
//!
//! Three properties are structural rather than checked, which is the point of them:
//!
//! 1. **There is no export.** No operation in the protocol returns a private key, or any function of one but
//!    a signature; and none adds, replaces or removes a key. A caller cannot ask for something the protocol
//!    cannot say.
//! 2. **No caller chooses the bytes that are signed.** Every signature `keyd` makes is over exactly
//!    32 bytes, and those 32 bytes are always a digest `keyd` computed itself: an SSH exchange hash
//!    over a transcript naming `keyd`'s own public key, or the SHA-256 of a fixed domain string, a
//!    length and the record. So a badge that leaks is not a signature oracle (servers/keyd.md R44),
//!    and no container that covers longer messages — a boot bundle's tar, a package, an SSH
//!    user-authentication request — can be what a `keyd` signature covers. What a badge *does* give
//!    its holder is the purpose it names: an `ssh_host` badge speaks as the box in a key exchange,
//!    which is what it is for.
//! 3. **No key here authenticates a person to the box.** There is no enrolment operation at all —
//!    keys come only from the signed manifest — and no purpose that signs an SSH
//!    user-authentication request. The check that the manifest does not list one key as both a
//!    principal's login key and a `keyd` key belongs to `init`, which reads both lists
//!    (servers/init.md R35); `holds` is the operation with which it asks.
//!
//! **No `unsafe`.** The crate forbids it outright.
//!
//! Stated residual: the seeds arrive in the read-only startup page `init` wrote, so they exist
//! in `init`'s memory and in the boot bundle, and `keyd` cannot erase its copy (the page is not
//! writable). Milestone 1 has no storage `keyd` could seal them in; this is the same trust as
//! the bundle itself, which verified boot already authenticates.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod keys;
pub mod server;
pub mod sha256;
pub mod ssh;

pub use keys::{Keys, Purpose};
pub use server::{BUDGET, COST, KeyServer, LIMITS};
