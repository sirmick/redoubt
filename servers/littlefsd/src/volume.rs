//! The volume under the files: `blkd`'s range as littlefs blocks, and the mounting rule
//! (servers/littlefsd.md, "Volumes, connections and labels").

use alloc::vec::Vec;

use littlefs::{BlockDevice, Config, Error, Filesystem};
pub use redoubt_fileserver::range::{Fault, Geometry, Range, SECTOR};

/// A littlefs block: eight sectors, fixed, with no argument to change it.
pub const BLOCK: u32 = 4096;
/// Sectors per block.
pub const SECTORS_PER_BLOCK: u64 = (BLOCK / SECTOR) as u64;
/// littlefs's smallest volume: the superblock pair and one more pair.
pub const MIN_BLOCKS: u32 = 4;

/// A range as littlefs's [`BlockDevice`]: programs are whole sectors (`prog_size` is one), and
/// a disk needs no erase, since a later write replaces a sector whole.
pub struct Blocks<R> {
    range: R,
    count: u32,
    /// `littlefsd` asks for no write on a read-only range; one that reached here anyway is refused
    /// before `blkd` sees it.
    read_only: bool,
    /// One block of sectors, for reads that start or end inside one.
    scratch: Vec<u8>,
}

impl<R: Range> Blocks<R> {
    fn first_sector(&self, block: u32, off: u32, len: usize) -> Result<u64, Error> {
        let end = (off as usize).checked_add(len).ok_or(Error::Io)?;
        if block >= self.count || end > BLOCK as usize {
            return Err(Error::Io);
        }
        Ok(u64::from(block) * SECTORS_PER_BLOCK + u64::from(off / SECTOR))
    }
}

impl<R: Range> BlockDevice for Blocks<R> {
    fn read(&mut self, block: u32, off: u32, buf: &mut [u8]) -> Result<(), Error> {
        let first = self.first_sector(block, off, buf.len())?;
        #[cfg(feature = "boot-stats")]
        crate::stats::block_read(block, off, buf.len(), BLOCK);
        let skip = (off % SECTOR) as usize;
        let span = (skip + buf.len()).div_ceil(SECTOR as usize) * SECTOR as usize;
        self.range.read(first, &mut self.scratch[..span]).map_err(|_| Error::Io)?;
        buf.copy_from_slice(&self.scratch[skip..skip + buf.len()]);
        Ok(())
    }

    fn prog(&mut self, block: u32, off: u32, data: &[u8]) -> Result<(), Error> {
        if self.read_only {
            return Err(Error::Invalid);
        }
        if !off.is_multiple_of(SECTOR) || !data.len().is_multiple_of(SECTOR as usize) {
            return Err(Error::Io);
        }
        let first = self.first_sector(block, off, data.len())?;
        self.range.write(first, data).map_err(|_| Error::Io)
    }

    fn erase(&mut self, block: u32) -> Result<(), Error> {
        if self.read_only {
            Err(Error::Invalid)
        } else if block >= self.count {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }

    fn sync(&mut self) -> Result<(), Error> { self.range.flush().map_err(|_| Error::Io) }
}

/// Why there is no volume to serve: `littlefsd` exits before serving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoVolume {
    /// `info` failed: the range cannot even be sized.
    NoInfo,
    /// Fewer than [`MIN_BLOCKS`] blocks, or more than littlefs can count.
    TooSmall,
    /// No memory for a block of scratch.
    NoMemory,
}

/// What mounting found.
pub enum Mounted<R: Range> {
    /// A filesystem to serve, whether its range refuses writes, and its block count.
    Files { fs: Filesystem<Blocks<R>>, read_only: bool, blocks: u32 },
    /// The range holds something that does not mount (or could not be read): served as corrupt.
    Corrupt(Error),
}

/// The geometry of a volume of `count` blocks.
pub fn config(count: u32) -> Config { Config { block_size: BLOCK, block_count: count, prog_size: SECTOR } }

/// Mounts `range`. A range whose first two blocks (the superblock pair) are all zero has never
/// been written, and is formatted, unless it is read-only; any other range that does not mount
/// is [`Mounted::Corrupt`]. Nothing that holds anything is ever formatted.
pub fn mount<R: Range>(mut range: R) -> Result<Mounted<R>, NoVolume> {
    let Geometry { sectors, read_only } = range.info().map_err(|_| NoVolume::NoInfo)?;
    let count = u32::try_from(sectors / SECTORS_PER_BLOCK).map_err(|_| NoVolume::TooSmall)?;
    if count < MIN_BLOCKS {
        return Err(NoVolume::TooSmall);
    }
    let mut scratch = Vec::new();
    scratch.try_reserve_exact(BLOCK as usize).map_err(|_| NoVolume::NoMemory)?;
    scratch.resize(BLOCK as usize, 0);
    let mut blocks = Blocks { range, count, read_only, scratch };
    match blank(&mut blocks) {
        // A blank range that cannot be written holds no filesystem and never will.
        Ok(true) if read_only => return Ok(Mounted::Corrupt(Error::Invalid)),
        Ok(true) => {
            if let Err(e) = Filesystem::format(&mut blocks, config(count)) {
                return Ok(Mounted::Corrupt(e));
            }
        }
        Ok(false) => {}
        Err(e) => return Ok(Mounted::Corrupt(e)),
    }
    Ok(match Filesystem::mount(blocks, config(count)) {
        Ok(fs) => Mounted::Files { fs, read_only, blocks: count },
        Err(e) => Mounted::Corrupt(e),
    })
}

/// Whether blocks 0 and 1 read as all zero.
fn blank<R: Range>(blocks: &mut Blocks<R>) -> Result<bool, Error> {
    for block in 0..2 {
        blocks.range.read(block * SECTORS_PER_BLOCK, &mut blocks.scratch).map_err(|_| Error::Io)?;
        if blocks.scratch.iter().any(|b| *b != 0) {
            return Ok(false);
        }
    }
    Ok(true)
}
