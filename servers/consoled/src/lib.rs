//! `consoled`: the ns16550 UART driver, serving `/dev/cons` over 9P (servers/consoled.md).
//!
//! - [`uart`]: the device. It takes its registers through `map_device` and reaches them through the runtime's
//!   bounds-checked [`redoubt_rt::handle::Registers`], so there is no `unsafe` here; the crate forbids it
//!   outright.
//! - [`server`]: the file behind the 9P skeleton. A write goes out of the UART; a read that has nothing to
//!   return **parks its call** rather than blocking the server ([`redoubt_rt::server::parked`]), and is
//!   served again, unchanged, when a key arrives.
//! - `src/bin/consoled.rs`: the two threads. One waits on the IRQ handle and says only "a byte arrived"; the
//!   other owns the UART, serves 9P and answers the parked reads.
//!
//! **What it refuses.** Opening with `OEXEC` or `OTRUNC` (meaningless on a console), walking
//! (there is nothing below `/dev/cons`), creating and removing (the skeleton's defaults), and —
//! through `check`, since the physical console carries no labels — a write from a labelled
//! caller: no write down onto a screen someone else is looking at.
//!
//! **Every line says who wrote it.** `init` starts it with the UART `init` has unmapped, attaches
//! through a root badge of its own, and mints each child's `/dev/cons` from it: a line written
//! through a minted connection starts with `[con N] `, its id, and only `init`'s lines are bare
//! ([`server::Lines`]; servers/consoled.md, "Started by `init`"). In the bench's other cases
//! `log-server` holds the UART instead; the two are never started together, since two holders of
//! one device would both reach its registers (kernel/devices.md, "Authority").
//!
//! **Stated residual: one line, one queue.** There is one physical keyboard, so there is one
//! input queue, and a byte goes to whichever connection has waited longest. A client that reads
//! `/dev/cons` therefore takes input another client might have been waiting for. That is what
//! sharing a console means; a session that must not share one gets its own (`sshd` serves one
//! per channel, servers/sshd.md), not a second reader here.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod server;
pub mod uart;

pub use server::{BUDGET, COST, Console, Lines, MAX_INPUT, PREFIX_LEN, limits, prefix};
pub use uart::Uart;
