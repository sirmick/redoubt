//! The partition table against tables a hostile disk would write.

use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::image::{ARRAY_LBA, ENTRIES, ENTRY_SIZE, Entry, FIRST_USABLE, Image};

const SECTORS: u64 = 8192;

fn good() -> Image {
    Image::new(SECTORS, &[Entry { first_lba: 64, last_lba: 1063 }, Entry { first_lba: 2048, last_lba: 4095 }])
}

/// Parses an image's table the way `read_partitions` does, without a device.
fn parse(image: &Image) -> Result<Vec<Partition>, GptError> {
    let sector = |lba: u64| {
        let at = lba as usize * SECTOR_SIZE as usize;
        &image.bytes[at..at + SECTOR_SIZE as usize]
    };
    let at = header(sector(HEADER_LBA), image.sectors)?;
    let start = at.lba as usize * SECTOR_SIZE as usize;
    let len = at.sectors as usize * SECTOR_SIZE as usize;
    partitions(&at, &image.bytes[start..start + len])
}

/// The bytes the builder writes are where UEFI 2.10 table 5.5 says they are. Written out by hand,
/// so the reader and the builder are not simply agreeing with each other.
#[test]
fn header_fields_sit_where_the_specification_says() {
    let image = good();
    let at = image.bytes[SECTOR_SIZE as usize..2 * SECTOR_SIZE as usize].to_vec();
    assert_eq!(&at[0..8], b"EFI PART");
    assert_eq!(u32::from_le_bytes(at[8..12].try_into().unwrap()), 0x0001_0000);
    assert_eq!(u32::from_le_bytes(at[12..16].try_into().unwrap()), 92);
    assert_eq!(u64::from_le_bytes(at[24..32].try_into().unwrap()), 1);
    assert_eq!(u64::from_le_bytes(at[72..80].try_into().unwrap()), ARRAY_LBA);
    assert_eq!(u32::from_le_bytes(at[80..84].try_into().unwrap()), ENTRIES);
    assert_eq!(u32::from_le_bytes(at[84..88].try_into().unwrap()), ENTRY_SIZE);
}

/// CRC-32 against the one value everybody publishes: `"123456789"` is `0xcbf43926`.
#[test]
fn crc32_matches_the_published_check_value() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn a_good_table_reads_back() {
    let found = parse(&good()).expect("a table the builder wrote");
    assert_eq!(found.len(), 2);
    assert_eq!((found[0].index, found[0].first_lba, found[0].sectors), (0, 64, 1000));
    assert_eq!((found[1].index, found[1].first_lba, found[1].sectors), (1, 2048, 2048));
}

/// An entry whose type GUID is all zero is not a partition, and the ones after it keep their own
/// indices: a badge names an entry, not a position in the answer.
#[test]
fn unused_entries_are_skipped_and_indices_are_the_entry_s() {
    let mut image = good();
    let at = ARRAY_LBA as usize * SECTOR_SIZE as usize;
    image.bytes[at..at + 16].fill(0);
    image.refresh_array_crc();
    let found = parse(&image).expect("one partition left");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].index, 1);
}

#[test]
fn a_disk_with_no_signature_is_refused() {
    let mut image = good();
    image.sector_mut(HEADER_LBA)[0] = b'X';
    assert_eq!(parse(&image), Err(GptError::NoSignature));
}

#[test]
fn a_header_crc_that_does_not_match_is_refused() {
    let mut image = good();
    image.sector_mut(HEADER_LBA)[40] ^= 1;
    assert_eq!(parse(&image), Err(GptError::BadHeader));
}

#[test]
fn an_entry_array_crc_that_does_not_match_is_refused() {
    let mut image = good();
    let at = ARRAY_LBA as usize * SECTOR_SIZE as usize;
    image.bytes[at + 33] ^= 0xff;
    assert_eq!(parse(&image), Err(GptError::BadArray));
}

/// Every one of these is a number a hostile disk would choose, and each must be refused rather
/// than turned into an index, a length or an allocation.
#[test]
fn hostile_header_numbers_are_refused() {
    let cases: &[(&str, usize, &[u8], GptError)] = &[
        ("header size 0", 12, &0u32.to_le_bytes(), GptError::BadHeader),
        ("header size past the sector", 12, &u32::MAX.to_le_bytes(), GptError::BadHeader),
        ("revision 0", 8, &0u32.to_le_bytes(), GptError::BadHeader),
        ("MyLBA is not 1", 24, &7u64.to_le_bytes(), GptError::BadHeader),
        ("no entries", 80, &0u32.to_le_bytes(), GptError::BadArray),
        ("more entries than we read", 80, &u32::MAX.to_le_bytes(), GptError::BadArray),
        ("entry size 0", 84, &0u32.to_le_bytes(), GptError::BadArray),
        ("entry size that overflows the product", 84, &u32::MAX.to_le_bytes(), GptError::BadArray),
        ("entry size not a multiple of 128", 84, &129u32.to_le_bytes(), GptError::BadArray),
        ("the array at the last LBA", 72, &u64::MAX.to_le_bytes(), GptError::BadArray),
        ("the array where the header is", 72, &1u64.to_le_bytes(), GptError::BadArray),
        ("the array inside the usable range", 72, &4096u64.to_le_bytes(), GptError::BadArray),
        ("last usable past the disk", 48, &u64::MAX.to_le_bytes(), GptError::BadHeader),
        ("first usable past last usable", 40, &u64::MAX.to_le_bytes(), GptError::BadHeader),
    ];
    for (why, off, bytes, expected) in cases {
        let mut image = good();
        image.sector_mut(HEADER_LBA)[*off..*off + bytes.len()].copy_from_slice(bytes);
        image.refresh_header_crc();
        assert_eq!(parse(&image), Err(*expected), "{why}");
    }
}

#[test]
fn hostile_partition_entries_are_refused() {
    let cases: &[(&str, &[Entry])] = &[
        ("ends before it starts", &[Entry { first_lba: 4096, last_lba: 64 }]),
        ("starts below the usable range", &[Entry { first_lba: 1, last_lba: 4096 }]),
        ("ends past the usable range", &[Entry { first_lba: FIRST_USABLE, last_lba: u64::MAX }]),
        (
            "overlaps the one before it",
            &[Entry { first_lba: 64, last_lba: 2048 }, Entry { first_lba: 2048, last_lba: 4095 }],
        ),
        (
            "contains the one before it",
            &[Entry { first_lba: 1024, last_lba: 2048 }, Entry { first_lba: 64, last_lba: 4095 }],
        ),
        (
            "is the same as the one before it",
            &[Entry { first_lba: 64, last_lba: 1063 }, Entry { first_lba: 64, last_lba: 1063 }],
        ),
    ];
    for (why, parts) in cases {
        let image = Image::new(SECTORS, parts);
        assert_eq!(parse(&image), Err(GptError::BadPartition), "{why}");
    }
}

/// Two partitions that touch but do not share a sector are fine: the refusal is of overlap, not
/// of adjacency.
#[test]
fn adjacent_partitions_are_allowed() {
    let image = Image::new(
        SECTORS,
        &[Entry { first_lba: 64, last_lba: 1023 }, Entry { first_lba: 1024, last_lba: 2047 }],
    );
    assert_eq!(parse(&image).map(|p| p.len()), Ok(2));
}

/// Arbitrary bytes at LBA 1 are refused, and never panic. The body is the fuzz target's, run
/// here over a deterministic sweep so it is in `cargo test` too.
#[test]
fn arbitrary_header_bytes_never_panic() {
    let mut state = 0x243f_6a88_85a3_08d3u64;
    for _ in 0..20_000 {
        let mut sector = vec![0u8; SECTOR_SIZE as usize];
        for byte in sector.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *byte = (state >> 33) as u8;
        }
        // Half the cases start with the signature, so the parse gets past the first check.
        if state & 1 == 0 {
            sector[..8].copy_from_slice(SIGNATURE);
        }
        let disk_sectors = state >> 40;
        if let Ok(at) = header(&sector, disk_sectors) {
            let array = vec![0u8; at.sectors as usize * SECTOR_SIZE as usize];
            let _ = partitions(&at, &array);
        }
    }
}

/// A header that checks out, with an array of arbitrary bytes.
#[test]
fn arbitrary_entry_arrays_never_panic() {
    let mut state = 0x13198a2e_03707344u64;
    let image = good();
    let at = header(&image.bytes[SECTOR_SIZE as usize..], image.sectors).expect("a good header");
    for _ in 0..2_000 {
        let mut array = vec![0u8; at.bytes as usize];
        for byte in array.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *byte = (state >> 33) as u8;
        }
        let mut at = at;
        at.crc32 = crc32(&array);
        let _ = partitions(&at, &array);
    }
}
