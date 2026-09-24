//! Host-only: nothing here runs on Redoubt. The crate exists for its tests
//! (`tests/vendored.rs`), which prove two things about `vendor/` (vendor/README.md):
//!
//! - every vendored file is byte for byte what `vendor/SHA256SUMS` records, and nothing has been added or
//!   removed;
//! - `Cargo.lock` and `cargo metadata` build the vendored copies, through the root manifest's
//!   `[patch.crates-io]` paths into `vendor/`, and no registry copy of them.
//!
//! That is integrity since vendoring, not provenance: `SHA256SUMS` is generated from the tree.
//! That the tree is what crates.io published is checked by `provenance.sh`, which needs the
//! network and is run in the review of any change to `vendor/` (vendor/README.md).
//!
//! It depends on `smoltcp` with exactly `ipd`'s features so the lockfile resolves the vendored
//! stack whether or not `ipd` is being built. The library is `no_std`, so building it for
//! both RISC-V targets builds that stack for both widths (the `vendor-build` bench case).

#![no_std]

pub use smoltcp;
