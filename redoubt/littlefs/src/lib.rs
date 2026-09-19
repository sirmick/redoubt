//! littlefs in pure Rust: `no_std` + `alloc`, no dependencies, no `unsafe`.
//!
//! This implements the littlefs on-disk format, **version 2.1** (`SPEC.md` of
//! littlefs-project/littlefs, checked against the C reference v2.11.3), for `fsd`: one
//! filesystem server per volume (planning/redoubt/NAMESPACES.md). Images this crate writes
//! mount in the C reference and the other way round; the C code runs only on the host, as a
//! test oracle (`diff/`).
//!
//! # Shape
//! - [`BlockDevice`]: the four operations littlefs needs from storage.
//! - [`Filesystem`]: format, mount, and every operation `fsd` needs: files (open, read,
//!   write, seek, truncate, sync, close), directories (mkdir, remove, rename, read_dir),
//!   stat, and user attributes on files and directories.
//! - Paths are `/`-separated names relative to the root; `.` and `..` are not accepted.
//!   Names read back from the medium are bytes: nothing forces them to be UTF-8.
//!
//! # The medium is hostile
//! Every length, offset, block pointer and tag read from the device is checked before it is
//! used. A malformed image yields [`Error::Corrupt`], never a panic; lists of metadata pairs
//! are walked with cycle detection and file skip-lists with bounded loops. Nothing recurses.
//!
//! # Power loss
//! Every change reaches the disk as one metadata commit (or, for renames and directory
//! removal, a sequence the next mount completes or undoes), so an interrupted operation
//! leaves the state from before or after it. This is littlefs's design; the crash-injection
//! tests in `tests/` check it at every block write.
//!
//! # Left out on purpose
//! Wear levelling and bad-block relocation (`block_cycles`): `fsd` sits on a virtio disk,
//! whose device handles both; a failed program or erase is reported, not worked around. As a
//! consequence this crate never relocates metadata or grows the superblock chain, but it
//! reads images where the C reference did. No v1 migration, no `fs_grow`, no directory
//! handles (see [`Filesystem::read_dir`]).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod crc;
mod ctz;
mod file;
mod fs;
mod mdir;
mod ops;
mod tag;

pub use file::{FileHandle, OpenOptions, SeekFrom};
pub use fs::Filesystem;

/// The on-disk version this crate writes: major 2, minor 1. It reads 2.0 too, and upgrades a
/// 2.0 superblock to 2.1 on the first write, like the reference.
pub const DISK_VERSION: u32 = 0x0002_0001;

/// Storage as littlefs sees it: `block_count` blocks of `block_size` bytes.
///
/// Reads may be of any range inside a block. Programs start and end on multiples of the
/// configured `prog_size` and only target erased bytes. Implementations report failures as
/// [`Error::Io`].
pub trait BlockDevice {
    fn read(&mut self, block: u32, off: u32, buf: &mut [u8]) -> Result<(), Error>;
    fn prog(&mut self, block: u32, off: u32, data: &[u8]) -> Result<(), Error>;
    fn erase(&mut self, block: u32) -> Result<(), Error>;
    /// Makes everything programmed so far durable.
    fn sync(&mut self) -> Result<(), Error>;
}

/// The geometry of a volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Bytes per block (the erase unit). At least 128, a multiple of `prog_size`.
    pub block_size: u32,
    /// Blocks in the volume. Mounting checks it against the superblock.
    pub block_count: u32,
    /// The program unit. Not stored on disk; may differ between mounts.
    pub prog_size: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The block device failed.
    Io,
    /// The image is malformed.
    Corrupt,
    /// No such file or directory.
    NoEntry,
    /// The name is already taken.
    Exists,
    /// A path component that must be a directory is a file.
    NotDir,
    /// A file operation on a directory.
    IsDir,
    /// Removing (or renaming over) a directory that has entries.
    NotEmpty,
    /// A bad argument: `.`/`..` or an empty name, a bad handle, a move into itself, a
    /// geometry littlefs cannot use, an unsupported on-disk version.
    Invalid,
    /// No free blocks, or the metadata does not fit.
    NoSpace,
    NameTooLong,
    FileTooBig,
    /// No attribute of that type.
    NoAttr,
    /// An earlier failure left memory and disk possibly out of step; nothing more is done
    /// until the volume is mounted again (which repairs it).
    Poisoned,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Debug::fmt(self, f) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    File,
    Dir,
}

/// What `stat` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub kind: FileType,
    /// Bytes in a file as last synced; 0 for a directory.
    pub size: u32,
}

/// One entry, as [`Filesystem::read_dir`] passes it.
#[derive(Debug)]
pub struct DirEntry<'a> {
    pub name: &'a [u8],
    pub kind: FileType,
    pub size: u32,
}
