//! beamlet: a small BEAM interpreter in safe Rust.
//!
//! It runs `.beam` files produced by one pinned Erlang/OTP compiler (see `DESIGN.md`). Security,
//! auditability and simplicity come first; speed and completeness come later, if at all.
//!
//! Map of the crate, in reading order:
//! - [`term`]: Erlang values. [`atom`]: interned atoms.
//! - [`platform`]: the whole interface to the host OS.
//! - [`loader`]: `.beam` parsing and validation; [`etf`]: the external term format.
//! - [`vm`]: modules, processes, scheduling. [`interp`]: the instruction loop.
//! - [`bif`]: native functions. [`bits`]: bitstring building and reading.

#![no_std]
#![forbid(unsafe_code)]
// Terms used as map keys contain a `Cell` only inside match states, which the compiler never
// lets code use as values, so their ordering cannot change while they are in a map.
#![allow(clippy::mutable_key_type)]

extern crate alloc;

pub mod atom;
pub mod bif;
pub mod bits;
pub mod etf;
pub mod ets;
pub mod float;
#[allow(dead_code)]
pub mod heap;
pub mod interp;
pub mod loader;
pub mod memory;
pub mod module;
pub mod opcodes;
pub mod platform;
pub mod pmap;
pub mod process;
pub mod term;
pub mod vm;

pub use platform::Platform;
pub use process::{Class, Exception};
pub use term::Term;
pub use vm::Vm;
