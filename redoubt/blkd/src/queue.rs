//! The split virtqueue (virtio 1.2, §2.7), with one request outstanding at a time.
//!
//! # The whole of what the device can reach
//! One contiguous run of [`DMA_PAGES`] pages from `dma_alloc`, laid out by the constants below:
//! the descriptor table, the available ring, the used ring, one request header, one status byte
//! and one data buffer. **Every address this driver ever writes into a descriptor is
//! `dma_phys() + <one of these constants>`**, and the only other addresses it gives the device
//! are the three queue base registers, which point at the same region. Nothing else in `blkd`'s
//! memory — its handle table, its heap, its stack, a client's lend — is ever named to the device.
//! On a platform whose hardware confines DMA (PLATFORM-FPGA.md) that is the end of it; on QEMU it
//! is a promise about what `blkd` asks for, not about what the device does (IO-ARCHITECTURE.md).
//!
//! # Nothing the device writes is believed
//! The descriptor table and the available ring are **write-only** here: they are filled from
//! constants for every request and never read back. A device that rewrites them — a `next` that
//! points out of the table, a chain that loops, a length that overflows — changes nothing this
//! driver believes, because this driver never asks them what it put there.
//!
//! Of the used ring, three values are read, and each is checked against what was sent:
//! - `idx` must be exactly one more than the last one seen. A jump, a step backwards, a stale value and an
//!   entry for a request never sent all fail that one test.
//! - the entry's `id` must be [`HEAD`], the descriptor this driver submits every time. The entry is read from
//!   the slot **this driver's own counter** names, so no device value is ever an index.
//! - `len` must not exceed the bytes the device was given to write. It is otherwise unused: the number of
//!   bytes copied back out is always this driver's own.
//!
//! Anything else ends the request with [`DeviceError::Io`], and [`crate::disk::Disk`] then
//! refuses every later request rather than keep talking to a device that has lied.

use crate::transport::Transport;
use crate::virtio::{self, DATA_LEN, DeviceError, REQUEST_TIMEOUT_US, reg};

/// Descriptors in the ring. Three is the longest chain `blkd` builds (header, data, status) and
/// the ring must be a power of two (§2.7), so four it is. A larger ring would only hold requests
/// this driver does not make: exactly one is outstanding at a time, which is what makes
/// "requests complete in order" (IO-ARCHITECTURE.md) true by construction rather than by care.
pub const QUEUE_SIZE: u16 = 4;

/// The descriptor every chain starts at. Fixed, so the used ring's `id` is compared with a
/// constant.
pub const HEAD: u32 = 0;

/// `VIRTQ_DESC_F_NEXT`: the chain continues at `next`.
const DESC_F_NEXT: u16 = 1;
/// `VIRTQ_DESC_F_WRITE`: the device writes this buffer (rather than reading it).
const DESC_F_WRITE: u16 = 2;

const DESC_BYTES: usize = 16;
const USED_ELEM_BYTES: usize = 8;

/// The descriptor table.
pub const DESC_OFF: usize = 0;
/// The available ring: `flags`, `idx`, `ring[QUEUE_SIZE]`, `used_event`.
pub const AVAIL_OFF: usize = DESC_OFF + DESC_BYTES * QUEUE_SIZE as usize;
const AVAIL_IDX_OFF: usize = AVAIL_OFF + 2;
const AVAIL_RING_OFF: usize = AVAIL_OFF + 4;
const AVAIL_BYTES: usize = 6 + 2 * QUEUE_SIZE as usize;
/// The used ring: `flags`, `idx`, `ring[QUEUE_SIZE]`, `avail_event`. Four-byte aligned (§2.7).
pub const USED_OFF: usize = (AVAIL_OFF + AVAIL_BYTES + 3) & !3;
const USED_IDX_OFF: usize = USED_OFF + 2;
const USED_RING_OFF: usize = USED_OFF + 4;
const USED_BYTES: usize = 6 + USED_ELEM_BYTES * QUEUE_SIZE as usize;

/// The virtio-blk request header: `type: u32`, `reserved: u32`, `sector: u64` (§5.2.6).
pub const HEADER_OFF: usize = (USED_OFF + USED_BYTES + 15) & !15;
const HEADER_BYTES: usize = 16;
/// The one status byte the device writes.
pub const STATUS_OFF: usize = HEADER_OFF + HEADER_BYTES;

/// The data buffer, on its own page so that a device writing past the end of it (which it cannot
/// be asked to do, but may do anyway) lands in pages this driver expects to be hostile rather
/// than on the rings it is about to read.
pub const DATA_OFF: usize = PAGE;

/// A page, which is what `dma_alloc` hands out (KERNEL-SPEC.md).
const PAGE: usize = 4096;

/// Pages in the DMA region: one for the rings, the header and the status byte, then
/// [`DATA_LEN`] bytes of data buffer.
pub const DMA_PAGES: usize = 1 + DATA_LEN.div_ceil(PAGE);

/// The region's length in bytes.
pub const DMA_LEN: usize = DMA_PAGES * PAGE;

/// Everything the layout claims, checked where a mistake cannot ship: the rings, the header and
/// the status byte fit in the first page, the data buffer fits the rest, and the alignments
/// §2.7 requires hold.
pub const LAYOUT_FITS: () = {
    assert!(QUEUE_SIZE.is_power_of_two());
    assert!(DESC_OFF.is_multiple_of(16) && AVAIL_OFF.is_multiple_of(2) && USED_OFF.is_multiple_of(4));
    assert!(STATUS_OFF < DATA_OFF);
    assert!(DATA_OFF + DATA_LEN <= DMA_LEN);
};

/// One buffer of a descriptor chain: an offset in the DMA region, a length, and which way the
/// device moves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub off: usize,
    pub len: u32,
    /// True when the **device** writes it (a read's data, every status byte).
    pub device_writes: bool,
}

/// The queue's driver-side state. Both counters are this driver's own; neither is ever read back
/// from the device.
#[derive(Clone, Copy, Debug, Default)]
pub struct Queue {
    /// The value written into `avail.idx` for the next request.
    next_avail: u16,
    /// The value `used.idx` must show when the next request completes.
    next_used: u16,
}

impl Queue {
    pub const fn new() -> Queue { Queue { next_avail: 0, next_used: 0 } }

    /// Points the device at the rings and makes queue 0 ready (§4.2.3.2).
    ///
    /// `QueueNumMax` is the one device-chosen number here, and it is only compared: a device
    /// offering fewer descriptors than one chain needs is refused, and a device offering more is
    /// told to use [`QUEUE_SIZE`] anyway.
    pub fn configure(&mut self, t: &impl Transport) -> Result<(), DeviceError> {
        let () = LAYOUT_FITS;
        if t.dma_len() < DMA_LEN {
            return Err(DeviceError::Queue);
        }
        t.reg_write(reg::QUEUE_SEL, 0)?;
        let max = t.reg_read(reg::QUEUE_NUM_MAX)?;
        if max < u32::from(QUEUE_SIZE) {
            return Err(DeviceError::Queue);
        }
        if t.reg_read(reg::QUEUE_READY)? != 0 {
            // A queue already live is a device that did not reset; talking to it would mean
            // sharing a ring with whatever was there before.
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
        // The rings start zeroed: `dma_alloc` returns zeroed pages (R11), and the device's own
        // reset put its `used.idx` back to 0, which is what `next_used` expects.
        t.dma_write_u16(AVAIL_OFF, 0)?;
        t.dma_write_u16(AVAIL_IDX_OFF, 0)?;
        self.next_avail = 0;
        self.next_used = 0;
        t.fence();
        t.reg_write(reg::QUEUE_READY, 1)?;
        if t.reg_read(reg::QUEUE_READY)? != 1 {
            return Err(DeviceError::Queue);
        }
        Ok(())
    }

    /// Writes `chain` into descriptors [`HEAD`]`..`, offers the head to the device and rings the
    /// doorbell.
    ///
    /// The chain is written from scratch every time, so whatever the device did to the table
    /// since the last request is overwritten before the next one is offered.
    pub fn submit(&mut self, t: &impl Transport, chain: &[Segment]) -> Result<(), DeviceError> {
        if chain.is_empty() || chain.len() > QUEUE_SIZE as usize {
            return Err(DeviceError::Queue);
        }
        let phys = t.dma_phys();
        for (i, segment) in chain.iter().enumerate() {
            // Every segment is inside the region, so every address handed to the device is.
            let end = segment.off.checked_add(segment.len as usize).ok_or(DeviceError::Queue)?;
            if end > t.dma_len() || segment.len == 0 {
                return Err(DeviceError::Queue);
            }
            let last = i + 1 == chain.len();
            let mut flags = 0;
            if !last {
                flags |= DESC_F_NEXT;
            }
            if segment.device_writes {
                flags |= DESC_F_WRITE;
            }
            let at = DESC_OFF + i * DESC_BYTES;
            t.dma_write_u64(at, phys_of(phys, segment.off)?)?;
            t.dma_write_u32(at + 8, segment.len)?;
            t.dma_write_u16(at + 12, flags)?;
            // `next` of the last descriptor is 0 and unread: `NEXT` is clear.
            t.dma_write_u16(at + 14, if last { 0 } else { (i + 1) as u16 })?;
        }
        let slot = usize::from(self.next_avail % QUEUE_SIZE);
        t.dma_write_u16(AVAIL_RING_OFF + slot * 2, HEAD as u16)?;
        // The descriptors must be visible before the index that publishes them (§2.7.13.3).
        t.fence();
        self.next_avail = self.next_avail.wrapping_add(1);
        t.dma_write_u16(AVAIL_IDX_OFF, self.next_avail)?;
        t.fence();
        t.reg_write(reg::QUEUE_NOTIFY, 0)?;
        Ok(())
    }

    /// Waits for the outstanding request and returns the length the device reported, having
    /// checked it against `writable`, the bytes the device was given to write.
    ///
    /// On any refusal the queue's counters are left where they were: the caller
    /// ([`crate::disk::Disk`]) marks the device broken and never submits again, so a completion
    /// that arrives afterwards is read by nobody.
    pub fn complete(&mut self, t: &impl Transport, writable: u32) -> Result<u32, DeviceError> {
        let deadline = t.now_us().saturating_add(REQUEST_TIMEOUT_US);
        loop {
            // The interrupt may have arrived (and been consumed) before this driver got here, so
            // the ring is looked at first, and again after every wake.
            t.fence();
            if t.dma_read_u16(USED_IDX_OFF)? != self.next_used {
                break;
            }
            let now = t.now_us();
            if now >= deadline {
                return Err(DeviceError::Timeout);
            }
            match t.wait_irq(deadline - now) {
                Ok(()) => {
                    // The interrupt is acknowledged whether or not the ring moved: a spurious one
                    // left unacknowledged would keep the line asserted (§4.2.2).
                    virtio::ack_interrupt(t)?;
                }
                Err(crate::transport::Fault::Timeout) => return Err(DeviceError::Timeout),
                Err(other) => return Err(other.into()),
            }
        }
        let idx = t.dma_read_u16(USED_IDX_OFF)?;
        if idx != self.next_used.wrapping_add(1) {
            // More than one entry (for requests never sent), a step backwards, or a jump.
            return Err(DeviceError::Io);
        }
        // The slot comes from this driver's counter, never from the device.
        let slot = usize::from(self.next_used % QUEUE_SIZE);
        let at = USED_RING_OFF + slot * USED_ELEM_BYTES;
        let id = t.dma_read_u32(at)?;
        let len = t.dma_read_u32(at + 4)?;
        if id != HEAD || len > writable {
            return Err(DeviceError::Io);
        }
        self.next_used = idx;
        Ok(len)
    }
}

/// `phys + off`, refusing the overflow rather than wrapping a physical address.
fn phys_of(phys: u64, off: usize) -> Result<u64, DeviceError> {
    phys.checked_add(off as u64).ok_or(DeviceError::Queue)
}
