//! The virtio-mmio transport (virtio 1.2, §4.2) and virtio-blk's own constants (§5.2): the
//! register map, the status handshake, feature negotiation, and reading the configuration space.
//!
//! Everything here reads numbers the device chose. Each one is checked before it is believed, and
//! **none of them is ever used as an index or a length**: the queue's size, the layout of the DMA
//! region and the length of every buffer are this crate's own constants ([`crate::queue`]).

use crate::transport::{Fault, Transport};

/// Register offsets in a virtio-mmio slot (virtio 1.2, §4.2.2). Only the ones `blkd` uses.
pub mod reg {
    pub const MAGIC: usize = 0x000;
    pub const VERSION: usize = 0x004;
    pub const DEVICE_ID: usize = 0x008;
    pub const DEVICE_FEATURES: usize = 0x010;
    pub const DEVICE_FEATURES_SEL: usize = 0x014;
    pub const DRIVER_FEATURES: usize = 0x020;
    pub const DRIVER_FEATURES_SEL: usize = 0x024;
    pub const QUEUE_SEL: usize = 0x030;
    pub const QUEUE_NUM_MAX: usize = 0x034;
    pub const QUEUE_NUM: usize = 0x038;
    pub const QUEUE_READY: usize = 0x044;
    pub const QUEUE_NOTIFY: usize = 0x050;
    pub const INTERRUPT_STATUS: usize = 0x060;
    pub const INTERRUPT_ACK: usize = 0x064;
    pub const STATUS: usize = 0x070;
    pub const QUEUE_DESC_LOW: usize = 0x080;
    pub const QUEUE_DESC_HIGH: usize = 0x084;
    pub const QUEUE_DRIVER_LOW: usize = 0x090;
    pub const QUEUE_DRIVER_HIGH: usize = 0x094;
    pub const QUEUE_DEVICE_LOW: usize = 0x0a0;
    pub const QUEUE_DEVICE_HIGH: usize = 0x0a4;
    pub const CONFIG_GENERATION: usize = 0x0fc;
    pub const CONFIG: usize = 0x100;
}

/// `"virt"` little-endian: the first register of a virtio-mmio slot.
pub const MAGIC: u32 = 0x7472_6976;
/// The only transport version this driver speaks. Version 1 ("legacy") has a different queue
/// layout and no feature word above 32; we do not carry two layouts for a device QEMU only
/// presents when asked to.
pub const VERSION: u32 = 2;
/// virtio-blk (virtio 1.2, §5.2).
pub const DEVICE_ID_BLOCK: u32 = 2;

/// Device status bits (§2.1).
pub mod status {
    pub const ACKNOWLEDGE: u32 = 1;
    pub const DRIVER: u32 = 2;
    pub const DRIVER_OK: u32 = 4;
    pub const FEATURES_OK: u32 = 8;
    pub const DEVICE_NEEDS_RESET: u32 = 64;
    pub const FAILED: u32 = 128;
}

/// Feature bits, as bit numbers in the 64-bit feature word.
pub mod feature {
    /// virtio-blk: the device is read-only (§5.2.3).
    pub const BLK_RO: u32 = 5;
    /// virtio-blk: the device understands a flush request, which is what makes `sync` mean
    /// anything (IO-ARCHITECTURE.md, `blkd`'s contract).
    pub const BLK_FLUSH: u32 = 9;
    /// The device is a 1.x device and uses the 1.x queue layout (§6).
    pub const VERSION_1: u32 = 32;
}

/// virtio-blk request types (§5.2.6).
pub mod request {
    pub const IN: u32 = 0;
    pub const OUT: u32 = 1;
    pub const FLUSH: u32 = 4;
}

/// virtio-blk request status bytes (§5.2.6). Anything else is a device that does not speak the
/// protocol.
pub mod blk_status {
    pub const OK: u8 = 0;
    pub const IOERR: u8 = 1;
    pub const UNSUPP: u8 = 2;
}

/// The sector, in bytes: virtio-blk's unit, fixed by the specification whatever the disk's own
/// block size (§5.2.6: "the offset (multiplied by 512)").
pub const SECTOR_SIZE: u32 = 512;

/// The most sectors one `read` or `write` may carry (IO-ARCHITECTURE.md, Bounds). 64 sectors is
/// 32 KiB: comfortably inside one `MAX_LEND_PAGES` lend with its encoding, and a whole number of
/// littlefs blocks at every block size `fsd` uses.
pub const MAX_SECTORS: u32 = 64;

/// The bytes of the data buffer: [`MAX_SECTORS`] sectors.
pub const DATA_LEN: usize = (MAX_SECTORS * SECTOR_SIZE) as usize;

/// How long `blkd` waits for one request to complete before giving up on the device. A device
/// that misses this is marked broken, because a completion that arrives later would be a used
/// entry for a request `blkd` is no longer tracking.
pub const REQUEST_TIMEOUT_US: u64 = 10_000_000;

/// How many times the configuration space is re-read when `ConfigGeneration` changes underneath
/// it (§4.2.2.2). A device that never settles is refused rather than read racily.
pub const CONFIG_TRIES: u32 = 8;

/// How many times the reset is polled before the device is declared dead.
pub const RESET_TRIES: u32 = 1024;

/// The device would not start, or stopped speaking the protocol. Every variant is a refusal:
/// nothing here is recoverable, because a device that has lied once is not trusted again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    /// The seam failed ([`Fault`]).
    Fault(Fault),
    /// Not a virtio-mmio slot, not transport version 2, or not a block device.
    NotBlockDevice,
    /// The device does not offer a feature `blkd` cannot work without, or refused the set we
    /// accepted.
    Features,
    /// The queue is too small for one request, or the device would not make it ready.
    Queue,
    /// The configuration space would not hold still, or says a capacity no disk can have.
    Config,
    /// The device is read-only and was asked to write.
    ReadOnly,
    /// The sectors asked for are not a whole run inside the disk and this driver's per-request
    /// bound. Nothing was sent to the device.
    Range,
    /// The device answered the request with virtio-blk status `IOERR` or `UNSUPP`: a failure it
    /// is entitled to report, so the request fails and the device is not broken.
    Rejected,
    /// The device failed the request, or said something the protocol does not allow: a used index
    /// that did not advance by one, an entry for a descriptor `blkd` did not submit, a length
    /// larger than the buffers it was given, or a status byte outside the three §5.2.6 defines.
    Io,
    /// The device did not complete a request before [`REQUEST_TIMEOUT_US`].
    Timeout,
    /// A request was made after one of the above. The device stays broken for the life of the
    /// process: `init` restarts `blkd`, which resets the device from the beginning.
    Broken,
}

impl From<Fault> for DeviceError {
    fn from(fault: Fault) -> DeviceError {
        match fault {
            Fault::Timeout => DeviceError::Timeout,
            other => DeviceError::Fault(other),
        }
    }
}

/// The bit `n` of a 64-bit feature word.
pub const fn bit(n: u32) -> u64 { 1u64 << n }

/// Reads a virtio-mmio slot's identification registers and refuses anything that is not a
/// version-2 block device, before writing a single register.
pub fn identify(t: &impl Transport) -> Result<(), DeviceError> {
    if t.reg_read(reg::MAGIC)? != MAGIC
        || t.reg_read(reg::VERSION)? != VERSION
        || t.reg_read(reg::DEVICE_ID)? != DEVICE_ID_BLOCK
    {
        return Err(DeviceError::NotBlockDevice);
    }
    Ok(())
}

/// What feature negotiation settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    /// The device declared itself read-only, so every `write` is refused before it is issued.
    pub read_only: bool,
}

/// Resets the device and takes it through the status handshake (§3.1.1) up to `FEATURES_OK`.
///
/// `blkd` accepts exactly three bits and offers no others: `VERSION_1`, without which the queue
/// layout is not the one below; `BLK_FLUSH`, without which `sync` could not be honoured and the
/// contract `fsd` rests on would be a lie (IO-ARCHITECTURE.md); and `BLK_RO`, which is not needed
/// but is accepted so that a read-only device is refused at the door rather than per write.
/// Everything else — indirect descriptors, event indices, discard, write zeroes, multiqueue — is
/// left unaccepted, so the device may not use any of it and the ring stays the one described in
/// [`crate::queue`].
pub fn negotiate(t: &impl Transport) -> Result<Features, DeviceError> {
    reset(t)?;
    set_status(t, status::ACKNOWLEDGE)?;
    set_status(t, status::ACKNOWLEDGE | status::DRIVER)?;

    t.reg_write(reg::DEVICE_FEATURES_SEL, 0)?;
    let low = u64::from(t.reg_read(reg::DEVICE_FEATURES)?);
    t.reg_write(reg::DEVICE_FEATURES_SEL, 1)?;
    let high = u64::from(t.reg_read(reg::DEVICE_FEATURES)?);
    let offered = low | (high << 32);

    let required = bit(feature::VERSION_1) | bit(feature::BLK_FLUSH);
    if offered & required != required {
        return Err(DeviceError::Features);
    }
    let accepted = required | (offered & bit(feature::BLK_RO));

    t.reg_write(reg::DRIVER_FEATURES_SEL, 0)?;
    t.reg_write(reg::DRIVER_FEATURES, accepted as u32)?;
    t.reg_write(reg::DRIVER_FEATURES_SEL, 1)?;
    t.reg_write(reg::DRIVER_FEATURES, (accepted >> 32) as u32)?;

    set_status(t, status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK)?;
    let now = t.reg_read(reg::STATUS)?;
    if now & status::FEATURES_OK == 0 || now & (status::FAILED | status::DEVICE_NEEDS_RESET) != 0 {
        return Err(DeviceError::Features);
    }
    Ok(Features { read_only: accepted & bit(feature::BLK_RO) != 0 })
}

/// Tells the device the driver is ready (the last step of §3.1.1).
pub fn driver_ok(t: &impl Transport) -> Result<(), DeviceError> {
    set_status(t, status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK)
}

/// Writes the status register and reads it back, refusing a device that dropped a bit or set
/// `FAILED` or `DEVICE_NEEDS_RESET`.
fn set_status(t: &impl Transport, bits: u32) -> Result<(), DeviceError> {
    t.reg_write(reg::STATUS, bits)?;
    let now = t.reg_read(reg::STATUS)?;
    if now & bits != bits || now & (status::FAILED | status::DEVICE_NEEDS_RESET) != 0 {
        return Err(DeviceError::NotBlockDevice);
    }
    Ok(())
}

/// Resets the device: write 0 and wait for the status register to read 0 (§4.2.3.1). A device
/// that will not reset is dead; the wait is bounded so that one cannot hang `blkd` at boot.
fn reset(t: &impl Transport) -> Result<(), DeviceError> {
    t.reg_write(reg::STATUS, 0)?;
    for _ in 0..RESET_TRIES {
        if t.reg_read(reg::STATUS)? == 0 {
            return Ok(());
        }
    }
    Err(DeviceError::NotBlockDevice)
}

/// The disk's capacity in 512-byte sectors, read out of the configuration space under
/// `ConfigGeneration` (§4.2.2.2), and checked: a disk of no sectors, or of so many that its byte
/// length does not fit a `u64`, is a configuration `blkd` refuses rather than computes with.
pub fn capacity(t: &impl Transport) -> Result<u64, DeviceError> {
    for _ in 0..CONFIG_TRIES {
        let before = t.reg_read(reg::CONFIG_GENERATION)?;
        let low = u64::from(t.reg_read(reg::CONFIG)?);
        let high = u64::from(t.reg_read(reg::CONFIG + 4)?);
        let after = t.reg_read(reg::CONFIG_GENERATION)?;
        if before != after {
            continue;
        }
        let sectors = low | (high << 32);
        if sectors == 0 || sectors > u64::MAX / u64::from(SECTOR_SIZE) {
            return Err(DeviceError::Config);
        }
        return Ok(sectors);
    }
    Err(DeviceError::Config)
}

/// Acknowledges whatever interrupt bits are set. Only the bits that were read are acknowledged,
/// so a bit the device raises between the two accesses is not lost.
///
/// **It is called whether or not the driver waited for the interrupt.** virtio requires the driver
/// to acknowledge one it was sent (§4.2.2), and a device that completed inside the doorbell write
/// still asserted its line; the kernel masking the source until the next `receive` (R5) makes
/// missing this survivable, not correct. Which bits were set is not returned, because nothing
/// branches on it: the ring is what says whether a request completed.
pub fn ack_interrupt(t: &impl Transport) -> Result<(), DeviceError> {
    let bits = t.reg_read(reg::INTERRUPT_STATUS)?;
    if bits != 0 {
        t.reg_write(reg::INTERRUPT_ACK, bits)?;
    }
    Ok(())
}
