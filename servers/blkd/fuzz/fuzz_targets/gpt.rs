//! Arbitrary bytes as a partition table. The claim: [`redoubt_blkd::gpt`] refuses them, with no
//! panic, no unbounded allocation and no unbounded work, whatever they say about their own sizes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_blkd::gpt;
use redoubt_blkd::virtio::SECTOR_SIZE;

fuzz_target!(|data: &[u8]| {
    let sector_len = SECTOR_SIZE as usize;
    if data.len() < sector_len + 8 {
        return;
    }
    // The first eight bytes choose the disk's size, so the header's LBAs are checked against
    // every sort of capacity, including ones that would overflow.
    let disk_sectors = u64::from_le_bytes(data[..8].try_into().expect("eight bytes"));
    let rest = &data[8..];
    let Ok(at) = gpt::header(&rest[..sector_len], disk_sectors) else { return };
    // The header checked out, so the array is read next: whatever bytes follow it, padded.
    let mut array = vec![0u8; at.sectors as usize * sector_len];
    let tail = &rest[sector_len..];
    let n = tail.len().min(array.len());
    array[..n].copy_from_slice(&tail[..n]);
    let _ = gpt::partitions(&at, &array);
});
