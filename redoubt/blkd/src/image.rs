//! Disk images for tests and fuzz targets: a GPT builder, host-only, never on the machine.
//!
//! It lives here rather than in a test file because three places need it — the unit tests, the
//! integration tests and the fuzz targets — and because a builder that writes the layout
//! [`crate::gpt`] reads is the honest way to check that the reader accepts a real table and not
//! merely one the same code wrote. Its output is checked against the UEFI layout by hand in
//! `gpt_tests.rs` (the field offsets are written out there), and a table it builds is one
//! `gdisk` would also accept.

use alloc::vec;
use alloc::vec::Vec;

use crate::gpt;
use crate::virtio::SECTOR_SIZE;

/// Entries in the array a built image carries: the UEFI default.
pub const ENTRIES: u32 = 128;
/// Bytes per entry: the UEFI default.
pub const ENTRY_SIZE: u32 = 128;
/// The LBA the entry array starts at.
pub const ARRAY_LBA: u64 = 2;
/// Sectors the array spans: 128 x 128 bytes.
pub const ARRAY_SECTORS: u64 = (ENTRIES * ENTRY_SIZE / SECTOR_SIZE) as u64;
/// The first LBA a partition may use.
pub const FIRST_USABLE: u64 = ARRAY_LBA + ARRAY_SECTORS;
/// Sectors reserved at the end for the backup header and array, which `blkd` never reads but a
/// real table always has.
pub const TAIL: u64 = ARRAY_SECTORS + 1;

/// A partition to put in a built image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub first_lba: u64,
    pub last_lba: u64,
}

/// A disk image with a GPT at LBA 1 and an entry array at LBA 2.
pub struct Image {
    pub bytes: Vec<u8>,
    pub sectors: u64,
}

impl Image {
    /// `sectors` sectors, with `parts` as the partitions in entry order.
    pub fn new(sectors: u64, parts: &[Entry]) -> Image {
        let mut bytes = vec![0u8; sectors as usize * SECTOR_SIZE as usize];
        let last_usable = sectors.saturating_sub(TAIL).saturating_sub(1);
        let array = entry_array(parts);
        let header = header(sectors, last_usable, gpt::crc32(&array));
        let at = SECTOR_SIZE as usize;
        bytes[at..at + header.len()].copy_from_slice(&header);
        let at = ARRAY_LBA as usize * SECTOR_SIZE as usize;
        bytes[at..at + array.len()].copy_from_slice(&array);
        Image { bytes, sectors }
    }

    /// One sector of the image, to change before it is handed to a device.
    pub fn sector_mut(&mut self, lba: u64) -> &mut [u8] {
        let at = lba as usize * SECTOR_SIZE as usize;
        &mut self.bytes[at..at + SECTOR_SIZE as usize]
    }

    /// Recomputes the header's CRC32 over whatever the header now says, so a test can change one
    /// field and still have a table that only fails on that field.
    pub fn refresh_header_crc(&mut self) {
        let header = self.sector_mut(gpt::HEADER_LBA);
        header[16..20].fill(0);
        let crc = gpt::crc32(&header[..gpt::MIN_HEADER_SIZE as usize]);
        header[16..20].copy_from_slice(&crc.to_le_bytes());
    }

    /// Recomputes the entry array's CRC32 in the header, likewise.
    pub fn refresh_array_crc(&mut self) {
        let at = ARRAY_LBA as usize * SECTOR_SIZE as usize;
        let len = (ENTRIES * ENTRY_SIZE) as usize;
        let crc = gpt::crc32(&self.bytes[at..at + len]);
        self.sector_mut(gpt::HEADER_LBA)[88..92].copy_from_slice(&crc.to_le_bytes());
        self.refresh_header_crc();
    }
}

/// The 92-byte header (UEFI 2.10, §5.3.2, table 5.5), with its own CRC32 filled in.
fn header(sectors: u64, last_usable: u64, array_crc: u32) -> Vec<u8> {
    let mut h = vec![0u8; gpt::MIN_HEADER_SIZE as usize];
    h[0..8].copy_from_slice(gpt::SIGNATURE);
    h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // revision 1.0
    h[12..16].copy_from_slice(&gpt::MIN_HEADER_SIZE.to_le_bytes());
    // 16..20 is the header CRC32, filled in below.
    h[24..32].copy_from_slice(&gpt::HEADER_LBA.to_le_bytes()); // MyLBA
    h[32..40].copy_from_slice(&sectors.saturating_sub(1).to_le_bytes()); // AlternateLBA
    h[40..48].copy_from_slice(&FIRST_USABLE.to_le_bytes());
    h[48..56].copy_from_slice(&last_usable.to_le_bytes());
    h[56..72].copy_from_slice(&[0x11; 16]); // DiskGUID
    h[72..80].copy_from_slice(&ARRAY_LBA.to_le_bytes());
    h[80..84].copy_from_slice(&ENTRIES.to_le_bytes());
    h[84..88].copy_from_slice(&ENTRY_SIZE.to_le_bytes());
    h[88..92].copy_from_slice(&array_crc.to_le_bytes());
    let crc = gpt::crc32(&h);
    h[16..20].copy_from_slice(&crc.to_le_bytes());
    h
}

/// The entry array: `parts` in order, the rest unused (an all-zero type GUID).
fn entry_array(parts: &[Entry]) -> Vec<u8> {
    let mut array = vec![0u8; (ENTRIES * ENTRY_SIZE) as usize];
    for (i, part) in parts.iter().take(ENTRIES as usize).enumerate() {
        let at = i * ENTRY_SIZE as usize;
        let entry = &mut array[at..at + ENTRY_SIZE as usize];
        entry[0..16].copy_from_slice(&[0x0f; 16]); // a type GUID that is not all zero
        entry[16..32].copy_from_slice(&[i as u8 + 1; 16]); // a unique GUID
        entry[32..40].copy_from_slice(&part.first_lba.to_le_bytes());
        entry[40..48].copy_from_slice(&part.last_lba.to_le_bytes());
    }
    array
}
