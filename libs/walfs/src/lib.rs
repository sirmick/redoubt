//! walfs in pure Rust: `no_std` + `alloc`, SHA-256 its one dependency, no `unsafe`.
//!
//! walfs is Redoubt's own logged file system for the SSD's writable volumes, specified by
//! docs/servers/walfsd.md ("The format"): this crate implements that page, and a field the page
//! does not have is a field the crate does not have. Its shape is xv6's (`kernel/fs.h` and
//! `kernel/log.c` of mit-pdos/xv6-riscv, its `riscv` branch as of 2026-10-06): a superblock,
//! a redo log of whole blocks with one commit block, fixed inodes with direct and indirect blocks,
//! directories of fixed entries and a block bitmap. Nothing of xv6's is copied; the layout is
//! Redoubt's, with 4 KiB blocks, a SHA-256 per block, 64-bit sizes and user attributes.
//!
//! # Shape
//! - [`BlockDevice`]: whole 4 KiB blocks read and written, and `sync`.
//! - [`Filesystem`]: format, mount (which recovers the log and finishes the orphan list), files (open, read,
//!   write, seek, truncate, sync, close), directories (mkdir, remove, rename, read_dir), stat, user
//!   attributes, and a volume check that names what it finds.
//! - Paths are `/`-separated names from the root; `.`, `..`, empty names and names with a NUL are refused.
//!   Names read back from the medium are opaque bytes.
//!
//! # Transactions
//! Every operation is one transaction of at most [`LOG_BLOCKS`] blocks, data and metadata
//! together: written to the log, committed by one header block, copied home, the header cleared,
//! with a `sync` between each step. A write larger than one transaction's room is several, each
//! whole. Removing or truncating a file is one transaction that the reader sees; freeing its
//! blocks may take more, finished from the orphan list, at mount if a power cut stopped it.
//!
//! # The medium is hostile
//! Every block read is checked before anything in it is parsed: a hash block against its own
//! SHA-256 in its last 32 bytes, the log's blocks against the header, which checks itself, and
//! every other block against its slot in the hash region. Every field, address and length is then checked
//! against the superblock's geometry. A malformed volume yields [`Error::Corrupt`], never a panic, and every
//! walk is bounded by the volume's counts.
//!
//! # Memory
//! The bitmap whole (`block_count / 8` bytes), one block per block of the open transaction (at
//! most [`LOG_BLOCKS`]), the last hash block read, scratch blocks for one read at a time, and a few
//! words per open handle: a write reaches the medium in the call that makes it.
//!
//! # Testing
//! All host-only; nothing here runs on the machine.
//! - `cargo test --release` (here): `tests/model.rs`, random operations against an in-memory model with
//!   handles held open and volumes run full; `tests/crash.rs`, power cut at every block write of fixed and
//!   random workloads and inside recovery, each mount equal to the model before or after the operation;
//!   `tests/hostile.rs`, damaged and forged volumes, refused as corrupt.
//! - `fuzz/` (its own workspace; nightly pinned in `fuzz/rust-toolchain.toml`), from `libs/walfs`: `cargo
//!   fuzz run -s none image` (arbitrary images mounted and walked) and `mutate` (a valid volume with bits
//!   flipped, mounted, walked and written).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod check;
mod file;
mod fs;
mod layout;
mod ops;

pub use check::Problem;
pub use file::{FileHandle, OpenOptions};
pub use fs::Filesystem;
pub use layout::Geometry;

/// Bytes per block.
pub const BLOCK: usize = 4096;
/// The most blocks one transaction writes.
pub const LOG_BLOCKS: usize = 32;
/// Bytes per inode.
pub(crate) const INODE: usize = 128;
/// Bytes of user attributes per inode.
pub const ATTRS: usize = 256;
/// Bytes per SHA-256.
pub(crate) const HASH: usize = 32;
/// Bytes per directory entry.
pub(crate) const DIRENT: usize = 260;
/// Bytes in a name.
pub const NAME_MAX: usize = 255;
/// Direct block addresses per inode.
pub(crate) const DIRECT: usize = 12;
/// Block addresses per indirect block.
pub(crate) const PER_INDIRECT: u64 = 1024;
/// The root directory's inode number.
pub const ROOT: u32 = 1;
/// The most blocks a file holds: the direct ones, a single-indirect block's and a double's.
pub(crate) const MAX_FILE_BLOCKS: u64 = DIRECT as u64 + PER_INDIRECT + PER_INDIRECT * PER_INDIRECT;
/// The most bytes a file holds.
pub const MAX_FILE_SIZE: u64 = MAX_FILE_BLOCKS * BLOCK as u64;
/// The most bytes of one attribute's value: the area less one record's type and length.
pub const ATTR_MAX: usize = ATTRS - 2;

/// One block.
pub type Block = [u8; BLOCK];

/// Storage as walfs sees it: `block_count` whole blocks. Implementations report failures as
/// [`Error::Io`].
///
/// # What a transaction relies on
/// Only that `sync` is durable: when it returns `Ok`, every write before it survives power loss.
/// Writes between two syncs may reach the medium in any order, and a write torn by power loss may
/// leave its block in any state; the hashes find it.
pub trait BlockDevice {
    /// Blocks on the device.
    fn block_count(&self) -> u32;
    fn read(&mut self, block: u32, buf: &mut Block) -> Result<(), Error>;
    fn write(&mut self, block: u32, data: &Block) -> Result<(), Error>;
    /// Makes every write so far durable.
    fn sync(&mut self) -> Result<(), Error>;
}

impl<D: BlockDevice + ?Sized> BlockDevice for &mut D {
    fn block_count(&self) -> u32 { (**self).block_count() }

    fn read(&mut self, block: u32, buf: &mut Block) -> Result<(), Error> { (**self).read(block, buf) }

    fn write(&mut self, block: u32, data: &Block) -> Result<(), Error> { (**self).write(block, data) }

    fn sync(&mut self) -> Result<(), Error> { (**self).sync() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The block device failed.
    Io,
    /// The volume is malformed, or a block does not match its hash.
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
    /// A bad argument: `.`, `..`, an empty name or one with a NUL, the root removed or renamed, a
    /// directory moved into itself, a closed or read-only handle, a create without write, an
    /// attribute type of 0, a geometry walfs cannot lay out.
    Invalid,
    /// No free block or inode; or attributes that do not fit their area.
    NoSpace,
    NameTooLong,
    /// A position or size past [`MAX_FILE_SIZE`].
    FileTooBig,
    /// No attribute of that type.
    NoAttr,
    /// An I/O error left memory and the medium possibly out of step; nothing more is done until the
    /// volume is mounted again, which recovers it.
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
    /// Bytes in a file; a directory's blocks in bytes.
    pub size: u64,
    pub inode: u32,
    /// The inode's allocation count: an inode and generation name one file for the volume's life.
    pub generation: u64,
    /// Microseconds since the Unix epoch; 0 until the machine has a clock.
    pub mtime: u64,
}

/// One entry, as [`Filesystem::read_dir`] passes it.
#[derive(Debug)]
pub struct DirEntry<'a> {
    pub name: &'a [u8],
    pub meta: Metadata,
}
