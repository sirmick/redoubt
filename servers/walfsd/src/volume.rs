//! The volume under the files: `blkd`'s range as walfs blocks, and the mounting rule
//! (servers/walfsd.md, "Serving").

use alloc::rc::Rc;
use core::cell::Cell;
use core::fmt;

pub use redoubt_fileserver::range::{Fault, Geometry, Range, SECTOR, Why};
use walfs::{Block, BlockDevice, Error, Filesystem, Geometry as Shape};

/// A walfs block.
pub const BLOCK: u32 = walfs::BLOCK as u32;
/// Sectors per block.
pub const SECTORS_PER_BLOCK: u64 = (BLOCK / SECTOR) as u64;
/// walfs's smallest volume at the inode density `walfsd` formats with: the superblock, the log's
/// 33 blocks, one block of inodes and two of their attributes, a hash block, a bitmap block and
/// one data block.
pub const MIN_BLOCKS: u32 = 40;

/// A range as walfs's [`BlockDevice`]: a block is eight sectors, and walfs's `sync`, which the
/// format puts between the steps of every transaction, is `blkd`'s `flush`.
pub struct Blocks<R> {
    range: R,
    count: u32,
    /// `walfsd` asks for no write on a read-only range; one that reached here anyway (a mount's
    /// recovery) is refused before `blkd` sees it.
    read_only: bool,
    /// Why the range's last request faulted, for the server to say when the fault poisons the
    /// volume: shared with it, since walfs holds the device.
    why: Rc<Cell<Option<Why>>>,
}

impl<R: Range> Blocks<R> {
    /// The range's fault as walfs's I/O error, its reason kept for the server.
    fn io(&self, _: Fault) -> Error {
        self.why.set(self.range.fault());
        Error::Io
    }
}

impl<R: Range> BlockDevice for Blocks<R> {
    fn block_count(&self) -> u32 { self.count }

    fn read(&mut self, block: u32, buf: &mut Block) -> Result<(), Error> {
        if block >= self.count {
            return Err(Error::Io);
        }
        self.range.read(u64::from(block) * SECTORS_PER_BLOCK, buf).map_err(|f| self.io(f))
    }

    fn write(&mut self, block: u32, data: &Block) -> Result<(), Error> {
        if self.read_only || block >= self.count {
            return Err(Error::Io);
        }
        self.range.write(u64::from(block) * SECTORS_PER_BLOCK, data).map_err(|f| self.io(f))?;
        // Test-only: the power-loss case's cut, after the write completed (src/cut.rs).
        #[cfg(feature = "cut-after-write")]
        crate::cut::wrote();
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> { self.range.flush().map_err(|f| self.io(f)) }
}

/// Why there is no volume to serve: `walfsd` exits before serving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoVolume {
    /// `info` failed: the range cannot even be sized.
    NoInfo,
    /// Fewer than [`MIN_BLOCKS`] blocks, or more than walfs can address.
    TooSmall,
}

/// What mounting found.
pub enum Mounted<R: Range> {
    /// A filesystem to serve, whether its range refuses writes, and where the range's last
    /// fault's reason is kept ([`Blocks`]).
    Files { fs: Filesystem<Blocks<R>>, read_only: bool, why: Rc<Cell<Option<Why>>> },
    /// The range holds something that does not mount (or could not be read): served as corrupt.
    Corrupt(Unmounted),
}

/// Why a range does not mount: the step that failed, walfs's error, and the range's reason when
/// the error was its I/O. `walfsd` says it beside its corrupt line (servers/walfsd.md, "Serving").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unmounted {
    pub step: Step,
    pub error: Error,
    pub why: Option<Why>,
}

/// The step of a mount that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Reading blocks 0 and 1, to learn whether the range is blank.
    Blank,
    /// The range is blank and read-only, so it holds no filesystem and never will.
    BlankReadOnly,
    /// Formatting a blank range.
    Format,
    /// walfs's mount: the superblock, the log's replay and the checks on what it holds.
    Mount,
    /// Counting what the volume holds, for the volume root's quota.
    Count,
}

impl fmt::Display for Unmounted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let step = match self.step {
            Step::Blank => "reading blocks 0 and 1",
            Step::BlankReadOnly => "blank and read-only",
            Step::Format => "formatting the blank range",
            Step::Mount => "mounting",
            Step::Count => "counting what it holds",
        };
        write!(f, "{step}: {:?}", self.error)?;
        match self.why {
            Some(why) => write!(f, ": {why}"),
            None => Ok(()),
        }
    }
}

/// Mounts `range`. A range whose first two blocks (the superblock and the log's header) are all
/// zero has never been written, and is formatted with an inode for every 16 blocks, the packer's
/// density, unless it is read-only; any other range that does not mount is [`Mounted::Corrupt`].
/// Nothing that holds anything is ever formatted.
pub fn mount<R: Range>(mut range: R) -> Result<Mounted<R>, NoVolume> {
    let Geometry { sectors, read_only } = range.info().map_err(|_| NoVolume::NoInfo)?;
    let count = u32::try_from(sectors / SECTORS_PER_BLOCK).map_err(|_| NoVolume::TooSmall)?;
    if count < MIN_BLOCKS {
        return Err(NoVolume::TooSmall);
    }
    let why = Rc::new(Cell::new(None));
    let mut blocks = Blocks { range, count, read_only, why: why.clone() };
    // The range's reason is kept only for its I/O error: any other error is walfs's own finding.
    let failed = |step, error| {
        let why = if error == Error::Io { why.get() } else { None };
        Ok(Mounted::Corrupt(Unmounted { step, error, why }))
    };
    match blank(&mut blocks) {
        // A blank range that cannot be written holds no filesystem and never will.
        Ok(true) if read_only => return failed(Step::BlankReadOnly, Error::Invalid),
        Ok(true) => {
            if let Err(e) = Filesystem::format(&mut blocks, Shape::for_blocks(count)) {
                return failed(Step::Format, e);
            }
        }
        Ok(false) => {}
        Err(e) => return failed(Step::Blank, e),
    }
    match Filesystem::mount(blocks) {
        Ok(fs) => Ok(Mounted::Files { fs, read_only, why }),
        Err(e) => failed(Step::Mount, e),
    }
}

/// Whether blocks 0 and 1 read as all zero.
fn blank<R: Range>(blocks: &mut Blocks<R>) -> Result<bool, Error> {
    let mut scratch = [0u8; walfs::BLOCK];
    for block in 0..2 {
        blocks.read(block, &mut scratch)?;
        if scratch.iter().any(|b| *b != 0) {
            return Ok(false);
        }
    }
    Ok(true)
}
