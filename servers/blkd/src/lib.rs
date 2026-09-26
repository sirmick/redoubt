//! `blkd`: the virtio-blk driver. It owns one disk, reads its partition table once, and serves
//! each partition to one `fsd` as a range of sectors (servers/blkd.md).
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
//! `virtio-drivers` (rcore-os) was read at 0.13.0 and rejected under tenet 5; the reasoning is in
//! servers/blkd.md, "Why", where `netd` will find it rather than re-argue it. The short of it: it
//! is careful where it matters (it shadows the descriptor table, and refuses a used entry whose id
//! is not the token awaited), but it is 13k lines of device classes we do not have behind six
//! dependencies, with an interface that is `unsafe` at every call site. [`queue`] and [`virtio`]
//! are about 500 lines with no dependencies, and with one request outstanding there is no
//! descriptor state to shadow at all.

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
pub use server::BlockServer;
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

/// Reads the partition table once and turns it into the root ranges, **one slot per GPT entry**:
/// slot *i* is the range badge *i* + 1 names, and `None` is an entry the table does not use
/// (servers/blkd.md, "Ranges and badges").
///
/// The holes are the point. A badge names an entry of the array, not a position among the entries
/// that happen to be in use, so a gap — which `gdisk` leaves routinely — does not renumber every
/// volume after it. A manifest naming a gap gets `not_permitted` from every request, which is a
/// refusal somebody notices; compacting the list would instead hand a filesystem the wrong volume
/// with no error anywhere.
///
/// It is read **once**, at start-up, and never re-read: every range is fixed against the capacity
/// the device reported then, so a device that changes either afterwards changes nothing.
pub fn read_partitions<T: Transport>(disk: &mut Disk<T>) -> Result<Vec<Option<Range>>, TableError> {
    let sector_bytes = SECTOR_SIZE as usize;
    let mut header = vec![0; sector_bytes];
    disk.read(gpt::HEADER_LBA, &mut header).map_err(TableError::Device)?;
    let at = gpt::header(&header, disk.sectors()).map_err(TableError::Table)?;
    // `at.sectors` is at most `MAX_ARRAY_BYTES / SECTOR_SIZE`, which is one request's worth, so
    // the whole array is read in one go and its size is bounded before anything is allocated.
    let mut array = vec![0; at.sectors as usize * sector_bytes];
    disk.read(at.lba, &mut array).map_err(TableError::Device)?;
    let found = gpt::partitions(&at, &array).map_err(TableError::Table)?;
    let mut roots: Vec<Option<Range>> = Vec::new();
    // `at.entries` is at most `gpt::MAX_PARTITIONS`, checked when the header was read.
    roots.try_reserve(at.entries as usize).map_err(|_| TableError::Table(gpt::GptError::NoMemory))?;
    roots.resize(at.entries as usize, None);
    for partition in &found {
        // `gpt::partitions` already checked both against the usable range, which is inside the
        // disk; `Range::new` is the one constructor, so the check is not skipped.
        let range = Range::new(partition.first_lba, partition.sectors, disk.sectors())
            .map_err(|_| TableError::Table(gpt::GptError::BadPartition))?;
        // The index came from `0..at.entries`, so the slot is there.
        let slot =
            roots.get_mut(partition.index as usize).ok_or(TableError::Table(gpt::GptError::BadArray))?;
        *slot = Some(range);
    }
    Ok(roots)
}
