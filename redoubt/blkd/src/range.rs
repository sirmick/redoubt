//! A block range: the window on the disk that one badge names.
//!
//! Every sector number a client sends is **relative to its own range**, so there is no address a
//! client can write down that names a sector outside it. Turning one into a disk LBA is the only
//! place the two numbering schemes meet, and it is here, in eight lines that overflow-check
//! everything.

/// A run of sectors on the disk. Its end never overflows: [`Range::new`] is the only way to make
/// one and refuses a run that would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    first: u64,
    sectors: u64,
}

/// The run does not fit the disk, or is empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotOnDisk;

impl Range {
    /// The run `first..first + sectors`, if it is non-empty and lies inside a disk of
    /// `disk_sectors` sectors.
    pub fn new(first: u64, sectors: u64, disk_sectors: u64) -> Result<Range, NotOnDisk> {
        match first.checked_add(sectors) {
            Some(end) if sectors > 0 && end <= disk_sectors => Ok(Range { first, sectors }),
            _ => Err(NotOnDisk),
        }
    }

    /// The whole disk, for the table `blkd` reads the partition table through.
    pub fn whole(disk_sectors: u64) -> Result<Range, NotOnDisk> { Range::new(0, disk_sectors, disk_sectors) }

    pub fn first(&self) -> u64 { self.first }

    pub fn sectors(&self) -> u64 { self.sectors }

    /// The disk LBA of `sector` in this range, if the whole run of `count` sectors from there is
    /// inside it. `None` is the client's `out_of_range`; nothing is sent to the device.
    pub fn absolute(&self, sector: u64, count: u64) -> Option<u64> {
        let end = sector.checked_add(count)?;
        if count == 0 || end > self.sectors {
            return None;
        }
        // `sector < self.sectors` and `self.first + self.sectors` does not overflow, so neither
        // does this.
        Some(self.first + sector)
    }

    /// The sub-range `sector..sector + count` of this one, for `grant`. A window that leaves this
    /// range, or an empty one, is `None`: nothing granted is ever wider than the badge it came
    /// through (WIRE.md, granting and releasing).
    pub fn narrow(&self, sector: u64, count: u64) -> Option<Range> {
        let first = self.absolute(sector, count)?;
        Some(Range { first, sectors: count })
    }
}
