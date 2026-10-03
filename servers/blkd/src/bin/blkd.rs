//! `blkd`, the program: take the device out of the startup block, bring the disk up, read its
//! partition table once, then answer calls on the endpoint named `blkd` until that endpoint is
//! destroyed.
//!
//! Everything it can do is in `redoubt-blkd`'s library, so host tests drive the same code against
//! a hostile fake device and the runtime's fake kernel (`tests/`).
//!
//! **Its device is handed in, not discovered** (servers/blkd.md, "Started by `init`"): the startup
//! block names two handles, [`DISK`] (the MMIO region, with the DMA flag) and [`DISK_IRQ`] (its
//! interrupt), which `init` places there from the boot manifest's `devices` list. `blkd` parses
//! no device tree and hardcodes no address; without both handles it does not start, which is the
//! honest thing for a driver that has no device.
//!
//! **Its ranges' labels are handed in too:** one argument per labelled volume, `labels.P=ID,...`
//! for GPT entry P ([`redoubt_blkd::args`]), parsed against the table before anything is served;
//! an argument it cannot take stops it with [`BAD_ARGS`].

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_blkd::args::range_labels;
use redoubt_blkd::kernel::Device;
use redoubt_blkd::server::BlockServer;
use redoubt_blkd::{Disk, read_partitions};
use redoubt_rt::handle::{Endpoint, Irq, Mmio};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint for `blkd` to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// The startup block named no [`DISK`] handle, or no [`DISK_IRQ`] handle, so there is no disk to
/// serve.
pub const NO_DEVICE: u32 = 5;

/// The startup-block name of the MMIO device object `blkd` drives: the boot manifest's `devices`
/// entry for the disk, which must carry the DMA flag (servers/blkd.md).
pub const DISK: &str = "disk";
/// The startup-block name of that device's interrupt.
pub const DISK_IRQ: &str = "disk-irq";
/// The device would not start, or is not a virtio-blk device (`redoubt_blkd::DeviceError`).
pub const NO_DISK: u32 = 6;
/// The disk has no usable partition table (`redoubt_blkd::TableError`). Fail closed and loudly:
/// a partition `blkd` cannot read is a volume `fsd` cannot mount, and serving without it would
/// look like the volume simply not existing. `init` restarts `blkd`, which reads the same disk
/// and exits again, so an unreadable disk is a reboot loop rather than a degraded boot
/// (servers/blkd.md, "Failure and restart"; servers/init.md, "Restarts and reboots").
pub const NO_PARTITIONS: u32 = 7;
/// An argument that is not `labels.P=ID[,ID...]`, names P twice, or names no partition
/// (`redoubt_blkd::args`): `blkd` never serves a range under labels it misread.
pub const BAD_ARGS: u32 = 4;

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("blkd") else { return NO_ENDPOINT };
    let (Some(mmio), Some(irq)) = (startup.handle(DISK), startup.handle(DISK_IRQ)) else {
        return NO_DEVICE;
    };
    let Ok(device) = Device::open(Mmio::from_handle(mmio), Irq::from_handle(irq)) else {
        return NO_DEVICE;
    };
    let Ok(mut disk) = Disk::new(device) else { return NO_DISK };
    let Ok(roots) = read_partitions(&mut disk) else { return NO_PARTITIONS };
    let Ok(labels) = range_labels(startup.args(), &roots) else { return BAD_ARGS };
    let mut server = BlockServer::new(disk, roots, labels);
    let endpoint = Endpoint::from_handle(handle);
    // The handler answers every call; what it returns is dropped.
    redoubt_rt::server::serve(&endpoint, |request| server.serve(request))
}
