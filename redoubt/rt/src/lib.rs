//! The native runtime for Redoubt programs: everything a `no_std` + `alloc` Rust server or
//! program needs between the ABI (`redoubt-sys`) and its own logic.
//!
//! - [`startup`]: the startup block a parent writes (INIT.md), parsed defensively.
//! - [`handle`]: typed handles and the system calls that are not IPC.
//! - [`ipc`]: lends and transfers, `call`, `send`, `receive`, `reply`.
//! - [`heap`]: the global allocator, over `map_anon`.
//! - [`start`]: the entry point ([`entry!`]), exit codes and the panic handler.
//! - [`path`]: lexical path cleaning, so `..` never climbs above a root.
//! - [`client`]: a small synchronous 9P client.
//! - [`server`]: the shared server library (CONTAINMENT.md): `admit`, `check`, the 9P server skeleton and
//!   typed-message dispatch.
//!
//! Every system call goes through one function in `sys.rs`. On the machine (`target_os =
//! "none"`) that is the `ecall`; on the host it is a [`HostKernel`] a test installs, so the whole
//! crate, and programs built on it, run in host tests against a fake kernel.
//!
//! Handles are `u32` indices and 64-bit values (ids, badges, accounts, time) are `u64` on both
//! widths; nothing here depends on the machine's word size.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod client;
pub mod handle;
pub mod heap;
pub mod ipc;
pub mod path;
pub mod server;
pub mod start;
pub mod startup;
mod sys;

pub use redoubt_sys as abi;
pub use redoubt_wire as wire;
#[cfg(target_os = "none")]
pub use start::start;
pub use start::{exit, init};
#[cfg(not(target_os = "none"))]
pub use sys::{HostKernel, install_host_kernel};

/// On the machine, the heap is every program's global allocator.
#[cfg(target_os = "none")]
#[global_allocator]
static HEAP: heap::Heap = heap::Heap::new();
