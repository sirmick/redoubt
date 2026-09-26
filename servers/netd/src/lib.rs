//! `netd`: the virtio-net driver. It owns one network device and serves its frames to one
//! `ipd` (servers/netd.md).
//!
//! # What this is trusted for
//! On QEMU no hardware confines DMA, so `netd` is inside the TCB (TENETS.md 7), as `blkd` is.
//! No code here can make a device that ignores its addresses harmless. What it is written to
//! guarantee is the rest, stated so that it can be checked:
//!
//! 1. **`netd` never asks the device to touch anything but the pages `dma_alloc` gave it.** It has two
//!    regions, one per queue, and every address it writes into a descriptor or a queue base register is a
//!    region's physical base plus a constant of [`ring`] ([`ring::LAYOUT_FITS`]). Descriptor *i* always names
//!    slot *i*. A client's lend never reaches the device: a transmit is copied into a slot, and a received
//!    frame is copied out.
//! 2. **Nothing the device says can corrupt `netd`'s memory, panic it or make it lie to `ipd`.** No device
//!    value is used as an index or a length until it has been checked against what `netd` gave the device.
//!    The descriptor tables and available rings are written, never read back. A device that breaks the
//!    protocol is reset and never trusted again.
//! 3. **Nothing a network sender puts on the wire can stop it.** A frame of a length `netd` does not carry is
//!    dropped and counted, never taken for a device's lie ([`rxq`]).
//! 4. **No address travels in a message.** Both regions are allocated, both queues configured and the device
//!    started by the serving thread before the receive thread exists. The receive thread's half ([`RxPart`])
//!    is handed over in this process's own memory ([`kernel`]).
//! 5. **A DMA page never leaves `netd`** (kernel/devices.md, `dma_alloc`: the kernel refuses to
//!    lend, transfer or `process_map` a `dma_alloc` page). No path here lends, transfers or maps
//!    one: each received frame is copied into a fresh one-page `Buffer` of anonymous memory before
//!    it is sent to `ipd`, a transmit is copied out of the caller's lend into a slot, and no reply
//!    carries a buffer. DMA addresses exist only as numbers inside [`kernel::Device`].
//! 6. **Either thread leaving its loop stops the device.** The receive thread resets it and tells
//!    the serving thread on every way out ([`receiver`]); the serving thread resets it on a lie, on
//!    a report, and before it exits; a panic resets it from the runtime's panic hook
//!    ([`kernel::Regs::arm_panic_reset`]). A kill or a fault runs none of this; the kernel resets
//!    the device then (kernel/invariants.md I16).
//!
//! # Shape
//! - [`transport`]: the seam to the kernel, and [`kernel`], its one implementation and the only `unsafe` in
//!   the crate. Host tests use [`fake::FakeNic`], a hostile device in safe Rust.
//! - [`virtio`]: the register map, the handshake, feature negotiation, the MAC.
//! - [`ring`]: one region's layout; [`rxq`] and [`txq`]: the two queues.
//! - [`device`]: bring-up. [`server`]: the `netif` protocol for `ipd`. [`receiver`]: the receive thread's
//!   loop.

#![no_std]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use redoubt_rt::handle::Endpoint;

pub mod device;
#[allow(unsafe_code)]
pub mod kernel;
pub mod receiver;
pub mod ring;
pub mod rxq;
pub mod server;
pub mod transport;
pub mod txq;
pub mod virtio;

#[cfg(not(target_os = "none"))]
pub mod fake;

pub use device::{Up, bring_up};
pub use server::NetServer;
pub use transport::{Fault, Transport};
pub use virtio::DeviceError;

/// The first badge a server mints (servers/serving.md, "Minted connections": badges at or above
/// it are minted, below it are the manifest's). `netd`'s client badge is one of `init`'s, below
/// it; the receive thread's badge is drawn above it, so the two can never be equal.
pub const FIRST_MINTED_BADGE: u64 = 1 << 63;

/// Word 0 of the receive thread's one message to the serving thread: the device lied, and the
/// receive thread has reset it and stopped. Word 0 of a `netif` call is an opcode (1 or 2), and
/// this arrives as a `send` on a badge only this process holds.
pub const BROKEN: u64 = 0xdead;

/// The receive thread's half of `netd`: its view of the device (registers, the receive region and
/// the interrupt), the receive queue that bring-up configured in that region, the handle it sends
/// frames to `ipd` on, and the one it reports a broken device on.
pub struct RxPart {
    pub device: kernel::Device,
    pub rx: rxq::RxQueue,
    pub ipd: Endpoint,
    pub broken: Endpoint,
}

/// `netd`'s arguments (servers/init.md: each server defines its own): exactly one,
/// `client=BADGE`, the badge `ipd`'s handle carries, in decimal, nonzero and below
/// [`FIRST_MINTED_BADGE`]. Anything else is refused, and `netd` does not start.
pub fn parse_client<'a>(mut args: impl Iterator<Item = &'a str>) -> Option<u64> {
    let arg = args.next()?;
    if args.next().is_some() {
        return None;
    }
    let digits = arg.strip_prefix("client=")?;
    if digits.is_empty()
        || digits.len() > 20
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.starts_with('0')
    {
        return None;
    }
    digits.parse::<u64>().ok().filter(|badge| *badge != 0 && *badge < FIRST_MINTED_BADGE)
}
