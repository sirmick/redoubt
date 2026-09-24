//! The layout of one DMA region, which holds one split virtqueue (virtio 1.2, §2.7) and its
//! sixteen packet slots, and the steps both queues share.
//!
//! # The whole of what the device can reach
//! Each region is one contiguous run of [`REGION_PAGES`] pages from `dma_alloc`: the descriptor
//! table, the available ring and the used ring on the first page, then [`QUEUE_SIZE`] slots of
//! [`SLOT_LEN`] bytes. **Every address `netd` gives the device is `dma_phys() + <a constant
//! here>`**: descriptor *i* always names slot *i*, and the queue's three base registers name the
//! rings. Nothing else of `netd`'s memory is ever named to the device.
//!
//! # Nothing the device writes is believed
//! The descriptor table and the available ring are write-only: `netd` fills them from constants
//! and its own counters and never reads them back. Of the used ring it reads the index and, at its
//! own counter's slot, each entry's id and length, and checks each ([`crate::rxq`],
//! [`crate::txq`]).

use crate::transport::Transport;
use crate::virtio::{DeviceError, reg};

/// Descriptors in each queue, and slots in each region. A power of two (§2.7).
pub const QUEUE_SIZE: u16 = 16;

/// `VIRTQ_DESC_F_WRITE`: the device writes this buffer.
pub const DESC_F_WRITE: u16 = 2;
/// `VIRTQ_AVAIL_F_NO_INTERRUPT`: the driver does not want an interrupt when this queue's buffers
/// are used. A hint the device may ignore; `netd` asks it only of the transmit queue, which it
/// reclaims when it next transmits.
pub const AVAIL_F_NO_INTERRUPT: u16 = 1;

const DESC_BYTES: usize = 16;
const USED_ELEM_BYTES: usize = 8;

pub const DESC_OFF: usize = 0;
/// The available ring: `flags`, `idx`, `ring[QUEUE_SIZE]`, `used_event`.
pub const AVAIL_OFF: usize = DESC_OFF + DESC_BYTES * QUEUE_SIZE as usize;
pub const AVAIL_IDX_OFF: usize = AVAIL_OFF + 2;
pub const AVAIL_RING_OFF: usize = AVAIL_OFF + 4;
const AVAIL_BYTES: usize = 6 + 2 * QUEUE_SIZE as usize;
/// The used ring: `flags`, `idx`, `ring[QUEUE_SIZE]`, `avail_event`. Four-byte aligned (§2.7).
pub const USED_OFF: usize = (AVAIL_OFF + AVAIL_BYTES + 3) & !3;
pub const USED_IDX_OFF: usize = USED_OFF + 2;
pub const USED_RING_OFF: usize = USED_OFF + 4;
const USED_BYTES: usize = 6 + USED_ELEM_BYTES * QUEUE_SIZE as usize;

const PAGE: usize = 4096;

/// The slots start on their own page, so a device writing past a slot lands in slots `netd`
/// already treats as hostile rather than on the rings.
pub const SLOTS_OFF: usize = PAGE;
/// One slot: the 12-byte header and a frame of up to 1514 bytes, rounded up.
pub const SLOT_LEN: usize = 2048;

/// Pages in one region.
pub const REGION_PAGES: usize = (SLOTS_OFF + SLOT_LEN * QUEUE_SIZE as usize).div_ceil(PAGE);
/// One region's length in bytes.
pub const REGION_LEN: usize = REGION_PAGES * PAGE;

/// Everything the layout claims, checked where a mistake cannot ship.
pub const LAYOUT_FITS: () = {
    assert!(QUEUE_SIZE.is_power_of_two());
    assert!(DESC_OFF.is_multiple_of(16) && AVAIL_OFF.is_multiple_of(2) && USED_OFF.is_multiple_of(4));
    assert!(USED_OFF + USED_BYTES <= SLOTS_OFF);
    assert!(SLOTS_OFF + SLOT_LEN * QUEUE_SIZE as usize <= REGION_LEN);
    assert!(crate::virtio::NET_HDR_LEN + crate::virtio::MAX_FRAME <= SLOT_LEN);
};

/// Slot `i`'s offset in the region.
pub const fn slot_off(i: u16) -> usize { SLOTS_OFF + i as usize * SLOT_LEN }

/// `phys + off`, refusing the overflow rather than wrapping a physical address.
pub fn phys_of(phys: u64, off: usize) -> Result<u64, DeviceError> {
    phys.checked_add(off as u64).ok_or(DeviceError::Queue)
}

/// Writes descriptor `i`: slot `i`, `len` bytes, `flags`. Written whole every time, so whatever
/// the device did to the table since is overwritten before the descriptor is offered again.
pub fn write_desc(t: &impl Transport, i: u16, len: u32, flags: u16) -> Result<(), DeviceError> {
    let at = DESC_OFF + usize::from(i) * DESC_BYTES;
    t.dma_write_u64(at, phys_of(t.dma_phys(), slot_off(i))?)?;
    t.dma_write_u32(at + 8, len)?;
    t.dma_write_u16(at + 12, flags)?;
    t.dma_write_u16(at + 14, 0)?;
    Ok(())
}

/// One used-ring entry, read at `netd`'s own counter: the device's id and length, not yet
/// believed.
pub fn used_entry(t: &impl Transport, counter: u16) -> Result<(u32, u32), DeviceError> {
    let at = USED_RING_OFF + usize::from(counter % QUEUE_SIZE) * USED_ELEM_BYTES;
    Ok((t.dma_read_u32(at)?, t.dma_read_u32(at + 4)?))
}

/// How many used entries the device has published since `next_used`, refused if more than
/// `outstanding` of this queue's buffers are with it: a jump, a step backwards (which wraps to a
/// large number) or completions for buffers never offered.
pub fn used_since(t: &impl Transport, next_used: u16, outstanding: u16) -> Result<u16, DeviceError> {
    t.fence();
    let idx = t.dma_read_u16(USED_IDX_OFF)?;
    let fresh = idx.wrapping_sub(next_used);
    if fresh > outstanding.count_ones() as u16 {
        return Err(DeviceError::Lie);
    }
    Ok(fresh)
}

/// Publishes available-ring entries up to `next_avail`, and tells the device (§2.7.13).
pub fn publish(t: &impl Transport, queue: u32, flags: u16, next_avail: u16) -> Result<(), DeviceError> {
    // `avail.flags` is written with every publish, so a device that rewrote it cannot leave the
    // queue's interrupt setting behind it.
    t.dma_write_u16(AVAIL_OFF, flags)?;
    // The entries must be visible before the index that publishes them (§2.7.13.3).
    t.fence();
    t.dma_write_u16(AVAIL_IDX_OFF, next_avail)?;
    t.fence();
    t.reg_write(reg::QUEUE_NOTIFY, queue)?;
    Ok(())
}

/// Offers descriptor `id` in available-ring slot `next_avail` (not yet published).
pub fn offer(t: &impl Transport, next_avail: u16, id: u16) -> Result<(), DeviceError> {
    let slot = usize::from(next_avail % QUEUE_SIZE);
    t.dma_write_u16(AVAIL_RING_OFF + slot * 2, id)?;
    Ok(())
}

/// Points the device's queue `queue` at this region's rings and makes it ready (§4.2.3.2).
///
/// `QueueNumMax` is the one number the device chooses here, and it is only compared: a device
/// offering fewer than [`QUEUE_SIZE`] descriptors is refused, and one offering more is told to use
/// [`QUEUE_SIZE`] anyway.
pub fn configure(t: &impl Transport, queue: u32) -> Result<(), DeviceError> {
    let () = LAYOUT_FITS;
    if t.dma_len() < REGION_LEN {
        return Err(DeviceError::Queue);
    }
    t.reg_write(reg::QUEUE_SEL, queue)?;
    if t.reg_read(reg::QUEUE_NUM_MAX)? < u32::from(QUEUE_SIZE) {
        return Err(DeviceError::Queue);
    }
    if t.reg_read(reg::QUEUE_READY)? != 0 {
        // A queue already live is a device that did not reset.
        return Err(DeviceError::Queue);
    }
    t.reg_write(reg::QUEUE_NUM, u32::from(QUEUE_SIZE))?;
    let phys = t.dma_phys();
    for (low, high, off) in [
        (reg::QUEUE_DESC_LOW, reg::QUEUE_DESC_HIGH, DESC_OFF),
        (reg::QUEUE_DRIVER_LOW, reg::QUEUE_DRIVER_HIGH, AVAIL_OFF),
        (reg::QUEUE_DEVICE_LOW, reg::QUEUE_DEVICE_HIGH, USED_OFF),
    ] {
        let addr = phys_of(phys, off)?;
        t.reg_write(low, addr as u32)?;
        t.reg_write(high, (addr >> 32) as u32)?;
    }
    // Both rings start from values this driver wrote, not ones it relies on the kernel or the
    // device's reset to have left.
    t.dma_zero(DESC_OFF, SLOTS_OFF)?;
    t.fence();
    t.reg_write(reg::QUEUE_READY, 1)?;
    if t.reg_read(reg::QUEUE_READY)? != 1 {
        return Err(DeviceError::Queue);
    }
    Ok(())
}
