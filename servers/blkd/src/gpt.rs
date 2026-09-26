//! The GUID Partition Table (UEFI 2.10, §5.3), the one on-disk structure `blkd` parses.
//!
//! **The medium is hostile.** Every length, LBA and count here comes off a disk `blkd` does not
//! control, so each is checked before it is used, and the parser never indexes with a number it
//! read: the entry array is sliced with `get`, its size is bounded before it is allocated, and
//! every partition's first and last LBA are checked against the disk's capacity and against the
//! header's own usable range. A malformed table yields [`GptError`], never a panic.
//!
//! **Overlaps are refused.** Two partitions that share a sector would let two volumes alias each
//! other's bytes, which is the containment `blkd` exists to give (a filesystem sees only its
//! partition, servers/blkd.md R53). A table with an overlap is refused whole rather than in part,
//! so there is no question of which volume won.
//!
//! **Primary header only.** `blkd` never writes a partition table, so a table that does not check
//! out is a disk to refuse, not damage to repair: there is no fallback to the backup header at
//! the last LBA, and no attempt to rebuild one from the other. The protective MBR at LBA 0 is not
//! read at all; it protects tools that are not us.

use alloc::vec::Vec;

use crate::virtio::SECTOR_SIZE;

/// The signature at the start of the header: `"EFI PART"`.
pub const SIGNATURE: &[u8; 8] = b"EFI PART";

/// The LBA the primary header sits at (UEFI 2.10, §5.3.1).
pub const HEADER_LBA: u64 = 1;

/// The smallest a header may claim to be: through `PartitionEntryArrayCRC32` (§5.3.2, table 5.5).
pub const MIN_HEADER_SIZE: u32 = 92;

/// Partitions `blkd` will read. The UEFI default is 128, and a table claiming more is refused
/// rather than truncated, so nothing is silently dropped.
pub const MAX_PARTITIONS: u32 = 128;

/// The smallest and largest partition-entry size accepted. §5.3.3 requires a multiple of 128; a
/// larger one only adds vendor padding, so a cap keeps the array inside one read.
pub const MIN_ENTRY_SIZE: u32 = 128;
pub const MAX_ENTRY_SIZE: u32 = 512;

/// The most bytes of entry array `blkd` reads, so the whole table is a bounded amount of work and
/// of memory whatever the header claims: 64 sectors, which is one [`crate::virtio::MAX_SECTORS`]
/// read.
pub const MAX_ARRAY_BYTES: u32 = 64 * SECTOR_SIZE;

/// Why a partition table was refused. Every one is "this disk is not usable", and `blkd` says no
/// more than that to a client, which never sees this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GptError {
    /// Not `"EFI PART"` at LBA 1.
    NoSignature,
    /// The header's size, its `MyLBA`, its revision or one of its CRC32s is wrong.
    BadHeader,
    /// The entry array does not lie inside the disk, or claims more entries or larger entries
    /// than [`MAX_PARTITIONS`] and [`MAX_ENTRY_SIZE`] allow.
    BadArray,
    /// A partition runs outside the usable range, ends before it starts, or overlaps another.
    BadPartition,
    /// The host could not allocate the entry array.
    NoMemory,
}

/// What the header says about the entry array: where to read it from and how to read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrayLocation {
    pub lba: u64,
    pub entries: u32,
    pub entry_size: u32,
    pub crc32: u32,
    pub first_usable: u64,
    pub last_usable: u64,
    /// The bytes of the array, `entries * entry_size`; checked to fit [`MAX_ARRAY_BYTES`].
    pub bytes: u32,
    /// The sectors the array spans, rounded up.
    pub sectors: u32,
}

/// One partition of the table, in entry order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    /// The entry's index in the array, from 0. The **root badge of partition *i* is *i* + 1**
    /// (servers/blkd.md), so this is what `init`'s manifest names.
    pub index: u32,
    /// The first LBA, inclusive.
    pub first_lba: u64,
    /// Sectors in the partition: `last_lba - first_lba + 1`.
    pub sectors: u64,
}

/// Checks the primary header at LBA 1 and says where the entry array is.
///
/// `sector` is the whole sector read from [`HEADER_LBA`]; `disk_sectors` is the capacity the
/// device reported at bring-up. Nothing here reads past `sector`.
pub fn header(sector: &[u8], disk_sectors: u64) -> Result<ArrayLocation, GptError> {
    let sector = sector.get(..SECTOR_SIZE as usize).ok_or(GptError::BadHeader)?;
    if sector.get(..8) != Some(&SIGNATURE[..]) {
        return Err(GptError::NoSignature);
    }
    let revision = u32_at(sector, 8)?;
    let header_size = u32_at(sector, 12)?;
    let stored_crc = u32_at(sector, 16)?;
    let my_lba = u64_at(sector, 24)?;
    let first_usable = u64_at(sector, 40)?;
    let last_usable = u64_at(sector, 48)?;
    let array_lba = u64_at(sector, 72)?;
    let entries = u32_at(sector, 80)?;
    let entry_size = u32_at(sector, 84)?;
    let array_crc = u32_at(sector, 88)?;

    // Revision 1.0 is the only one the specification has ever defined.
    if revision != 0x0001_0000 {
        return Err(GptError::BadHeader);
    }
    if !(MIN_HEADER_SIZE..=SECTOR_SIZE).contains(&header_size) {
        return Err(GptError::BadHeader);
    }
    if my_lba != HEADER_LBA {
        return Err(GptError::BadHeader);
    }
    // The header's own CRC32 is taken over `header_size` bytes with the CRC field zeroed
    // (§5.3.2). `header_size` was bounded above, so the slice is inside the sector.
    let claimed = sector.get(..header_size as usize).ok_or(GptError::BadHeader)?;
    if crc32_with_zeroed(claimed, 16, 4) != stored_crc {
        return Err(GptError::BadHeader);
    }

    if entries == 0 || entries > MAX_PARTITIONS {
        return Err(GptError::BadArray);
    }
    if !(MIN_ENTRY_SIZE..=MAX_ENTRY_SIZE).contains(&entry_size) || !entry_size.is_multiple_of(MIN_ENTRY_SIZE)
    {
        return Err(GptError::BadArray);
    }
    let bytes = entries.checked_mul(entry_size).ok_or(GptError::BadArray)?;
    if bytes > MAX_ARRAY_BYTES {
        return Err(GptError::BadArray);
    }
    let sectors = bytes.div_ceil(SECTOR_SIZE);

    // The usable range must be a range, must leave room for the header and the array, and must
    // lie inside the disk.
    if first_usable > last_usable || last_usable >= disk_sectors {
        return Err(GptError::BadHeader);
    }
    // The array must be inside the disk and outside the usable range, or a partition could name
    // the sectors that describe it.
    let array_end = array_lba.checked_add(u64::from(sectors)).ok_or(GptError::BadArray)?;
    if array_lba <= HEADER_LBA || array_end > disk_sectors || array_end > first_usable {
        return Err(GptError::BadArray);
    }

    Ok(ArrayLocation {
        lba: array_lba,
        entries,
        entry_size,
        crc32: array_crc,
        first_usable,
        last_usable,
        bytes,
        sectors,
    })
}

/// Checks the entry array against the header and returns the partitions that are in use, in entry
/// order.
///
/// `array` is the bytes read from [`ArrayLocation::lba`]; only the first
/// [`ArrayLocation::bytes`] of it are looked at, whatever else was read with it.
pub fn partitions(at: &ArrayLocation, array: &[u8]) -> Result<Vec<Partition>, GptError> {
    let array = array.get(..at.bytes as usize).ok_or(GptError::BadArray)?;
    if crc32(array) != at.crc32 {
        return Err(GptError::BadArray);
    }
    let mut found: Vec<Partition> = Vec::new();
    found.try_reserve(at.entries as usize).map_err(|_| GptError::NoMemory)?;
    for index in 0..at.entries {
        let start = (index as usize) * (at.entry_size as usize);
        let entry = array.get(start..start + MIN_ENTRY_SIZE as usize).ok_or(GptError::BadArray)?;
        // An all-zero type GUID is an unused entry (§5.3.3). Its other fields mean nothing, so
        // they are not checked: an unused entry cannot become a range. The two GUIDs go no
        // further than this: a volume names its partition by its place in the array (the badge
        // it is given), not by a name off a medium `blkd` does not trust.
        if entry[..16] == [0; 16] {
            continue;
        }
        let first_lba = u64_at(entry, 32)?;
        let last_lba = u64_at(entry, 40)?;
        if first_lba > last_lba || first_lba < at.first_usable || last_lba > at.last_usable {
            return Err(GptError::BadPartition);
        }
        // `last >= first` and both are inside the usable range, so this cannot overflow.
        let sectors = last_lba - first_lba + 1;
        let partition = Partition { index, first_lba, sectors };
        // An overlap would let two volumes alias each other's bytes. At most
        // `MAX_PARTITIONS` entries, so the comparison is bounded work.
        if found.iter().any(|other| overlaps(other, &partition)) {
            return Err(GptError::BadPartition);
        }
        found.push(partition);
    }
    Ok(found)
}

/// Whether two partitions share a sector. Both are known to lie inside the usable range, so the
/// ends do not overflow.
fn overlaps(a: &Partition, b: &Partition) -> bool {
    let (a_end, b_end) = (a.first_lba + a.sectors, b.first_lba + b.sectors);
    a.first_lba < b_end && b.first_lba < a_end
}

fn u32_at(bytes: &[u8], off: usize) -> Result<u32, GptError> {
    let field = bytes.get(off..off + 4).ok_or(GptError::BadHeader)?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

fn u64_at(bytes: &[u8], off: usize) -> Result<u64, GptError> {
    let field = bytes.get(off..off + 8).ok_or(GptError::BadHeader)?;
    let mut value = [0; 8];
    value.copy_from_slice(field);
    Ok(u64::from_le_bytes(value))
}

/// CRC-32 (the one ISO 3309 and UEFI 2.10 §5.3.1 name): reflected, polynomial `0xedb88320`,
/// initial and final value all ones. Written here rather than taken from a crate, because it is
/// twelve lines and this crate parses an untrusted medium (TENETS.md 5).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let low = crc & 1;
            crc >>= 1;
            if low != 0 {
                crc ^= 0xedb8_8320;
            }
        }
    }
    !crc
}

/// [`crc32`] of `bytes` with `len` bytes at `off` treated as zero: the header's own CRC covers
/// the field that holds it, which must read as zero while it is computed (§5.3.2).
fn crc32_with_zeroed(bytes: &[u8], off: usize, len: usize) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for (i, byte) in bytes.iter().enumerate() {
        let value = if i >= off && i < off + len { 0 } else { *byte };
        crc ^= u32::from(value);
        for _ in 0..8 {
            let low = crc & 1;
            crc >>= 1;
            if low != 0 {
                crc ^= 0xedb8_8320;
            }
        }
    }
    !crc
}

#[cfg(test)]
#[path = "gpt_tests.rs"]
mod tests;
