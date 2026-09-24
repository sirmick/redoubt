//! The receive queue: sixteen slots always offered to the device, drained on each interrupt.
//!
//! **A lie and a bad frame are different things** (IO-ARCHITECTURE.md, `netd`). A used entry
//! that breaks the ring protocol is a lie and ends the device: the index running past what is
//! outstanding, an id out of range, not outstanding or seen twice, a length below the header or
//! above the slot, or a header asking for checksum or segmentation that was never negotiated. A
//! frame whose length is outside [`MIN_FRAME`]`..=`[`MAX_FRAME`] but inside the slot is only
//! **content**: some sender on the wire sent it (an 802.1Q-tagged frame, a runt), and an honest
//! device delivers it. It is dropped and counted, and the slot is offered again. A packet on the
//! LAN never bricks the NIC.
//!
//! Each frame is copied out of the region **once**, with `netd`'s own length, into memory of
//! `netd`'s own before any byte of it is looked at, and each slot is zeroed before it is offered
//! again, so a device that reports more than it wrote hands back zeros, never an earlier frame.

use crate::ring::{self, DESC_F_WRITE, QUEUE_SIZE, SLOT_LEN, slot_off};
use crate::transport::Transport;
use crate::virtio::{DeviceError, MAX_FRAME, MIN_FRAME, NET_HDR_LEN, RX_QUEUE};

/// The receive queue's driver-side state. Every counter is `netd`'s own.
#[derive(Clone, Copy, Debug, Default)]
pub struct RxQueue {
    next_avail: u16,
    next_used: u16,
    /// Bit `i`: slot `i` is with the device.
    outstanding: u16,
    /// Frames handed on, and frames dropped for their length.
    pub delivered: u64,
    pub dropped: u64,
}

/// One slot's bytes, where a frame is copied before it is looked at.
pub type Frame = [u8; SLOT_LEN];

impl RxQueue {
    pub const fn new() -> RxQueue {
        RxQueue { next_avail: 0, next_used: 0, outstanding: 0, delivered: 0, dropped: 0 }
    }

    /// Points queue 0 at this region's rings (§4.2.3.2).
    pub fn configure(&mut self, t: &impl Transport) -> Result<(), DeviceError> {
        ring::configure(t, RX_QUEUE)?;
        *self = RxQueue::new();
        Ok(())
    }

    /// Offers every slot to the device, without telling it yet ([`RxQueue::notify`] does, once
    /// the device is `DRIVER_OK`: the driver may not notify before, §3.1.1).
    pub fn offer_all(&mut self, t: &impl Transport) -> Result<(), DeviceError> {
        for id in 0..QUEUE_SIZE {
            self.offer(t, id)?;
        }
        t.dma_write_u16(ring::AVAIL_OFF, 0)?;
        t.fence();
        t.dma_write_u16(ring::AVAIL_IDX_OFF, self.next_avail)?;
        t.fence();
        Ok(())
    }

    /// Tells the device buffers are available.
    pub fn notify(&self, t: &impl Transport) -> Result<(), DeviceError> {
        t.reg_write(crate::virtio::reg::QUEUE_NOTIFY, RX_QUEUE)?;
        Ok(())
    }

    /// Slots currently with the device.
    pub fn outstanding(&self) -> u32 { self.outstanding.count_ones() }

    /// Zeroes slot `id`, rewrites its descriptor and puts it on the available ring.
    fn offer(&mut self, t: &impl Transport, id: u16) -> Result<(), DeviceError> {
        t.dma_zero(slot_off(id), SLOT_LEN)?;
        ring::write_desc(t, id, SLOT_LEN as u32, DESC_F_WRITE)?;
        ring::offer(t, self.next_avail, id)?;
        self.next_avail = self.next_avail.wrapping_add(1);
        self.outstanding |= 1 << id;
        Ok(())
    }

    /// Takes every frame the device has completed, hands each good one to `deliver` (out of
    /// `scratch`, `netd`'s own memory), drops and counts the rest, and offers each slot again.
    ///
    /// On a lie it stops at once and returns it; the caller resets the device and never calls
    /// this again. The counters are left where they were, so nothing after the lie is believed.
    pub fn drain(
        &mut self,
        t: &impl Transport,
        scratch: &mut Frame,
        mut deliver: impl FnMut(&[u8]),
    ) -> Result<u16, DeviceError> {
        let fresh = ring::used_since(t, self.next_used, self.outstanding)?;
        // Every entry of the batch is checked before any slot is offered again, so a buffer the
        // device completes twice in one batch is found: its bit is already clear the second time.
        let mut used = [0u16; QUEUE_SIZE as usize];
        for n in 0..usize::from(fresh) {
            let (id, len) = ring::used_entry(t, self.next_used)?;
            let id = u16::try_from(id).ok().filter(|id| *id < QUEUE_SIZE).ok_or(DeviceError::Lie)?;
            if self.outstanding & (1 << id) == 0 {
                return Err(DeviceError::Lie);
            }
            let len = len as usize;
            if !(NET_HDR_LEN..=SLOT_LEN).contains(&len) {
                return Err(DeviceError::Lie);
            }
            // Copied out once, with a length checked against the slot, before anything reads it.
            let bytes = &mut scratch[..len];
            t.dma_read(slot_off(id), bytes)?;
            // No checksum offload and no segmentation were negotiated, so the header's `flags`
            // and `gso_type` must be zero (§5.1.6.3): anything else asks `netd` for work it said
            // it would not do.
            if bytes[0] != 0 || bytes[1] != 0 {
                return Err(DeviceError::Lie);
            }
            self.outstanding &= !(1 << id);
            self.next_used = self.next_used.wrapping_add(1);
            used[n] = id;
            let frame = &bytes[NET_HDR_LEN..];
            if (MIN_FRAME..=MAX_FRAME).contains(&frame.len()) {
                self.delivered += 1;
                deliver(frame);
            } else {
                self.dropped += 1;
            }
        }
        if fresh > 0 {
            for id in &used[..usize::from(fresh)] {
                self.offer(t, *id)?;
            }
            ring::publish(t, RX_QUEUE, 0, self.next_avail)?;
        }
        Ok(fresh)
    }
}
