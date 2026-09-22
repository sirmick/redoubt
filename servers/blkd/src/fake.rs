//! A virtio-blk device in safe Rust, written to be **hostile**: host-only, and the whole of what
//! `blkd`'s tests run against.
//!
//! It implements [`Transport`], so it sees every register access and every byte of DMA traffic
//! the driver makes, in order, and may change the region between any two of them — which is
//! exactly what a device with no IOMMU can do. [`Policy`] says how it misbehaves; the default is
//! a device that follows the specification, and every field turns on one lie.
//!
//! The lies it can tell, and what each attacks:
//!
//! | Field | What it does |
//! | --- | --- |
//! | `magic`, `version`, `device_id` | is not the device the driver expects |
//! | `never_resets`, `status_drops_bits` | refuses the status handshake |
//! | `features` | offers a feature set the driver must refuse, or none at all |
//! | `queue_num_max`, `queue_ready_stuck` | a queue too small for one request, or one it will not make ready |
//! | `capacity`, `config_never_settles` | a capacity that would overflow, or a configuration space that never holds still |
//! | `used_idx_delta` | moves `used.idx` by something other than one: a jump, a stall, a step backwards |
//! | `extra_used_entries` | completes requests that were never sent |
//! | `used_id` | names a descriptor the driver did not submit — including one out of the ring |
//! | `used_len` | claims to have written more than it was given, up to `u32::MAX` |
//! | `blk_status` | a status byte outside the three the specification defines |
//! | `short_write` | writes fewer data bytes than asked for |
//! | `no_interrupt`, `never_complete`, `spurious_interrupts` | makes the driver wait, or wake for nothing |
//! | `scribble` | rewrites the descriptor table, the available ring, the header or the whole region |
//! | `scribble_on_every_read` | rewrites the rings **between** two of the driver's own reads |
//! | `keep_writing_data` | goes on writing the data buffer after the request is complete |
//!
//! Nothing here uses `unsafe`: the "DMA region" is a `Vec<u8>` and the "registers" are fields, so
//! a fake that got its own bounds wrong would panic in the test rather than corrupt anything.

use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use crate::queue::{DATA_OFF, DMA_LEN, QUEUE_SIZE};
use crate::transport::{Fault, Transport};
use crate::virtio::{self, DATA_LEN, SECTOR_SIZE, bit, blk_status, feature, reg, request, status};

/// The physical address the fake claims its DMA region is at. Not page-aligned by accident: a
/// driver that assumed an alignment it was not promised would show up here.
pub const FAKE_PHYS: u64 = 0x8800_0000;

/// What the fake lies about. `Policy::default()` tells no lies.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub magic: Option<u32>,
    pub version: Option<u32>,
    pub device_id: Option<u32>,
    /// The whole 64-bit feature word the device offers, instead of the honest one.
    pub features: Option<u64>,
    pub queue_num_max: Option<u32>,
    pub capacity: Option<u64>,
    /// `ConfigGeneration` changes on every read, so the driver can never read the capacity
    /// atomically.
    pub config_never_settles: bool,
    /// The status register never reads back 0 after a reset.
    pub never_resets: bool,
    /// The status register reads back fewer bits than were written.
    pub status_drops_bits: bool,
    /// `QueueReady` never reads back 1.
    pub queue_ready_stuck: bool,
    /// `QueueReady` reads 1 before the driver sets it: a queue that was never reset.
    pub queue_ready_before: bool,
    /// How far `used.idx` moves on a completion, instead of 1.
    pub used_idx_delta: Option<u16>,
    /// Extra used entries written before the real one: completions for requests never sent.
    pub extra_used_entries: u16,
    /// The descriptor id in the used entry, instead of the head the driver submitted.
    pub used_id: Option<u32>,
    /// The length in the used entry, instead of what was written.
    pub used_len: Option<u32>,
    /// The virtio-blk status byte, instead of the honest one.
    pub blk_status: Option<u8>,
    /// Write only the first half of the data a read asked for.
    pub short_write: bool,
    /// Complete the request but raise no interrupt.
    pub no_interrupt: bool,
    /// Never complete the request at all.
    pub never_complete: bool,
    /// Raise the interrupt this many times without completing anything, before doing the work.
    pub spurious_interrupts: u32,
    /// Serve the request while the driver is blocked in `wait_irq`, rather than inside the write
    /// of `QueueNotify`. This is what a real device does; the other way round exercises the path
    /// where the completion is already there when the driver first looks.
    pub defer: bool,
    /// What to rewrite in the region just before completing.
    pub scribble: Option<Scribble>,
    /// Rewrite the rings before **every** read the driver makes of the region, so nothing it
    /// reads twice reads the same.
    pub scribble_on_every_read: bool,
    /// Go on writing the data buffer after the request has completed.
    pub keep_writing_data: bool,
}

/// What a hostile device rewrites in the region it shares with the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scribble {
    /// Descriptors full of out-of-range lengths, addresses and `next` indices.
    Descriptors,
    /// A descriptor chain that points at itself.
    DescriptorLoop,
    /// The available ring, including its index.
    Avail,
    /// The request header.
    Header,
    /// Every byte of the region, rings, header, status and data alike.
    Whole(u8),
}

/// The fake device. One is built per test; [`FakeDevice::policy`] may be changed between requests,
/// so a device can behave until the driver trusts it and then turn.
pub struct FakeDevice {
    regs: RefCell<Regs>,
    dma: RefCell<Vec<u8>>,
    /// The disk's bytes: `sectors * 512` of them.
    disk: RefCell<Vec<u8>>,
    policy: Cell<Policy>,
    irq: Cell<u32>,
    now: Cell<u64>,
    /// Requests the device has been notified of, honest or not.
    notified: Cell<u64>,
    /// How many reads the `scribble_on_every_read` policy has rewritten the rings before.
    scribble_reads: Cell<u64>,
    /// A request was notified and has not been served yet (`Policy::defer`).
    pending: Cell<bool>,
    /// Addresses the driver gave the device that were not inside its region. Always 0, and
    /// asserted on: it is the executable form of "`blkd` never asks the device to touch anything
    /// but the pages `dma_alloc` gave it".
    strayed: Cell<u64>,
}

#[derive(Debug, Default)]
struct Regs {
    status: u32,
    device_features_sel: u32,
    driver_features_sel: u32,
    driver_features: u64,
    queue_num: u32,
    queue_ready: u32,
    config_generation: u32,
    /// The used index the device will publish next.
    used_idx: u16,
    /// The available index the device last consumed.
    seen_avail: u16,
    /// The physical addresses of the three rings, as the driver programmed them. The device uses
    /// *these*, not this crate's constants, so a driver that pointed a ring somewhere else would
    /// be talking to a device that is not listening — and `strayed` would count it.
    desc_addr: u64,
    avail_addr: u64,
    used_addr: u64,
}

impl FakeDevice {
    /// A device with `sectors` sectors of zeroed disk and no lies.
    pub fn new(sectors: u64) -> FakeDevice {
        FakeDevice::with_image(vec![0; sectors as usize * SECTOR_SIZE as usize])
    }

    /// A device whose disk starts as `image`, which must be a whole number of sectors.
    pub fn with_image(image: Vec<u8>) -> FakeDevice {
        FakeDevice {
            regs: RefCell::new(Regs::default()),
            dma: RefCell::new(vec![0; DMA_LEN]),
            disk: RefCell::new(image),
            policy: Cell::new(Policy::default()),
            irq: Cell::new(0),
            now: Cell::new(1),
            notified: Cell::new(0),
            scribble_reads: Cell::new(0),
            pending: Cell::new(false),
            strayed: Cell::new(0),
        }
    }

    pub fn set_policy(&self, policy: Policy) { self.policy.set(policy); }

    pub fn notified(&self) -> u64 { self.notified.get() }

    /// The feature bits the driver wrote back: what it accepted of what was offered.
    pub fn driver_features(&self) -> u64 { self.regs.borrow().driver_features }

    /// Addresses the driver named that were outside the region the device was given. The claim
    /// this crate makes is that it is always 0.
    pub fn strayed(&self) -> u64 { self.strayed.get() }

    /// The disk's sectors.
    pub fn sectors(&self) -> u64 { (self.disk.borrow().len() / SECTOR_SIZE as usize) as u64 }

    /// A copy of one sector of the disk, for tests that check what was written.
    pub fn sector(&self, lba: u64) -> Vec<u8> {
        let disk = self.disk.borrow();
        let start = lba as usize * SECTOR_SIZE as usize;
        disk.get(start..start + SECTOR_SIZE as usize).map(Vec::from).unwrap_or_default()
    }

    /// The whole 64-bit feature word this device offers.
    fn offered(&self) -> u64 {
        self.policy
            .get()
            .features
            // What QEMU offers, roughly: the two bits `blkd` needs, and a handful it must not
            // accept. A driver that echoed the offer back would turn on indirect descriptors and
            // event indices and then read a ring that is not the one it wrote.
            .unwrap_or(
                bit(feature::VERSION_1)
                    | bit(feature::BLK_FLUSH)
                    | bit(1)  // BLK_SIZE_MAX
                    | bit(2)  // BLK_SEG_MAX
                    | bit(6)  // BLK_BLK_SIZE
                    | bit(28) // RING_INDIRECT_DESC
                    | bit(29), // RING_EVENT_IDX
            )
    }

    fn capacity(&self) -> u64 { self.policy.get().capacity.unwrap_or_else(|| self.sectors()) }

    /// Reads `len` bytes of the region at `off`, for the device's own use. Bounds-checked, so a
    /// device asked for nonsense reads zeros rather than panicking the test.
    fn peek(&self, off: usize, len: usize) -> Vec<u8> {
        let dma = self.dma.borrow();
        dma.get(off..off + len).map(Vec::from).unwrap_or_else(|| vec![0; len])
    }

    fn peek_u16(&self, off: usize) -> u16 {
        let b = self.peek(off, 2);
        u16::from_le_bytes([b[0], b[1]])
    }

    fn peek_u32(&self, off: usize) -> u32 {
        let b = self.peek(off, 4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    fn peek_u64(&self, off: usize) -> u64 {
        let b = self.peek(off, 8);
        let mut v = [0; 8];
        v.copy_from_slice(&b);
        u64::from_le_bytes(v)
    }

    fn poke(&self, off: usize, bytes: &[u8]) {
        let mut dma = self.dma.borrow_mut();
        if let Some(slot) = dma.get_mut(off..off + bytes.len()) {
            slot.copy_from_slice(bytes);
        }
    }

    /// The offset in the region that a physical address names, if it is inside it. A device that
    /// was given an address outside its region would fail here, which is how the test that
    /// `blkd` never hands one out is not vacuous: [`FakeDevice::process`] refuses the request
    /// rather than reaching for host memory it does not own.
    fn offset_of(&self, phys: u64) -> Option<usize> {
        let inside = phys
            .checked_sub(FAKE_PHYS)
            .and_then(|delta| usize::try_from(delta).ok())
            .filter(|off| *off < DMA_LEN);
        if inside.is_none() {
            self.strayed.set(self.strayed.get() + 1);
        }
        inside
    }

    /// The three rings' offsets in the region, as the driver programmed them; `None` for one the
    /// driver put outside the region, which also counts as having strayed.
    fn rings(&self) -> (Option<usize>, Option<usize>, Option<usize>) {
        let (desc, avail, used) = {
            let regs = self.regs.borrow();
            (regs.desc_addr, regs.avail_addr, regs.used_addr)
        };
        (self.offset_of(desc), self.offset_of(avail), self.offset_of(used))
    }

    /// Serves whatever the available ring offers. Called when the driver writes `QueueNotify`.
    fn process(&self) {
        self.notified.set(self.notified.get() + 1);
        let policy = self.policy.get();
        for _ in 0..policy.spurious_interrupts {
            self.irq.set(self.irq.get() | 1);
        }
        if policy.never_complete {
            return;
        }
        if policy.defer {
            self.pending.set(true);
            return;
        }
        self.run(&policy);
    }

    /// Serves whatever the available ring offers, now.
    fn run(&self, policy: &Policy) {
        let (Some(desc), Some(avail), Some(used)) = self.rings() else { return };
        let avail_idx = self.peek_u16(avail + 2);
        let mut seen = self.regs.borrow().seen_avail;
        // Only ever one outstanding, but a loop costs nothing and models a device that batches.
        let mut served = 0;
        while seen != avail_idx && served < QUEUE_SIZE {
            let slot = usize::from(seen % QUEUE_SIZE);
            let head = self.peek_u16(avail + 4 + slot * 2);
            self.serve_chain(desc, used, head, policy);
            seen = seen.wrapping_add(1);
            served += 1;
        }
        self.regs.borrow_mut().seen_avail = seen;
    }

    /// Walks one descriptor chain and does what it asks, then publishes a used entry.
    fn serve_chain(&self, desc: usize, used: usize, head: u16, policy: &Policy) {
        let mut segments: Vec<(usize, u32, bool)> = Vec::new();
        let mut next = head;
        // The device's own walk is bounded by the ring size: a driver that wrote a loop would
        // hang a real device, not this one.
        for _ in 0..QUEUE_SIZE {
            if usize::from(next) >= QUEUE_SIZE as usize {
                return;
            }
            let at = desc + usize::from(next) * 16;
            let addr = self.peek_u64(at);
            let len = self.peek_u32(at + 8);
            let flags = self.peek_u16(at + 12);
            let Some(off) = self.offset_of(addr) else { return };
            if off.checked_add(len as usize).is_none_or(|end| end > DMA_LEN) {
                return;
            }
            segments.push((off, len, flags & 2 != 0));
            if flags & 1 == 0 {
                break;
            }
            next = self.peek_u16(at + 14);
        }
        let Some(&(header_off, header_len, false)) = segments.first() else { return };
        if header_len < 16 {
            return;
        }
        let kind = self.peek_u32(header_off);
        let sector = self.peek_u64(header_off + 8);
        let Some(&(status_off, _, true)) = segments.last() else { return };

        let mut written = 0u32;
        let mut outcome = blk_status::OK;
        match kind {
            request::IN => {
                let Some(&(data_off, data_len, true)) = segments.get(1) else { return };
                let mut bytes = self.read_disk(sector, data_len as usize);
                if policy.short_write {
                    bytes.truncate(bytes.len() / 2);
                }
                self.poke(data_off, &bytes);
                written = bytes.len() as u32;
            }
            request::OUT => {
                let Some(&(data_off, data_len, false)) = segments.get(1) else { return };
                let bytes = self.peek(data_off, data_len as usize);
                if !self.write_disk(sector, &bytes) {
                    outcome = blk_status::IOERR;
                }
            }
            request::FLUSH => {}
            _ => outcome = blk_status::UNSUPP,
        }

        if let Some(what) = policy.scribble {
            self.scribble(what);
        }
        self.poke(status_off, &[policy.blk_status.unwrap_or(outcome)]);
        written = written.saturating_add(1);

        let mut regs = self.regs.borrow_mut();
        let base = regs.used_idx;
        // Completions for requests that were never sent, ahead of the real one.
        for extra in 0..policy.extra_used_entries {
            let slot = usize::from(base.wrapping_add(extra) % QUEUE_SIZE);
            self.write_used(used, slot, 0xdead_beef, u32::MAX);
        }
        let real = base.wrapping_add(policy.extra_used_entries);
        self.write_used(
            used,
            usize::from(real % QUEUE_SIZE),
            policy.used_id.unwrap_or(crate::queue::HEAD),
            policy.used_len.unwrap_or(written),
        );
        let idx = base
            .wrapping_add(policy.used_idx_delta.unwrap_or_else(|| policy.extra_used_entries.wrapping_add(1)));
        regs.used_idx = idx;
        drop(regs);
        self.poke(used + 2, &idx.to_le_bytes());

        if policy.keep_writing_data {
            self.poke(DATA_OFF, &vec![0xa5; DATA_LEN]);
        }
        if !policy.no_interrupt {
            self.irq.set(self.irq.get() | 1);
        }
    }

    fn write_used(&self, used: usize, slot: usize, id: u32, len: u32) {
        let at = used + 4 + slot * 8;
        self.poke(at, &id.to_le_bytes());
        self.poke(at + 4, &len.to_le_bytes());
    }

    fn read_disk(&self, sector: u64, len: usize) -> Vec<u8> {
        let disk = self.disk.borrow();
        let start = match usize::try_from(sector).ok().and_then(|s| s.checked_mul(SECTOR_SIZE as usize)) {
            Some(start) => start,
            None => return vec![0; len],
        };
        match disk.get(start..start + len) {
            Some(bytes) => Vec::from(bytes),
            None => vec![0; len],
        }
    }

    fn write_disk(&self, sector: u64, bytes: &[u8]) -> bool {
        let mut disk = self.disk.borrow_mut();
        let Some(start) = usize::try_from(sector).ok().and_then(|s| s.checked_mul(SECTOR_SIZE as usize))
        else {
            return false;
        };
        match disk.get_mut(start..start + bytes.len()) {
            Some(slot) => {
                slot.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }

    /// Rewrites part of the region with values chosen to be the worst ones: lengths and indices
    /// at their maxima, addresses far outside the region, and chains that loop.
    fn scribble(&self, what: Scribble) {
        match what {
            Scribble::Descriptors => {
                for i in 0..QUEUE_SIZE as usize {
                    let at = crate::queue::DESC_OFF + i * 16;
                    self.poke(at, &u64::MAX.to_le_bytes());
                    self.poke(at + 8, &u32::MAX.to_le_bytes());
                    self.poke(at + 12, &1u16.to_le_bytes());
                    self.poke(at + 14, &u16::MAX.to_le_bytes());
                }
            }
            Scribble::DescriptorLoop => {
                for i in 0..QUEUE_SIZE as usize {
                    let at = crate::queue::DESC_OFF + i * 16;
                    self.poke(at + 12, &1u16.to_le_bytes());
                    self.poke(at + 14, &(i as u16).to_le_bytes());
                }
            }
            Scribble::Avail => {
                self.poke(crate::queue::AVAIL_OFF, &u16::MAX.to_le_bytes());
                self.poke(crate::queue::AVAIL_OFF + 2, &u16::MAX.to_le_bytes());
                for i in 0..QUEUE_SIZE as usize {
                    self.poke(crate::queue::AVAIL_OFF + 4 + i * 2, &u16::MAX.to_le_bytes());
                }
            }
            Scribble::Header => {
                self.poke(crate::queue::HEADER_OFF, &[0xff; 16]);
            }
            Scribble::Whole(byte) => {
                let mut dma = self.dma.borrow_mut();
                dma.fill(byte);
            }
        }
    }
}

impl Transport for FakeDevice {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> {
        let policy = self.policy.get();
        let mut regs = self.regs.borrow_mut();
        let value = match off {
            reg::MAGIC => policy.magic.unwrap_or(virtio::MAGIC),
            reg::VERSION => policy.version.unwrap_or(virtio::VERSION),
            reg::DEVICE_ID => policy.device_id.unwrap_or(virtio::DEVICE_ID_BLOCK),
            reg::DEVICE_FEATURES => {
                let all = self.offered();
                if regs.device_features_sel == 0 { all as u32 } else { (all >> 32) as u32 }
            }
            reg::QUEUE_NUM_MAX => policy.queue_num_max.unwrap_or(u32::from(QUEUE_SIZE) * 4),
            reg::QUEUE_READY => {
                if policy.queue_ready_before && regs.queue_ready == 0 {
                    1
                } else if policy.queue_ready_stuck {
                    0
                } else {
                    regs.queue_ready
                }
            }
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
            reg::CONFIG => self.capacity() as u32,
            x if x == reg::CONFIG + 4 => (self.capacity() >> 32) as u32,
            _ => 0,
        };
        Ok(value)
    }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> {
        {
            let mut regs = self.regs.borrow_mut();
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
                    if regs.driver_features_sel == 0 {
                        regs.driver_features = (regs.driver_features & !0xffff_ffff) | u64::from(value);
                    } else {
                        regs.driver_features =
                            (regs.driver_features & 0xffff_ffff) | (u64::from(value) << 32);
                    }
                }
                reg::QUEUE_NUM => regs.queue_num = value,
                reg::QUEUE_DESC_LOW => regs.desc_addr = half(regs.desc_addr, value, false),
                reg::QUEUE_DESC_HIGH => regs.desc_addr = half(regs.desc_addr, value, true),
                reg::QUEUE_DRIVER_LOW => regs.avail_addr = half(regs.avail_addr, value, false),
                reg::QUEUE_DRIVER_HIGH => regs.avail_addr = half(regs.avail_addr, value, true),
                reg::QUEUE_DEVICE_LOW => regs.used_addr = half(regs.used_addr, value, false),
                reg::QUEUE_DEVICE_HIGH => regs.used_addr = half(regs.used_addr, value, true),
                reg::QUEUE_READY => regs.queue_ready = value,
                reg::INTERRUPT_ACK => self.irq.set(self.irq.get() & !value),
                _ => {}
            }
        }
        if off == reg::QUEUE_READY && value != 0 {
            // The driver says the rings are where it put them: check each is inside the region
            // the device was given, exactly as a device with an IOMMU in front of it would.
            let (desc, avail, used) = {
                let regs = self.regs.borrow();
                (regs.desc_addr, regs.avail_addr, regs.used_addr)
            };
            for addr in [desc, avail, used] {
                let _ = self.offset_of(addr);
            }
        }
        if off == reg::QUEUE_NOTIFY {
            self.process();
        }
        Ok(())
    }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> {
        if self.policy.get().scribble_on_every_read {
            self.scribble_reads.set(self.scribble_reads.get() + 1);
            // Between any two of the driver's reads, the rings change under it.
            let pattern = (self.scribble_reads.get() % 251) as u8;
            self.scribble(Scribble::Descriptors);
            self.scribble(Scribble::Avail);
            let mut dma = self.dma.borrow_mut();
            if let Some(rings) = dma.get_mut(crate::queue::USED_OFF..DATA_OFF) {
                rings.fill(pattern);
            }
        }
        let dma = self.dma.borrow();
        let bytes = dma.get(off..off + out.len()).ok_or(Fault::Bounds)?;
        out.copy_from_slice(bytes);
        Ok(())
    }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> {
        let mut dma = self.dma.borrow_mut();
        let slot = dma.get_mut(off..off + src.len()).ok_or(Fault::Bounds)?;
        slot.copy_from_slice(src);
        Ok(())
    }

    fn dma_phys(&self) -> u64 { FAKE_PHYS }

    fn dma_len(&self) -> usize { DMA_LEN }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> {
        // A deferred request is served while the driver is blocked, which is when a real one
        // would complete.
        if self.pending.replace(false) {
            self.run(&self.policy.get());
        }
        if self.irq.get() != 0 {
            self.now.set(self.now.get().saturating_add(1));
            return Ok(());
        }
        // Nothing pending: the kernel's `receive` times out, and the clock has moved on.
        self.now.set(self.now.get().saturating_add(timeout_us.max(1)));
        Err(Fault::Timeout)
    }

    fn now_us(&self) -> u64 { self.now.get() }
}

/// One 32-bit half of a 64-bit ring address, as virtio-mmio writes them (§4.2.2).
fn half(current: u64, value: u32, high: bool) -> u64 {
    if high {
        (current & 0xffff_ffff) | (u64::from(value) << 32)
    } else {
        (current & !0xffff_ffff) | u64::from(value)
    }
}

/// Draws a [`Policy`] from a stream of bytes: **the one place a hostile device is built at
/// random**, shared by the randomized sweep in `tests/device.rs` and by the `device` and
/// `request` fuzz targets. A lie added to [`Policy`] and not added here is a lie nothing
/// exercises, and one place to forget is better than three.
///
/// `next` hands out bytes; an exhausted source reads as zeros, so a short input is an honest
/// device rather than a refusal to run. **Every field draws its bytes whether or not its flag is
/// set**, so flipping one bit of the input changes one lie rather than shifting every field after
/// it, which is what lets a fuzzer keep what it has found.
pub fn policy_from(next: &mut impl FnMut() -> u8) -> Policy {
    let lies = u32_of(next);
    let on = |n: u32| (lies >> n) & 1 == 1;
    let (magic, version, device_id) = (u32_of(next), u32_of(next), u32_of(next));
    let features = u64_of(next);
    let queue_num_max = u32_of(next);
    let capacity = u64_of(next);
    let used_idx_delta = u32_of(next) as u16;
    let extra = u16::from(next() % 8);
    let (used_id, used_len) = (u32_of(next), u32_of(next));
    let status = next();
    let spurious = u32::from(next() % 4);
    let scribble = match next() % 5 {
        0 => Scribble::Descriptors,
        1 => Scribble::DescriptorLoop,
        2 => Scribble::Avail,
        3 => Scribble::Header,
        other => Scribble::Whole(other),
    };
    Policy {
        magic: on(0).then_some(magic),
        version: on(1).then_some(version),
        device_id: on(2).then_some(device_id),
        features: on(3).then_some(features),
        queue_num_max: on(4).then_some(queue_num_max),
        capacity: on(5).then_some(capacity),
        config_never_settles: on(6),
        never_resets: on(7),
        status_drops_bits: on(8),
        queue_ready_stuck: on(9),
        queue_ready_before: on(10),
        used_idx_delta: on(11).then_some(used_idx_delta),
        extra_used_entries: if on(12) { extra } else { 0 },
        used_id: on(13).then_some(used_id),
        used_len: on(14).then_some(used_len),
        blk_status: on(15).then_some(status),
        short_write: on(16),
        no_interrupt: on(17),
        never_complete: on(18),
        spurious_interrupts: if on(19) { spurious } else { 0 },
        defer: on(20),
        scribble: on(21).then_some(scribble),
        scribble_on_every_read: on(22),
        keep_writing_data: on(23),
    }
}

fn u32_of(next: &mut impl FnMut() -> u8) -> u32 { u32::from_le_bytes([next(), next(), next(), next()]) }

fn u64_of(next: &mut impl FnMut() -> u8) -> u64 { u64::from(u32_of(next)) | (u64::from(u32_of(next)) << 32) }
