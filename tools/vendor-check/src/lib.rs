//! Host-only: nothing here runs on Redoubt. The crate exists for its tests
//! (`tests/vendored.rs`), which prove two things about `vendor/` (vendor/README.md):
//!
//! - every vendored file is byte for byte what crates.io published (`vendor/SHA256SUMS`), and
//!   nothing has been added or removed;
//! - `Cargo.lock` builds the vendored copies, through the root manifest's `[patch.crates-io]`
//!   paths, and no registry copy of them.
//!
//! It depends on `smoltcp` with exactly `ipd`'s features so the lockfile resolves the vendored
//! stack whether or not `ipd` is being built. The library is `no_std`, so building it for
//! both RISC-V targets builds that stack for both widths (the `vendor-build` bench case).

#![no_std]

pub use smoltcp;
