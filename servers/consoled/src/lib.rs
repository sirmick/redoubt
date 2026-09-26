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
//! **Until `init` starts it, `log-server` keeps the UART.** Nothing starts `consoled` yet: it
//! takes its device handles from a startup block, and no `init` writes one
//! (docs/plan/m1-separation.md, step 3). So the bench's console is still `log-server`, which
//! holds the same ns16550 and the same interrupt because the kernel hands every device to the
//! bundle's first program (kernel/devices.md), and this crate is only built and host tested.
//! **The two must never run together**: two holders of one device handle both reach the
//! registers (kernel/devices.md, "Authority"), and two readers of one receive FIFO would each
//! take half the line. What separates them is that only one of them is ever started — today
//! `log-server`, and once `init` starts the servers `consoled`, which the manifest gives the
//! UART's handles and whose arrival is what retires `log-server`'s console duties.
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

pub use server::{BUDGET, COST, Console, LIMITS, MAX_INPUT};
pub use uart::Uart;
