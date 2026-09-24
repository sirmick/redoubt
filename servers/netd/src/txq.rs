//! The transmit queue: up to sixteen frames with the device at once, reclaimed when the next one
//! is sent.
//!
//! A transmit's descriptor carries **exactly its header and frame** (`12 + len` bytes), never
//! the slot: a device reads only what it is told to, so no byte of an earlier frame, which may
//! have been another client's traffic to another host, goes out behind a later one. A slot the
//! device keeps longer than [`TX_TIMEOUT_US`] ends the device, as a late completion would be an
//! entry for a buffer `netd` no longer believes is outstanding.

use crate::ring::{self, AVAIL_F_NO_INTERRUPT, QUEUE_SIZE, SLOT_LEN, slot_off};
use crate::transport::Transport;
use crate::virtio::{DeviceError, MAX_FRAME, MIN_FRAME, NET_HDR_LEN, TX_QUEUE, TX_TIMEOUT_US};

/// What [`TxQueue::transmit`] did with a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// The frame is on the ring.
    Queued,
    /// Every slot is with the device: the frame was not sent. The caller drops it, as a full
    /// wire does; TCP sends it again.
    Busy,
    /// The frame is shorter than [`MIN_FRAME`] or longer than [`MAX_FRAME`]: not sent.
    BadLength,
}

/// The transmit queue's driver-side state. Every counter is `netd`'s own.
#[derive(Clone, Copy, Debug, Default)]
pub struct TxQueue {
    next_avail: u16,
    next_used: u16,
    /// Bit `i`: slot `i` is with the device.
    outstanding: u16,
    /// When each outstanding slot was offered, in microseconds since boot.
    offered_at: [u64; QUEUE_SIZE as usize],
    pub sent: u64,
    pub busy: u64,
}

impl TxQueue {
    pub const fn new() -> TxQueue {
        TxQueue {
            next_avail: 0,
            next_used: 0,
            outstanding: 0,
            offered_at: [0; QUEUE_SIZE as usize],
            sent: 0,
            busy: 0,
        }
    }

    /// Points queue 1 at this region's rings (§4.2.3.2).
    pub fn configure(&mut self, t: &impl Transport) -> Result<(), DeviceError> {
        ring::configure(t, TX_QUEUE)?;
        *self = TxQueue::new();
        Ok(())
    }

    /// Slots currently with the device.
    pub fn outstanding(&self) -> u32 { self.outstanding.count_ones() }

    /// Takes back every slot the device has finished with, checking each entry.
    pub fn reclaim(&mut self, t: &impl Transport) -> Result<(), DeviceError> {
        let fresh = ring::used_since(t, self.next_used, self.outstanding)?;
        for _ in 0..fresh {
            let (id, len) = ring::used_entry(t, self.next_used)?;
            let id = u16::try_from(id).ok().filter(|id| *id < QUEUE_SIZE).ok_or(DeviceError::Lie)?;
            // A transmit buffer is read by the device, not written: its used length is what it
            // wrote, which can be no more than the slot.
            if self.outstanding & (1 << id) == 0 || len as usize > SLOT_LEN {
                return Err(DeviceError::Lie);
            }
            self.outstanding &= !(1 << id);
            self.next_used = self.next_used.wrapping_add(1);
        }
        Ok(())
    }

    /// Puts `frame` on the ring, behind a zeroed header, and tells the device.
    pub fn transmit(&mut self, t: &impl Transport, frame: &[u8]) -> Result<Sent, DeviceError> {
        if !(MIN_FRAME..=MAX_FRAME).contains(&frame.len()) {
            return Ok(Sent::BadLength);
        }
        self.reclaim(t)?;
        let now = t.now_us();
        for id in 0..QUEUE_SIZE {
            if self.outstanding & (1 << id) != 0 && now.saturating_sub(self.offered_at[usize::from(id)]) >= TX_TIMEOUT_US
            {
                return Err(DeviceError::Timeout);
            }
        }
        let Some(id) = (0..QUEUE_SIZE).find(|id| self.outstanding & (1 << id) == 0) else {
            self.busy += 1;
            return Ok(Sent::Busy);
        };
        let at = slot_off(id);
        t.dma_write(at, &[0; NET_HDR_LEN])?;
        t.dma_write(at + NET_HDR_LEN, frame)?;
        // Exactly the header and the frame; the device reads nothing else of the slot.
        ring::write_desc(t, id, (NET_HDR_LEN + frame.len()) as u32, 0)?;
        ring::offer(t, self.next_avail, id)?;
        self.next_avail = self.next_avail.wrapping_add(1);
        self.outstanding |= 1 << id;
        self.offered_at[usize::from(id)] = now;
        ring::publish(t, TX_QUEUE, AVAIL_F_NO_INTERRUPT, self.next_avail)?;
        self.sent += 1;
        Ok(Sent::Queued)
    }
}
