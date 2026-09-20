//! `blkd`, the program: take the device out of the startup block, bring the disk up, read its
//! partition table once, then answer calls on the endpoint named `blkd` until that endpoint is
//! destroyed.
//!
//! Everything it can do is in `redoubt-blkd`'s library, so host tests drive the same code against
//! a hostile fake device and the runtime's fake kernel (`tests/`).
//!
//! **Its device is handed in, not discovered** (IO-ARCHITECTURE.md, Driver model): the startup
//! block names two handles, [`DISK`] (the MMIO region, with the DMA flag) and [`DISK_IRQ`] (its
//! interrupt), which `init` places there from the boot manifest's `devices` list. `blkd` parses
//! no device tree and hardcodes no address; without both handles it does not start, which is the
//! honest thing for a driver that has no device.
//!
//! **Pending on WP-R3.** WP-K3's device objects, `dma_alloc` and IRQ receive are merged and wired
//! here. What is still missing is the `init` that reads the boot manifest, creates `blkd`'s
//! endpoint and writes this startup block (and WP-K4's `process_start`, which puts the handles in
//! the table), so nothing starts this program yet; until then its behaviour is covered by host
//! tests against a hostile fake device (`blkd-host-tests`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_blkd::kernel::Device;
use redoubt_blkd::server::{BUDGET, BlockServer, COST, LIMITS};
use redoubt_blkd::{Disk, read_partitions};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::{Endpoint, Irq, Mmio};
use redoubt_rt::ipc::Event;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint for `blkd` to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// The limits in this build do not fit the budget or the open-call headroom: a build-time
/// mistake, caught at the only moment it can be.
pub const BAD_LIMITS: u32 = 4;
/// The startup block named no [`DISK`] handle, or no [`DISK_IRQ`] handle, so there is no disk to
/// serve.
pub const NO_DEVICE: u32 = 5;

/// The startup-block name of the MMIO device object `blkd` drives: the boot manifest's `devices`
/// entry for the disk, which must carry the DMA flag (IO-ARCHITECTURE.md).
pub const DISK: &str = "disk";
/// The startup-block name of that device's interrupt.
pub const DISK_IRQ: &str = "disk-irq";
/// The device would not start, or is not a virtio-blk device (`redoubt_blkd::DeviceError`).
pub const NO_DISK: u32 = 6;
/// The disk has no usable partition table (`redoubt_blkd::TableError`). Fail closed and loudly:
/// a partition `blkd` cannot read is a volume `fsd` cannot mount, and serving without it would
/// look like the volume simply not existing.
pub const NO_PARTITIONS: u32 = 7;
/// The kernel would not give a random word. `blkd`'s first granted badge is drawn from one
/// (answer 126), and a predictable one is a hole across a restart, so it does not start
/// without it.
pub const NO_RANDOM: u32 = 8;

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("blkd") else { return NO_ENDPOINT };
    let (Some(mmio), Some(irq)) = (startup.handle(DISK), startup.handle(DISK_IRQ)) else {
        return NO_DEVICE;
    };
    let mmio = Mmio::from_handle(mmio);
    let Ok(device) = Device::open(&mmio, Irq::from_handle(irq)) else { return NO_DEVICE };
    let Ok(mut disk) = Disk::new(device) else { return NO_DISK };
    let Ok(roots) = read_partitions(&mut disk) else { return NO_PARTITIONS };
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = BlockServer::new(disk, roots, LIMITS, &COST, BUDGET, random) else {
        return BAD_LIMITS;
    };
    let endpoint = Endpoint::from_handle(handle);
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // A failed reply means the caller is gone; there is nobody to tell.
                let _ = server.serve(request);
            }
            // Every message of this protocol is a `call` (WIRE.md, answer 98). A `send` is
            // dropped, and what it brought is closed, so it cannot grow the handle table.
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            // No call is ever held open here: every request is answered as it is taken, so no
            // abandoned-call notice can name one. The interrupt arrives on the IRQ handle inside
            // a request, never here.
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
    }
}
