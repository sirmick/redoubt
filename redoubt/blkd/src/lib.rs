//! `blkd`: the virtio-blk driver. It owns one disk, reads its partition table once, and serves
//! each partition to one `fsd` as a range of sectors (IO-ARCHITECTURE.md, Storage).
//!
//! # What this is trusted for
//! On QEMU no hardware confines DMA, so `blkd` is inside the TCB (TENETS.md 7): it programs a
//! device with physical addresses, and a device that ignores them can write anywhere in RAM. No
//! code here can make that untrue. What it is written to guarantee is the other half, and it is
//! stated so it can be checked:
//!
//! 1. **`blkd` never asks the device to touch anything but the pages `dma_alloc` gave it.** Every address it
//!    writes into a descriptor or a queue base register is `dma_phys() + <a constant of [`queue`]>`, and
//!    those constants are checked at compile time to lie inside the region ([`queue::LAYOUT_FITS`]). A
//!    client's lent pages are never named to the device: a write is copied into the DMA buffer first, and a
//!    read is copied out of it afterwards.
//! 2. **Nothing the device says can corrupt `blkd`'s memory or panic it.** No device value is ever used as an
//!    index or a length. The descriptor table and the available ring are written from constants for every
//!    request and never read back, so a device that rewrites them — a chain that loops, a `next` out of
//!    range, a length that overflows — changes nothing `blkd` believes. Of the used ring, three values are
//!    read and each is checked against what was sent. A device that breaks the protocol is marked broken and
//!    never spoken to again.
//! 3. **A filesystem sees only its partition.** A client's sector numbers are relative to its own range, so
//!    there is no address it can write down that names a sector outside it ([`range::Range`]), and a
//!    partition table with overlapping entries is refused whole ([`gpt`]).
//!
//! # Shape
//! - [`transport`]: the one seam to the kernel — MMIO registers, the DMA region, the interrupt and the clock.
//!   Everything else in the crate is written against it, so the whole driver runs in host tests against
//!   [`fake::FakeDevice`], a deliberately hostile virtio-blk device.
//! - [`virtio`]: the virtio-mmio register map, the status handshake, feature negotiation and the
//!   configuration space.
//! - [`queue`]: the split virtqueue, one request outstanding, and the layout of the DMA region.
//! - [`disk`]: bring-up, and read, write and flush over the whole disk.
//! - [`gpt`]: the partition table, the one on-disk structure `blkd` parses.
//! - [`range`]: a block range, which is what a badge names.
//! - [`server`]: the typed protocol, with `admit` and `check` on every request.
//!
//! # Why the queue is ours
//! BUILD-PLAN.md names the `virtio-drivers` crate (rcore-os). It was read at 0.13.0 and judged
//! under TENETS.md 5, which asks for small, `no_std`, pure Rust, maintained, **and read by us**.
//! It is maintained and pure Rust, and its split-queue handling is careful in the way that
//! matters: it keeps a private shadow of the descriptor table, so a device that rewrites the real
//! one cannot steer the driver's walk of it, and `pop_used` refuses a used entry whose id is not
//! the token the caller is waiting for.
//!
//! It is not small, and that is the whole of the objection. 13,000 lines cover nine device
//! classes we do not have (GPU, sound, vsock, console, input, 9P, RNG, net) and two transports we
//! do not use (PCI, and x86-64 hypercalls), behind six dependencies — `zerocopy` and its derive
//! macro, `bitflags`, `enumn`, `thiserror`, `log`, `safe-mmio` — every one of which would enter
//! the TCB with it. Its interface is `unsafe` where ours has to be safe: `VirtIOBlk`'s
//! non-blocking calls, `VirtQueue::pop_used` and the whole `Hal` trait are `unsafe fn`, so using
//! it means writing our own `unsafe` at every call site and an `unsafe impl Hal` besides, and its
//! internals `assert!` and `panic!` on conditions our `unsafe` would be promising — a promise we
//! could only keep by reading all 13,000 lines and keeping them in our head, which is what tenet
//! 1 says we must be able to do and what this size makes impossible. `pop_used` also hands back
//! the device's `len` unvalidated, which is a length we would have to check anyway.
//!
//! What `blkd` needs is one split virtqueue with **one request outstanding**, one MMIO transport,
//! and three request types. That is [`queue`] and [`virtio`]: about 500 lines with no
//! dependencies, no `unsafe` outside the seam, and a stronger property than the shadow table
//! gives — with one request in flight there is no descriptor state to shadow, because the table
//! is rewritten from constants before every request and never read.

#![no_std]
// `unsafe` lives in [`kernel`] and nowhere else, and the compiler is what says so: the module
// below carries the only `#[allow(unsafe_code)]` in the crate.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

pub mod disk;
pub mod gpt;
#[allow(unsafe_code)]
pub mod kernel;
pub mod queue;
pub mod range;
pub mod server;
pub mod transport;
pub mod virtio;

#[cfg(not(target_os = "none"))]
pub mod fake;
#[cfg(not(target_os = "none"))]
pub mod image;

pub use disk::Disk;
pub use range::Range;
pub use server::{BUDGET, BlockServer, COST, LIMITS};
pub use transport::{Fault, Transport};
pub use virtio::{DeviceError, SECTOR_SIZE};

/// The disk has no usable partition table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    /// The device failed while the table was being read.
    Device(DeviceError),
    /// The table itself is malformed ([`gpt::GptError`]).
    Table(gpt::GptError),
}

/// Reads the partition table once and turns it into the root ranges, in GPT entry order: index
/// *i* is the range badge *i* + 1 names (IO-ARCHITECTURE.md).
///
/// It is read **once**, at start-up, and never re-read: every range is fixed against the capacity
/// the device reported then, so a device that changes either afterwards changes nothing.
pub fn read_partitions<T: Transport>(disk: &mut Disk<T>) -> Result<Vec<Range>, TableError> {
    let sector_bytes = SECTOR_SIZE as usize;
    let mut header = vec![0; sector_bytes];
    disk.read(gpt::HEADER_LBA, &mut header).map_err(TableError::Device)?;
    let at = gpt::header(&header, disk.sectors()).map_err(TableError::Table)?;
    // `at.sectors` is at most `MAX_ARRAY_BYTES / SECTOR_SIZE`, which is one request's worth, so
    // the whole array is read in one go and its size is bounded before anything is allocated.
    let mut array = vec![0; at.sectors as usize * sector_bytes];
    disk.read(at.lba, &mut array).map_err(TableError::Device)?;
    let found = gpt::partitions(&at, &array).map_err(TableError::Table)?;
    let mut roots = Vec::new();
    roots.try_reserve(found.len()).map_err(|_| TableError::Table(gpt::GptError::NoMemory))?;
    for partition in &found {
        // `gpt::partitions` already checked both against the usable range, which is inside the
        // disk; this is the one constructor, so the check is not skipped.
        let range = Range::new(partition.first_lba, partition.sectors, disk.sectors())
            .map_err(|_| TableError::Table(gpt::GptError::BadPartition))?;
        roots.push(range);
    }
    Ok(roots)
}
