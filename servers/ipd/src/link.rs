//! The link: smoltcp's [`Device`] over `netd` (servers/netd.md, "Serving `ipd`").
//!
//! **Receiving** is one slot. `netd` pushes each frame as a `send` on the ingress badge; the
//! server puts it here ([`Link::arrive`]) and has the stack process it at once, so a frame never
//! waits behind another and nothing queues without bound: a frame that arrives while the slot is
//! full replaces nothing and is dropped, as a full wire drops.
//!
//! **Transmitting** is a call to `netd` through a [`Netif`], one frame per call. `busy` and a
//! timeout drop the frame (TCP sends it again); only `failed`, which `netd` answers for good once
//! its device has lied, puts the link down ([`Link::is_down`]): `ipd` then answers `unreachable`
//! and asks `netd` again with backoff; `ipd` never exits on a link fault.

use alloc::vec::Vec;

use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::time::Instant;

/// The largest frame `netd` carries: 14 bytes of header and an MTU of 1500 (servers/netd.md).
pub const MAX_FRAME: usize = 1514;
/// The smallest: a bare Ethernet header.
pub const MIN_FRAME: usize = 14;
/// The MTU `ipd` builds packets for.
pub const MTU: usize = 1500;

/// Why a frame did not go out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkFault {
    /// `netd`'s transmit ring is full, or it did not answer in time: this frame is dropped.
    Dropped,
    /// `netd` answers `failed`: its device is broken, and the link is down until `info` works.
    Down,
}

/// What `ipd` needs from `netd`.
pub trait Netif {
    /// Sends one Ethernet frame (`MIN_FRAME..=MAX_FRAME` bytes).
    fn transmit(&mut self, frame: &[u8]) -> Result<(), LinkFault>;
}

/// The link as smoltcp sees it: one received frame at a time, and transmits through a [`Netif`].
pub struct Link<N> {
    netif: N,
    rx: Option<Vec<u8>>,
    down: bool,
    /// Frames sent, dropped on the way out, and received frames dropped for want of the slot or
    /// for their length.
    pub sent: u64,
    pub dropped_out: u64,
    pub dropped_in: u64,
}

impl<N: Netif> Link<N> {
    pub fn new(netif: N) -> Link<N> {
        Link { netif, rx: None, down: false, sent: 0, dropped_out: 0, dropped_in: 0 }
    }

    pub fn netif(&mut self) -> &mut N { &mut self.netif }

    /// A frame from `netd`: kept for the stack's next ingress, if it is a length `netd` carries and
    /// the slot is free. `false` if it was dropped.
    pub fn arrive(&mut self, frame: &[u8]) -> bool {
        if self.rx.is_some() || !(MIN_FRAME..=MAX_FRAME).contains(&frame.len()) {
            self.dropped_in += 1;
            return false;
        }
        let mut copy = Vec::new();
        if copy.try_reserve_exact(frame.len()).is_err() {
            self.dropped_in += 1;
            return false;
        }
        copy.extend_from_slice(frame);
        self.rx = Some(copy);
        true
    }

    /// The frame waiting for the stack, if any.
    pub fn pending(&self) -> Option<&[u8]> { self.rx.as_deref() }

    /// Forgets the waiting frame (the stack did not take it).
    pub fn discard(&mut self) {
        if self.rx.take().is_some() {
            self.dropped_in += 1;
        }
    }

    /// Whether `netd` has said its device is broken.
    pub fn is_down(&self) -> bool { self.down }

    /// `netd` answered `info` again: the link is up.
    pub fn up(&mut self) { self.down = false; }
}

impl<N: Netif> Device for Link<N> {
    type RxToken<'a>
        = RxToken
    where
        Self: 'a;
    type TxToken<'a>
        = TxToken<'a, N>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(RxToken, TxToken<'_, N>)> {
        let frame = self.rx.take()?;
        Some((RxToken(frame), TxToken { link: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<TxToken<'_, N>> {
        if self.down {
            return None;
        }
        Some(TxToken { link: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = MAX_FRAME;
        // One frame per `netd` call: smoltcp is told there is no burst to fill.
        caps.max_burst_size = Some(1);
        caps
    }
}

/// A received frame, handed to smoltcp once.
pub struct RxToken(Vec<u8>);

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

/// Room for one frame, sent to `netd` when smoltcp has written it.
pub struct TxToken<'a, N> {
    link: &'a mut Link<N>,
}

impl<N: Netif> phy::TxToken for TxToken<'_, N> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let link = self.link;
        // smoltcp never asks for more than the MTU it was told ([`MAX_FRAME`], header included).
        // Were it to, the frame is built where it fits and dropped, never cut short.
        if len > MAX_FRAME {
            let mut big = alloc::vec![0u8; len];
            link.dropped_out += 1;
            return f(&mut big);
        }
        let mut frame = [0u8; MAX_FRAME];
        let result = f(&mut frame[..len]);
        if link.down || len < MIN_FRAME {
            link.dropped_out += 1;
            return result;
        }
        match link.netif.transmit(&frame[..len]) {
            Ok(()) => link.sent += 1,
            Err(LinkFault::Dropped) => link.dropped_out += 1,
            Err(LinkFault::Down) => {
                link.dropped_out += 1;
                link.down = true;
            }
        }
        result
    }
}
