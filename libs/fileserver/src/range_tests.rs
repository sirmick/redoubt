use alloc::vec::Vec;

use super::{Fault, Geometry, Range, SECTOR};

/// Sectors in memory, counting the reads asked of them.
struct Sectors {
    bytes: Vec<u8>,
    reads: usize,
}

impl Range for Sectors {
    fn info(&mut self) -> Result<Geometry, Fault> {
        Ok(Geometry { sectors: (self.bytes.len() / SECTOR as usize) as u64, read_only: true })
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.reads += 1;
        if !out.len().is_multiple_of(SECTOR as usize) {
            return Err(Fault);
        }
        let at = usize::try_from(sector).map_err(|_| Fault)? * SECTOR as usize;
        let from = self.bytes.get(at..at.checked_add(out.len()).ok_or(Fault)?).ok_or(Fault)?;
        out.copy_from_slice(from);
        Ok(())
    }
}

/// A byte read anywhere in the range reads whole sectors and gives back exactly those bytes:
/// aligned runs in one read, a head or tail inside a sector through one sector each, and a read
/// past the end a fault. A range that says nothing of writing refuses it.
#[test]
fn a_byte_read_over_a_sector_range_reads_whole_sectors() {
    let s = SECTOR as usize;
    let bytes: Vec<u8> = (0..8 * s).map(|i| (i % 251) as u8).collect();
    let mut range = Sectors { bytes: bytes.clone(), reads: 0 };
    for (at, len, reads) in [
        (0, 3 * s, 1),
        (2 * s, s, 1),
        (100, 50, 1),
        (s - 10, 20, 2),
        (s - 10, 2 * s + 20, 3),
        (3 * s + 1, 4 * s, 3),
        (7 * s + 500, 12, 1),
        (0, 0, 0),
    ] {
        let mut out = alloc::vec![0u8; len];
        range.reads = 0;
        range.read_at(at as u64, &mut out).unwrap();
        assert_eq!(out, bytes[at..at + len], "{at}+{len}");
        assert_eq!(range.reads, reads, "{at}+{len}");
    }
    let mut out = [0u8; 16];
    assert_eq!(range.read_at((8 * s - 8) as u64, &mut out), Err(Fault));
    assert_eq!(range.read_at(u64::MAX, &mut out), Err(Fault));
    assert_eq!(range.write(0, &[0; 512]), Err(Fault));
    assert_eq!(range.flush(), Err(Fault));
    assert_eq!(range.info(), Ok(Geometry { sectors: 8, read_only: true }));
}
