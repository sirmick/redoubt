//! The virtio-mmio transport (virtio 1.2, §4.2) and virtio-net's own constants (§5.1): the
//! register map, the status handshake, feature negotiation and the MAC.
//!
//! Everything here reads numbers the device chose. Each one is checked before it is believed, and
//! none of them is ever used as an index or a length: the queues' sizes, the layout of the DMA
//! regions and the length of every buffer are this crate's own constants ([`crate::ring`]).

use crate::transport::{Fault, Transport};

/// Register offsets in a virtio-mmio slot (virtio 1.2, §4.2.2). Only the ones `netd` uses.
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
    /// virtio-net's configuration space: `mac: u8[6]` first (§5.1.4).
    pub const CONFIG: usize = 0x100;
}

/// `"virt"` little-endian.
pub const MAGIC: u32 = 0x7472_6976;
/// The only transport version this driver speaks, as `blkd`: version 1 ("legacy") has another
/// queue layout, and QEMU presents it only unless told not to (the bench's `MODERN_VIRTIO`).
pub const VERSION: u32 = 2;
/// virtio-net (virtio 1.2, §5.1).
pub const DEVICE_ID_NET: u32 = 1;

/// The receive queue and the transmit queue (§5.1.2).
pub const RX_QUEUE: u32 = 0;
pub const TX_QUEUE: u32 = 1;

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
    /// virtio-net: the device has a MAC address in its configuration space (§5.1.3).
    pub const MAC: u32 = 5;
    /// The device is a 1.x device: the 1.x queue layout, and a 12-byte header on every packet.
    pub const VERSION_1: u32 = 32;
}

/// The header virtio 1.x puts in front of every packet, both ways (§5.1.6): `flags: u8`,
/// `gso_type: u8`, `hdr_len`, `gso_size`, `csum_start`, `csum_offset`, `num_buffers` (`u16`
/// each). With `VERSION_1` it is always this long.
pub const NET_HDR_LEN: usize = 12;

/// The shortest and longest Ethernet frame `netd` carries, without the FCS: a header of 14
/// bytes, and the 1500-byte MTU's payload after it.
pub const MIN_FRAME: usize = 14;
pub const MAX_FRAME: usize = 1514;
/// The MTU `info` reports: `netd` accepts no MTU feature, so the standard one.
pub const MTU: u32 = 1500;

/// How long a transmit slot may stay with the device before `netd` gives up on it, as `blkd`'s
/// request timeout: ten seconds, after which the device is broken.
pub const TX_TIMEOUT_US: u64 = 10_000_000;

/// How many times the configuration space is re-read when `ConfigGeneration` changes underneath
/// it (§4.2.2.2). A device that never settles is refused.
pub const CONFIG_TRIES: u32 = 8;

/// How many times a reset is polled before the device is declared dead.
pub const RESET_TRIES: u32 = 1024;

/// The device would not start, or stopped speaking the protocol. Every variant is a refusal:
/// nothing here is recovered from, because a device that lied once is not trusted again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    /// The seam failed ([`Fault`]).
    Fault(Fault),
    /// Not a virtio-mmio slot, not transport version 2, not a network device, or a status
    /// handshake the device would not complete.
    NotNetDevice,
    /// The device does not offer `VERSION_1` and `MAC`, or refused the set we accepted.
    Features,
    /// A queue too small, one already live, or one the device would not make ready.
    Queue,
    /// The configuration would not hold still, or the MAC is not a unicast address.
    Config,
    /// The device broke the ring protocol: a used index past what is outstanding, an id that is
    /// out of range, not outstanding or seen twice, a length outside what it was given, or a
    /// header asking for what was never negotiated. (A frame of the wrong length is not a lie:
    /// [`crate::rxq`].)
    Lie,
    /// A transmit slot stayed with the device longer than [`TX_TIMEOUT_US`].
    Timeout,
}

impl From<Fault> for DeviceError {
    fn from(fault: Fault) -> DeviceError { DeviceError::Fault(fault) }
}

/// The bit `n` of a 64-bit feature word.
pub const fn bit(n: u32) -> u64 { 1u64 << n }

/// Reads the identification registers and refuses anything that is not a version-2 network
/// device, before writing a single register.
pub fn identify(t: &impl Transport) -> Result<(), DeviceError> {
    if t.reg_read(reg::MAGIC)? != MAGIC
        || t.reg_read(reg::VERSION)? != VERSION
        || t.reg_read(reg::DEVICE_ID)? != DEVICE_ID_NET
    {
        return Err(DeviceError::NotNetDevice);
    }
    Ok(())
}

/// Resets the device and takes it through the handshake (§3.1.1) up to `FEATURES_OK`.
///
/// `netd` accepts exactly two bits and offers no others: `VERSION_1`, without which the queue
/// layout and the header are not the ones below, and `MAC`, without which the device has no
/// address to give `ipd`. Everything else (checksum offload, segmentation, mergeable buffers,
/// the control queue, multiqueue, event indices, indirect descriptors) stays unaccepted, so the
/// device may send no GSO, no merged buffers and no checksum offload, and every header it writes
/// must be zeros ([`crate::rxq`] checks).
pub fn negotiate(t: &impl Transport) -> Result<(), DeviceError> {
    reset(t)?;
    set_status(t, status::ACKNOWLEDGE)?;
    set_status(t, status::ACKNOWLEDGE | status::DRIVER)?;

    t.reg_write(reg::DEVICE_FEATURES_SEL, 0)?;
    let low = u64::from(t.reg_read(reg::DEVICE_FEATURES)?);
    t.reg_write(reg::DEVICE_FEATURES_SEL, 1)?;
    let high = u64::from(t.reg_read(reg::DEVICE_FEATURES)?);
    let offered = low | (high << 32);

    let accepted = bit(feature::VERSION_1) | bit(feature::MAC);
    if offered & accepted != accepted {
        return Err(DeviceError::Features);
    }
    t.reg_write(reg::DRIVER_FEATURES_SEL, 0)?;
    t.reg_write(reg::DRIVER_FEATURES, accepted as u32)?;
    t.reg_write(reg::DRIVER_FEATURES_SEL, 1)?;
    t.reg_write(reg::DRIVER_FEATURES, (accepted >> 32) as u32)?;

    set_status(t, status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK)?;
    let now = t.reg_read(reg::STATUS)?;
    if now & status::FEATURES_OK == 0 || now & (status::FAILED | status::DEVICE_NEEDS_RESET) != 0 {
        return Err(DeviceError::Features);
    }
    Ok(())
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
        return Err(DeviceError::NotNetDevice);
    }
    Ok(())
}

/// Resets the device: write 0, and poll until the status register reads 0 (§4.2.3.1), a bounded
/// number of times. After a reset the device may not touch its rings, so this is also how `netd`
/// stops a device it no longer trusts, or is about to leave (servers/netd.md R57).
pub fn reset(t: &impl Transport) -> Result<(), DeviceError> {
    t.reg_write(reg::STATUS, 0)?;
    for _ in 0..RESET_TRIES {
        if t.reg_read(reg::STATUS)? == 0 {
            return Ok(());
        }
    }
    Err(DeviceError::NotNetDevice)
}

/// The device's MAC, read byte by byte under `ConfigGeneration` (§4.2.2.2), as a `u64` whose low
/// 48 bits are the address, first octet lowest (the `netif` `info` reply). A MAC that is zero,
/// or has the group bit set (multicast, broadcast), is refused: no frame could be addressed to it
/// honestly, and `ipd`'s stack would refuse it anyway.
pub fn mac(t: &impl Transport) -> Result<u64, DeviceError> {
    for _ in 0..CONFIG_TRIES {
        let before = t.reg_read(reg::CONFIG_GENERATION)?;
        let mut octets = [0u8; 6];
        for (i, octet) in octets.iter_mut().enumerate() {
            *octet = t.reg_read_u8(reg::CONFIG + i)?;
        }
        let after = t.reg_read(reg::CONFIG_GENERATION)?;
        if before != after {
            continue;
        }
        if !unicast(octets) {
            return Err(DeviceError::Config);
        }
        return Ok(octets.iter().rev().fold(0u64, |mac, octet| (mac << 8) | u64::from(*octet)));
    }
    Err(DeviceError::Config)
}

/// Whether `octets` is an address one host can own: not all zeros, and the group bit (the low
/// bit of the first octet, set for multicast and for broadcast) clear.
pub fn unicast(octets: [u8; 6]) -> bool { octets != [0; 6] && octets[0] & 1 == 0 }

/// Acknowledges whatever interrupt bits are set; only the bits read are acknowledged, so one the
/// device raises between the two accesses is not lost.
pub fn ack_interrupt(t: &impl Transport) -> Result<(), DeviceError> {
    let bits = t.reg_read(reg::INTERRUPT_STATUS)?;
    if bits != 0 {
        t.reg_write(reg::INTERRUPT_ACK, bits)?;
    }
    Ok(())
}
