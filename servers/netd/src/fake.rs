//! A virtio-net device in safe Rust, written to be **hostile**: host-only, and the whole of what
//! `netd`'s tests run against.
//!
//! It has one set of registers and two DMA regions, as `netd` does: [`FakeNic::rx_view`] and
//! [`FakeNic::tx_view`] are the two transports `netd`'s threads hold. It sees every register
//! access and every byte of DMA traffic, in order, and may change a region between any two of
//! them, as a device with no IOMMU can. [`Policy`] says how it misbehaves; the default tells no
//! lies, and every field turns on one.
//!
//! The device uses the ring addresses `netd` programmed, not this crate's constants, so a driver
//! that pointed a ring elsewhere would be talking to a device that is not listening. Every address
//! **the driver writes** (a queue base register, a descriptor's address) is checked as it is
//! written: [`FakeNic::strayed`] counts one outside both regions, and [`FakeNic::crossed`] one in
//! the other queue's region, which would mean the two threads share memory. Garbage the device
//! scribbles on its own rings is not the driver's, and is not counted.
//!
//! Nothing here uses `unsafe`: the regions are `Vec<u8>`s and the registers are fields.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use crate::ring::{AVAIL_OFF, DESC_OFF, QUEUE_SIZE, REGION_LEN, SLOTS_OFF};
use crate::transport::{Fault, Transport};
use crate::virtio::{self, NET_HDR_LEN, bit, feature, reg, status};

/// Where the fake claims its two regions are. Neither page-aligned, by design.
pub const RX_PHYS: u64 = 0x8800_0100;
pub const TX_PHYS: u64 = 0x9900_0300;

/// The MAC an honest fake has: locally administered, unicast.
pub const FAKE_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

/// What the fake lies about. `Policy::default()` tells no lies.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub magic: Option<u32>,
    pub version: Option<u32>,
    pub device_id: Option<u32>,
    /// The whole 64-bit feature word offered, instead of the honest one.
    pub features: Option<u64>,
    pub never_resets: bool,
    pub status_drops_bits: bool,
    pub queue_num_max: Option<u32>,
    pub queue_ready_before: bool,
    pub queue_ready_stuck: bool,
    pub config_never_settles: bool,
    /// The MAC in the configuration space, instead of [`FAKE_MAC`].
    pub mac: Option<[u8; 6]>,
    /// How far the receive queue's `used.idx` moves per frame, instead of 1.
    pub rx_used_idx_delta: Option<u16>,
    /// Receive completions for buffers never offered, ahead of the real one.
    pub rx_extra_used: u16,
    /// The id in a receive used entry, instead of the buffer's.
    pub rx_used_id: Option<u32>,
    /// The id of the buffer used before, again: a buffer completed twice.
    pub rx_duplicate_id: bool,
    /// The length in a receive used entry, instead of what was written.
    pub rx_used_len: Option<u32>,
    /// The header's `flags` byte, instead of 0 (asks for checksum work).
    pub header_flags: Option<u8>,
    /// The header's `gso_type` byte, instead of 0 (asks for segmentation).
    pub header_gso: Option<u8>,
    /// Transmit buffers are never completed.
    pub tx_never_complete: bool,
    pub tx_used_id: Option<u32>,
    pub tx_used_len: Option<u32>,
    pub tx_used_idx_delta: Option<u16>,
    /// Raise the interrupt this many times without completing anything, on every notify.
    pub spurious_interrupts: u32,
    /// What to rewrite just before completing anything.
    pub scribble: Option<Scribble>,
    /// Go on writing a receive slot after completing it.
    pub keep_writing: bool,
}

/// What a hostile device rewrites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scribble {
    /// Both descriptor tables, with out-of-range lengths, addresses and `next` indices.
    Descriptors,
    /// Both available rings, their index included.
    Avail,
    /// Every byte of the receive region.
    WholeRx(u8),
    /// Every byte of the transmit region.
    WholeTx(u8),
}

/// Which of the two regions a view, or an address, is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Rx,
    Tx,
}

#[derive(Debug, Default, Clone, Copy)]
struct Queue {
    num: u32,
    ready: u32,
    desc: u64,
    avail: u64,
    used: u64,
    /// The available index the device has consumed up to.
    seen_avail: u16,
    /// The used index the device will publish next.
    used_idx: u16,
    /// The last buffer id completed (for `rx_duplicate_id`).
    last_id: Option<u16>,
}

#[derive(Debug, Default)]
struct Regs {
    status: u32,
    device_features_sel: u32,
    driver_features_sel: u32,
    driver_features: u64,
    queue_sel: u32,
    queues: [Queue; 2],
    config_generation: u32,
}

/// The fake device. [`FakeNic::set_policy`] may change between frames, so a device can behave
/// until the driver trusts it, then turn.
pub struct FakeNic {
    regs: RefCell<Regs>,
    rx_dma: RefCell<Vec<u8>>,
    tx_dma: RefCell<Vec<u8>>,
    policy: Cell<Policy>,
    irq: Cell<u32>,
    now: Cell<u64>,
    strayed: Cell<u64>,
    crossed: Cell<u64>,
    /// Frames waiting for a receive buffer, as a real device's backlog.
    inbox: RefCell<VecDeque<Vec<u8>>>,
    /// Every transmitted descriptor: its length, and the bytes the device read (header and frame).
    wire: RefCell<Vec<Vec<u8>>>,
    /// Bytes the device read from a transmit slot outside the descriptor it was given. Always 0.
    overread: Cell<u64>,
}

impl Default for FakeNic {
    fn default() -> Self { FakeNic::new() }
}

impl FakeNic {
    pub fn new() -> FakeNic {
        FakeNic {
            regs: RefCell::new(Regs::default()),
            rx_dma: RefCell::new(vec![0; REGION_LEN]),
            tx_dma: RefCell::new(vec![0; REGION_LEN]),
            policy: Cell::new(Policy::default()),
            irq: Cell::new(0),
            now: Cell::new(1),
            strayed: Cell::new(0),
            crossed: Cell::new(0),
            inbox: RefCell::new(VecDeque::new()),
            wire: RefCell::new(Vec::new()),
            overread: Cell::new(0),
        }
    }

    pub fn set_policy(&self, policy: Policy) { self.policy.set(policy); }

    pub fn policy(&self) -> Policy { self.policy.get() }

    /// The receive thread's transport.
    pub fn rx_view(&self) -> View<'_> { View { nic: self, region: Region::Rx } }

    /// The serving thread's transport.
    pub fn tx_view(&self) -> View<'_> { View { nic: self, region: Region::Tx } }

    /// Addresses the driver named outside both regions. The claim is that it is always 0.
    pub fn strayed(&self) -> u64 { self.strayed.get() }

    /// A queue's ring or buffer named in the other queue's region. Always 0.
    pub fn crossed(&self) -> u64 { self.crossed.get() }

    /// The feature bits the driver accepted.
    pub fn driver_features(&self) -> u64 { self.regs.borrow().driver_features }

    /// The device status register as last written.
    pub fn status(&self) -> u32 { self.regs.borrow().status }

    /// Every transmitted buffer, as the device read it: 12 bytes of header, then the frame.
    pub fn wire(&self) -> Vec<Vec<u8>> { self.wire.borrow().clone() }

    pub fn overread(&self) -> u64 { self.overread.get() }

    /// Advances the device's clock.
    pub fn advance(&self, us: u64) { self.now.set(self.now.get().saturating_add(us)); }

    /// Frames waiting for a receive buffer.
    pub fn backlog(&self) -> usize { self.inbox.borrow().len() }

    /// A frame arrives from the wire: delivered into the next receive buffer the driver offered,
    /// or kept until one is offered.
    pub fn arrive(&self, frame: &[u8]) {
        self.inbox.borrow_mut().push_back(frame.to_vec());
        self.pump();
    }

    fn offered(&self) -> u64 {
        self.policy.get().features.unwrap_or(
            bit(feature::VERSION_1)
                | bit(feature::MAC)
                | bit(0)  // CSUM
                | bit(1)  // GUEST_CSUM
                | bit(7)  // GUEST_TSO4
                | bit(11) // HOST_TSO4
                | bit(15) // MRG_RXBUF
                | bit(16) // STATUS
                | bit(17) // CTRL_VQ
                | bit(28) // RING_INDIRECT_DESC
                | bit(29), // RING_EVENT_IDX
        )
    }

    fn mac(&self) -> [u8; 6] { self.policy.get().mac.unwrap_or(FAKE_MAC) }

    fn dma(&self, region: Region) -> &RefCell<Vec<u8>> {
        match region {
            Region::Rx => &self.rx_dma,
            Region::Tx => &self.tx_dma,
        }
    }

    /// The region and offset a physical address names, if it is inside one of the two regions
    /// with `len` bytes after it.
    fn locate(&self, phys: u64, len: usize) -> Option<(Region, usize)> {
        for (region, base) in [(Region::Rx, RX_PHYS), (Region::Tx, TX_PHYS)] {
            if let Some(off) = phys.checked_sub(base).and_then(|d| usize::try_from(d).ok()) {
                if off.checked_add(len).is_some_and(|end| end <= REGION_LEN) {
                    return Some((region, off));
                }
            }
        }
        None
    }

    /// As [`FakeNic::locate`], only inside `expected`: what the device will touch.
    fn locate_in(&self, phys: u64, len: usize, expected: Region) -> Option<usize> {
        self.locate(phys, len).filter(|(region, _)| *region == expected).map(|(_, off)| off)
    }

    /// An address the driver just gave the device, for a queue whose memory is `expected`:
    /// counted if it is outside both regions, or in the other one.
    fn check_driver_address(&self, phys: u64, len: usize, expected: Region) {
        match self.locate(phys, len) {
            None => self.strayed.set(self.strayed.get() + 1),
            Some((region, _)) if region != expected => self.crossed.set(self.crossed.get() + 1),
            Some(_) => {}
        }
    }

    fn peek(&self, region: Region, off: usize, len: usize) -> Vec<u8> {
        let dma = self.dma(region).borrow();
        dma.get(off..off + len).map(Vec::from).unwrap_or_else(|| vec![0; len])
    }

    fn peek_u16(&self, region: Region, off: usize) -> u16 {
        let b = self.peek(region, off, 2);
        u16::from_le_bytes([b[0], b[1]])
    }

    fn peek_u32(&self, region: Region, off: usize) -> u32 {
        let b = self.peek(region, off, 4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    fn peek_u64(&self, region: Region, off: usize) -> u64 {
        let b = self.peek(region, off, 8);
        let mut v = [0; 8];
        v.copy_from_slice(&b);
        u64::from_le_bytes(v)
    }

    fn poke(&self, region: Region, off: usize, bytes: &[u8]) {
        let mut dma = self.dma(region).borrow_mut();
        if let Some(slot) = dma.get_mut(off..off + bytes.len()) {
            slot.copy_from_slice(bytes);
        }
    }

    /// Queue `q`'s three rings, located in the region `q` belongs to.
    fn rings(&self, q: usize, region: Region) -> Option<(usize, usize, usize)> {
        let queue = self.regs.borrow().queues[q];
        if queue.ready == 0 {
            return None;
        }
        let desc = self.locate_in(queue.desc, 16 * QUEUE_SIZE as usize, region)?;
        let avail = self.locate_in(queue.avail, 6 + 2 * QUEUE_SIZE as usize, region)?;
        let used = self.locate_in(queue.used, 6 + 8 * QUEUE_SIZE as usize, region)?;
        Some((desc, avail, used))
    }

    /// The next buffer the driver offered on queue `q`: its head id, and its descriptor's
    /// address, length and flags. `None` if none is available.
    fn next_buffer(&self, q: usize, region: Region) -> Option<(u16, u64, u32, u16, usize)> {
        let (desc, avail, used) = self.rings(q, region)?;
        let seen = self.regs.borrow().queues[q].seen_avail;
        if seen == self.peek_u16(region, avail + 2) {
            return None;
        }
        let head = self.peek_u16(region, avail + 4 + usize::from(seen % QUEUE_SIZE) * 2);
        self.regs.borrow_mut().queues[q].seen_avail = seen.wrapping_add(1);
        if head >= QUEUE_SIZE {
            return None;
        }
        let at = desc + usize::from(head) * 16;
        Some((
            head,
            self.peek_u64(region, at),
            self.peek_u32(region, at + 8),
            self.peek_u16(region, at + 12),
            used,
        ))
    }

    /// Publishes one used entry (and the extra, lying ones the policy asks for) on queue `q`.
    fn complete(&self, q: usize, region: Region, used: usize, id: u32, len: u32, extra: u16, delta: Option<u16>) {
        let mut regs = self.regs.borrow_mut();
        let base = regs.queues[q].used_idx;
        for e in 0..extra {
            let slot = usize::from(base.wrapping_add(e) % QUEUE_SIZE);
            self.poke(region, used + 4 + slot * 8, &0xdead_beefu32.to_le_bytes());
            self.poke(region, used + 8 + slot * 8, &u32::MAX.to_le_bytes());
        }
        let slot = usize::from(base.wrapping_add(extra) % QUEUE_SIZE);
        self.poke(region, used + 4 + slot * 8, &id.to_le_bytes());
        self.poke(region, used + 8 + slot * 8, &len.to_le_bytes());
        let idx = base.wrapping_add(delta.unwrap_or(extra.wrapping_add(1)));
        regs.queues[q].used_idx = idx;
        drop(regs);
        self.poke(region, used + 2, &idx.to_le_bytes());
    }

    /// Delivers waiting frames into offered receive buffers.
    fn pump(&self) {
        let policy = self.policy.get();
        loop {
            if self.inbox.borrow().is_empty() {
                return;
            }
            let Some((head, addr, len, flags, used)) = self.next_buffer(0, Region::Rx) else { return };
            let frame = self.inbox.borrow_mut().pop_front().unwrap_or_default();
            // A receive buffer must be device-writable and hold the header and the frame.
            let Some(off) = self.locate_in(addr, len as usize, Region::Rx) else { continue };
            if flags & 2 == 0 || (len as usize) < NET_HDR_LEN + frame.len() {
                continue;
            }
            if let Some(what) = policy.scribble {
                self.scribble(what);
            }
            let mut header = [0u8; NET_HDR_LEN];
            header[0] = policy.header_flags.unwrap_or(0);
            header[1] = policy.header_gso.unwrap_or(0);
            header[10] = 1; // num_buffers
            self.poke(Region::Rx, off, &header);
            self.poke(Region::Rx, off + NET_HDR_LEN, &frame);
            let last = self.regs.borrow().queues[0].last_id;
            let id = if policy.rx_duplicate_id { last.unwrap_or(head) } else { head };
            self.regs.borrow_mut().queues[0].last_id = Some(head);
            self.complete(
                0,
                Region::Rx,
                used,
                policy.rx_used_id.unwrap_or(u32::from(id)),
                policy.rx_used_len.unwrap_or((NET_HDR_LEN + frame.len()) as u32),
                policy.rx_extra_used,
                policy.rx_used_idx_delta,
            );
            if policy.keep_writing {
                self.poke(Region::Rx, off, &[0xa5; 64]);
            }
            self.irq.set(self.irq.get() | 1);
        }
    }

    /// Sends every transmit buffer the driver offered.
    fn transmit(&self) {
        let policy = self.policy.get();
        while let Some((head, addr, len, flags, used)) = self.next_buffer(1, Region::Tx) {
            let Some(off) = self.locate_in(addr, len as usize, Region::Tx) else { continue };
            // A transmit buffer is read by the device: never device-writable.
            if flags & 2 != 0 {
                continue;
            }
            // Exactly `len` bytes, the descriptor's; the slot's end is not read. If the slot
            // reaches past what the descriptor names, that is counted as an over-read by us.
            let bytes = self.peek(Region::Tx, off, len as usize);
            let slot_end = SLOTS_OFF + (usize::from(head) + 1) * crate::ring::SLOT_LEN;
            if off + len as usize > slot_end {
                self.overread.set(self.overread.get() + 1);
            }
            self.wire.borrow_mut().push(bytes);
            if policy.tx_never_complete {
                continue;
            }
            if let Some(what) = policy.scribble {
                self.scribble(what);
            }
            self.complete(
                1,
                Region::Tx,
                used,
                policy.tx_used_id.unwrap_or(u32::from(head)),
                policy.tx_used_len.unwrap_or(0),
                0,
                policy.tx_used_idx_delta,
            );
        }
    }

    fn scribble(&self, what: Scribble) {
        match what {
            Scribble::Descriptors => {
                for region in [Region::Rx, Region::Tx] {
                    for i in 0..QUEUE_SIZE as usize {
                        let at = DESC_OFF + i * 16;
                        self.poke(region, at, &u64::MAX.to_le_bytes());
                        self.poke(region, at + 8, &u32::MAX.to_le_bytes());
                        self.poke(region, at + 12, &3u16.to_le_bytes());
                        self.poke(region, at + 14, &u16::MAX.to_le_bytes());
                    }
                }
            }
            Scribble::Avail => {
                for region in [Region::Rx, Region::Tx] {
                    self.poke(region, AVAIL_OFF, &[0xff; 6 + 2 * QUEUE_SIZE as usize]);
                }
            }
            Scribble::WholeRx(byte) => self.rx_dma.borrow_mut().fill(byte),
            Scribble::WholeTx(byte) => self.tx_dma.borrow_mut().fill(byte),
        }
    }

    fn reg_read(&self, off: usize) -> u32 {
        let policy = self.policy.get();
        let mut regs = self.regs.borrow_mut();
        let q = regs.queue_sel as usize;
        match off {
            reg::MAGIC => policy.magic.unwrap_or(virtio::MAGIC),
            reg::VERSION => policy.version.unwrap_or(virtio::VERSION),
            reg::DEVICE_ID => policy.device_id.unwrap_or(virtio::DEVICE_ID_NET),
            reg::DEVICE_FEATURES => {
                let all = self.offered();
                if regs.device_features_sel == 0 { all as u32 } else { (all >> 32) as u32 }
            }
            reg::QUEUE_NUM_MAX => {
                if q < 2 {
                    policy.queue_num_max.unwrap_or(256)
                } else {
                    0
                }
            }
            reg::QUEUE_READY => match regs.queues.get(q) {
                Some(queue) if policy.queue_ready_before && queue.ready == 0 => 1,
                Some(_) if policy.queue_ready_stuck => 0,
                Some(queue) => queue.ready,
                None => 0,
            },
            reg::INTERRUPT_STATUS => self.irq.get(),
            reg::STATUS => {
                if policy.never_resets && regs.status == 0 {
                    status::FAILED
                } else if policy.status_drops_bits {
                    regs.status & !status::FEATURES_OK
                } else {
                    regs.status
                }
            }
            reg::CONFIG_GENERATION => {
                if policy.config_never_settles {
                    regs.config_generation = regs.config_generation.wrapping_add(1);
                }
                regs.config_generation
            }
            _ => 0,
        }
    }

    fn reg_write(&self, off: usize, value: u32) {
        {
            let mut regs = self.regs.borrow_mut();
            let q = regs.queue_sel as usize;
            let half = |current: u64, high: bool| {
                if high {
                    (current & 0xffff_ffff) | (u64::from(value) << 32)
                } else {
                    (current & !0xffff_ffff) | u64::from(value)
                }
            };
            match off {
                reg::STATUS => {
                    if value == 0 {
                        *regs = Regs::default();
                        self.irq.set(0);
                    } else {
                        regs.status = value;
                    }
                }
                reg::DEVICE_FEATURES_SEL => regs.device_features_sel = value,
                reg::DRIVER_FEATURES_SEL => regs.driver_features_sel = value,
                reg::DRIVER_FEATURES => {
                    regs.driver_features = if regs.driver_features_sel == 0 {
                        (regs.driver_features & !0xffff_ffff) | u64::from(value)
                    } else {
                        (regs.driver_features & 0xffff_ffff) | (u64::from(value) << 32)
                    };
                }
                reg::QUEUE_SEL => regs.queue_sel = value,
                reg::INTERRUPT_ACK => self.irq.set(self.irq.get() & !value),
                _ if q < 2 => {
                    let queue = &mut regs.queues[q];
                    match off {
                        reg::QUEUE_NUM => queue.num = value,
                        reg::QUEUE_READY => queue.ready = value,
                        reg::QUEUE_DESC_LOW => queue.desc = half(queue.desc, false),
                        reg::QUEUE_DESC_HIGH => queue.desc = half(queue.desc, true),
                        reg::QUEUE_DRIVER_LOW => queue.avail = half(queue.avail, false),
                        reg::QUEUE_DRIVER_HIGH => queue.avail = half(queue.avail, true),
                        reg::QUEUE_DEVICE_LOW => queue.used = half(queue.used, false),
                        reg::QUEUE_DEVICE_HIGH => queue.used = half(queue.used, true),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if off == reg::QUEUE_READY && value != 0 {
            let (q, queue) = {
                let regs = self.regs.borrow();
                let q = regs.queue_sel as usize;
                (q, regs.queues.get(q).copied())
            };
            if let Some(queue) = queue {
                let region = if q == 0 { Region::Rx } else { Region::Tx };
                self.check_driver_address(queue.desc, 16 * QUEUE_SIZE as usize, region);
                self.check_driver_address(queue.avail, 6 + 2 * QUEUE_SIZE as usize, region);
                self.check_driver_address(queue.used, 6 + 8 * QUEUE_SIZE as usize, region);
            }
        }
        if off == reg::QUEUE_NOTIFY {
            for _ in 0..self.policy.get().spurious_interrupts {
                self.irq.set(self.irq.get() | 1);
            }
            // Only a device that is running touches its rings.
            if self.regs.borrow().status & status::DRIVER_OK != 0 {
                match value {
                    0 => self.pump(),
                    1 => self.transmit(),
                    _ => {}
                }
            }
        }
    }

    fn config_byte(&self, off: usize) -> u8 {
        if self.policy.get().config_never_settles {
            let mut regs = self.regs.borrow_mut();
            regs.config_generation = regs.config_generation.wrapping_add(1);
        }
        match off.checked_sub(reg::CONFIG) {
            Some(i) if i < 6 => self.mac()[i],
            _ => 0,
        }
    }
}

/// One of the fake's two transports: the shared registers and one region.
pub struct View<'a> {
    nic: &'a FakeNic,
    region: Region,
}

impl Transport for View<'_> {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> { Ok(self.nic.reg_read(off)) }

    fn reg_read_u8(&self, off: usize) -> Result<u8, Fault> { Ok(self.nic.config_byte(off)) }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> {
        self.nic.reg_write(off, value);
        Ok(())
    }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> {
        let dma = self.nic.dma(self.region).borrow();
        let bytes = dma.get(off..off + out.len()).ok_or(Fault::Bounds)?;
        out.copy_from_slice(bytes);
        Ok(())
    }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> {
        {
            let mut dma = self.nic.dma(self.region).borrow_mut();
            let slot = dma.get_mut(off..off + src.len()).ok_or(Fault::Bounds)?;
            slot.copy_from_slice(src);
        }
        // A descriptor's address field, written by the driver: it must name this view's region.
        let table = DESC_OFF..DESC_OFF + 16 * QUEUE_SIZE as usize;
        if table.contains(&off) && (off - DESC_OFF).is_multiple_of(16) && src.len() == 8 {
            let mut addr = [0; 8];
            addr.copy_from_slice(src);
            self.nic.check_driver_address(u64::from_le_bytes(addr), 1, self.region);
        }
        Ok(())
    }

    fn dma_zero(&self, off: usize, len: usize) -> Result<(), Fault> {
        let mut dma = self.nic.dma(self.region).borrow_mut();
        dma.get_mut(off..off + len).ok_or(Fault::Bounds)?.fill(0);
        Ok(())
    }

    fn dma_phys(&self) -> u64 {
        match self.region {
            Region::Rx => RX_PHYS,
            Region::Tx => TX_PHYS,
        }
    }

    fn dma_len(&self) -> usize { REGION_LEN }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> {
        if self.nic.irq.get() != 0 {
            self.nic.advance(1);
            return Ok(());
        }
        self.nic.advance(timeout_us.max(1));
        Err(Fault::Timeout)
    }

    fn now_us(&self) -> u64 { self.nic.now.get() }
}

/// Draws a [`Policy`] from a stream of bytes: the one place a hostile device is built at random,
/// shared by the randomized sweep in `tests/device.rs` and the fuzz targets. Every field draws
/// its bytes whether or not its flag is set, so one flipped bit changes one lie.
pub fn policy_from(next: &mut impl FnMut() -> u8) -> Policy {
    let lies = u32_of(next);
    let on = |n: u32| (lies >> n) & 1 == 1;
    let (magic, version, device_id) = (u32_of(next), u32_of(next), u32_of(next));
    let features = u64_of(next);
    let queue_num_max = u32_of(next);
    let mac = [next(), next(), next(), next(), next(), next()];
    let rx_delta = u32_of(next) as u16;
    let rx_extra = u16::from(next() % 4);
    let (rx_id, rx_len) = (u32_of(next), u32_of(next));
    let (flags, gso) = (next(), next());
    let (tx_id, tx_len) = (u32_of(next), u32_of(next));
    let tx_delta = u32_of(next) as u16;
    let spurious = u32::from(next() % 4);
    let scribble = match next() % 4 {
        0 => Scribble::Descriptors,
        1 => Scribble::Avail,
        2 => Scribble::WholeRx(next()),
        _ => Scribble::WholeTx(next()),
    };
    Policy {
        magic: on(0).then_some(magic),
        version: on(1).then_some(version),
        device_id: on(2).then_some(device_id),
        features: on(3).then_some(features),
        never_resets: on(4),
        status_drops_bits: on(5),
        queue_num_max: on(6).then_some(queue_num_max),
        queue_ready_before: on(7),
        queue_ready_stuck: on(8),
        config_never_settles: on(9),
        mac: on(10).then_some(mac),
        rx_used_idx_delta: on(11).then_some(rx_delta),
        rx_extra_used: if on(12) { rx_extra } else { 0 },
        rx_used_id: on(13).then_some(rx_id),
        rx_duplicate_id: on(14),
        rx_used_len: on(15).then_some(rx_len),
        header_flags: on(16).then_some(flags),
        header_gso: on(17).then_some(gso),
        tx_never_complete: on(18),
        tx_used_id: on(19).then_some(tx_id),
        tx_used_len: on(20).then_some(tx_len),
        tx_used_idx_delta: on(21).then_some(tx_delta),
        spurious_interrupts: if on(22) { spurious } else { 0 },
        scribble: on(23).then_some(scribble),
        keep_writing: on(24),
    }
}

fn u32_of(next: &mut impl FnMut() -> u8) -> u32 { u32::from_le_bytes([next(), next(), next(), next()]) }

fn u64_of(next: &mut impl FnMut() -> u8) -> u64 { u64::from(u32_of(next)) | (u64::from(u32_of(next)) << 32) }

/// Drives one randomly lying device drawn from `bytes`: bring-up, then frames both ways for a few
/// rounds, changing its lies as it goes. Returns the device, for the caller to assert on
/// ([`FakeNic::strayed`], [`FakeNic::crossed`], [`FakeNic::overread`]); a panic anywhere is the
/// failure itself. Shared by the randomized sweep in `tests/device.rs` and the `device` fuzz target.
pub fn exercise(bytes: &mut impl FnMut() -> u8) -> FakeNic {
    let nic = FakeNic::new();
    nic.set_policy(policy_from(bytes));
    {
        let (rx_view, tx_view) = (nic.rx_view(), nic.tx_view());
        if let Ok(crate::Up { mut rx, mut tx, .. }) = crate::bring_up(&rx_view, &tx_view) {
            let mut scratch: crate::rxq::Frame = [0; crate::ring::SLOT_LEN];
            for round in 0..8u8 {
                if round % 3 == 2 {
                    nic.set_policy(policy_from(bytes));
                }
                let len = usize::from(bytes()) * 8;
                let frame: Vec<u8> = (0..len).map(|i| round.wrapping_add(i as u8)).collect();
                nic.arrive(&frame);
                let drained = rx.drain(&rx_view, &mut scratch, |f| {
                    assert!((virtio::MIN_FRAME..=virtio::MAX_FRAME).contains(&f.len()), "a bad frame delivered");
                });
                if drained.is_err() {
                    break;
                }
                let out: Vec<u8> = vec![round; 14 + usize::from(bytes()) * 5];
                if tx.transmit(&tx_view, &out).is_err() {
                    break;
                }
                nic.advance(u64::from(bytes()) * 100_000);
            }
        }
    }
    nic
}
