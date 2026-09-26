//! beamlet: a small BEAM interpreter in safe Rust.
//!
//! It runs `.beam` files produced by one pinned Erlang/OTP compiler (docs/userland/beamlet.md).
//! Security, auditability and simplicity come first; speed and completeness come later, if at all.
//!
//! Map of the crate, in reading order:
//! - [`term`]: Erlang values. [`atom`]: interned atoms.
//! - [`platform`]: the whole interface to the host OS.
//! - [`loader`]: `.beam` parsing and validation; [`etf`]: the external term format.
//! - [`vm`]: modules, processes, scheduling. [`interp`]: the instruction loop.
//! - [`bif`]: native functions. [`bits`]: bitstring building and reading.

#![no_std]
#![forbid(unsafe_code)]
// Without the `std` feature there is one scheduler and shared values need not be `Send`.
#![cfg_attr(not(feature = "std"), allow(clippy::arc_with_non_send_sync))]
// An `OwnedException` is large, but only returned when a process fails to start.
#![allow(clippy::result_large_err)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod atom;
pub mod bif;
pub mod bits;
pub mod etf;
pub mod ets;
pub mod float;
pub mod interp;
pub mod loader;
pub mod memory;
pub mod module;
pub mod opcodes;
pub mod platform;
pub mod process;
pub mod sched;
pub mod sync;
pub mod term;
pub mod vm;

pub use platform::Platform;
pub use process::{Class, Exception};
pub use term::Term;
pub use vm::Vm;
