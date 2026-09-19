//! `keyd`: the process that holds every private key on the box. It signs; it never exports.
//!
//! The design is INIT.md (keyd), CAPABILITIES.md (the powerbox and approvals, agents 7) and
//! CONTAINMENT.md (covert and timing channels). What is here:
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
//! 2. **No caller chooses the bytes that are signed.** Each purpose fixes them: an SSH exchange hash `keyd`
//!    computed, over a transcript naming `keyd`'s own public key; or an audit record under a fixed domain
//!    string, with its length. So a badge that leaks is not a signature oracle (answer 95).
//! 3. **No key here authenticates a person to the box.** There is no enrolment operation at all — keys come
//!    only from the signed manifest — and no purpose that signs an SSH user-authentication request. The check
//!    that the manifest does not list one key as both a principal's login key and a `keyd` key belongs to
//!    `init`, which reads both lists (BUILD-PLAN.md, WP-R3); `holds` is the operation with which it asks.
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
