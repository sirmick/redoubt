//! littlefs in pure Rust: `no_std` + `alloc`, no dependencies, no `unsafe`.
//!
//! This implements the littlefs on-disk format, **version 2.1** (`SPEC.md` of
//! littlefs-project/littlefs, vendored with `DESIGN.md` in `diff/c/`, checked against the C
//! reference v2.11.3), for `fsd`: one filesystem server per volume
//! (docs/servers/fsd.md). Images this crate writes mount in the C reference and the
//! other way round; the C code runs only on the host, as a test oracle (`diff/`).
//!
//! # Shape
//! - [`BlockDevice`]: the four operations littlefs needs from storage, and the contract
//!   power-loss safety relies on.
//! - [`Filesystem`]: format, mount, and every operation `fsd` needs: files (open, read,
//!   write, seek, truncate, sync, close), directories (mkdir, remove, rename, read_dir),
//!   stat, user attributes (the reference's "custom attributes") on files and directories,
//!   and a volume check.
//! - Paths are `/`-separated names relative to the root; `.` and `..` are refused, and a
//!   trailing slash names a directory. Names read back from the medium are opaque bytes:
//!   nothing forces them to be UTF-8, or to be names a path could name (the check reports
//!   those). `fsd` must never join such a name into a path it then resolves.
//!
//! # Terms
//! A *pair* is a metadata pair: two blocks, one holding the current log of commits. Every
//! pair is on one linked *list of pairs* starting at blocks {0, 1} (the reference's
//! "threaded linked-list"); a directory is one or more consecutive pairs of it (joined by
//! hard tails). *Orphans* are directories on the list that no entry names.
//!
//! # The medium is hostile
//! Every length, offset, block pointer and tag read from the device is checked before it is
//! used. A malformed image yields [`Error::Corrupt`], never a panic. Every walk is bounded: a
//! walk along the list of pairs spends at most `block_count / 2` steps (no volume holds more
//! pairs); a file's skip-list walk is bounded by its size, which is checked against the
//! volume; a walk of the whole volume stops at `3 * block_count` blocks. The only recursion
//! is one level deep (a commit that empties a pair recommits to the pair before it).
//! Metadata is checksummed; file data is not (as in the reference).
//!
//! # Power loss
//! Every change reaches the disk as one metadata commit (or, for renames and directory
//! removal, a sequence the next mount completes or undoes), so an interrupted operation
//! leaves the state from before or after it. This is littlefs's design; the crash-injection
//! tests in `tests/` check it at every block write. Mounting writes nothing: the first
//! operation that writes after a mount first repairs what an interrupted one left.
//!
//! # Memory
//! One block-sized buffer per metadata fetch (freed when the fetch is done), one block per
//! file handle that is writing, and the allocator's bitmap of `block_count / 8` bytes.
//!
//! # Differences from the C reference
//! - Open file handles follow renames and survive removal (their data stays readable; sync
//!   then commits nothing). The reference detaches them.
//! - Renaming a directory into itself is refused.
//! - [`Filesystem::read_dir`] returns no `.` or `..` entries.
//! - Stricter log parsing: CRC-valid commits that make no sense (duplicate names, entries
//!   without names, tags out of range) are `Corrupt`, where the reference may accept them.
//! - Files up to `min(1022, block_size / 8)` bytes are stored inline (the reference also
//!   caps this by its cache size); either reads the other's inline files of any length.
//! - Mounting requires the configured block count to equal the superblock's.
//! - [`Filesystem::unmount`] drops open handles unsynced.
//! - A file's attributes and its data are two commits (the reference can do both in one).
//! - Only on-disk version 2.1: 2.0 images, which the reference upgrades in place, are
//!   refused with [`Error::Invalid`]; `fsd` formats its own volumes.
//!
//! # Left out on purpose
//! Wear levelling and bad-block relocation (`block_cycles`): `fsd` sits on a virtio disk,
//! whose device handles both; a failed program or erase is reported, not worked around. As a
//! consequence this crate never relocates metadata or grows the superblock chain, but it
//! reads images where the C reference did. No v1 migration, no `fs_grow`, no directory
//! handles (see [`Filesystem::read_dir`]).
//!
//! # Testing
//! All host-only; nothing here runs on the machine.
//! - `cargo test --release` (here): unit tests and forged hostile metadata (`src/tests.rs`);
//!   `tests/model.rs`, random operations against an in-memory model, with handles held open
//!   and volumes run full; `tests/crash.rs`, power failure at every write of fixed and random
//!   workloads (`MODEL_TRACE`, `MODEL_PEEK` and `MODEL_SEEDS` help debug the model test);
//!   `tests/hostile.rs`, corrupted and noise images, and hand-built hostile ones.
//! - `diff/` (its own workspace, outside the root one; needs a C compiler): the C reference
//!   v2.11.3 as an oracle. `cargo test --release` there runs Rust-writes-C-reads and the
//!   reverse, both taking turns on one image, and the reference with wear levelling on.
//!   `WL_SEED=n` runs one wear-levelling seed; `DIFF_ONLY=c` or `rust` replays the same
//!   operations with one side doing every step. Ignored tests show the reference's own leak.
//! - `fuzz/` (its own workspace; nightly pinned in `fuzz/rust-toolchain.toml`), from
//!   `libs/littlefs`: `cargo fuzz run -s none image fuzz/corpus/image fuzz/seeds/image`
//!   (arbitrary images) and the same for `mutate` (edits of a valid volume, optionally with
//!   the commit CRC fixed up). `fuzz/seeds/` is a minimized corpus to start from; `-s none`
//!   because the address sanitizer adds nothing to safe Rust and costs a factor of five.

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
#[cfg(test)]
mod tests;

pub use file::{FileHandle, OpenOptions};
pub use fs::Filesystem;

/// The on-disk version this crate reads and writes: major 2, minor 1. Older images (2.0) are
/// refused rather than upgraded as the reference does; `fsd` formats its own volumes.
pub const DISK_VERSION: u32 = 0x0002_0001;

/// Storage as littlefs sees it: `block_count` blocks of `block_size` bytes.
///
/// Reads may be of any range inside a block. Programs start and end on multiples of the
/// configured `prog_size` and only target bytes erased since they were last programmed.
/// Implementations report failures as [`Error::Io`].
///
/// # What power-loss safety relies on
/// The crash guarantees hold for a device that keeps this contract (the crash tests inject
/// exactly these failures; a device outside it is shown failing in `tests/crash.rs`):
/// - **A torn program persists a prefix.** If power fails during `prog`, the bytes that
///   landed are a prefix of whole program units, possibly followed by one partly written
///   unit; nothing after that. A unit is overwritten as a whole: a later program of it
///   replaces all its bytes (disk semantics) or only clears bits of erased bytes (flash).
/// - **A torn erase** leaves the block erased, untouched, or erased only in part.
/// - **Order within a block**: an erase and later programs of the same block reach the
///   medium in the order they were issued.
/// - **`sync` means durable**: when it returns `Ok`, everything programmed or erased
///   before it survives power loss. The filesystem syncs before each metadata commit that
///   depends on data blocks, and after each commit.
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
    /// Bytes per block (the erase unit), a multiple of `prog_size`. At least 128, the
    /// reference's minimum (SPEC.md's bound for the CTZ pointers alone is 104).
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
    /// A bad argument or an unusable volume: `.`, `..`, an empty name or one with a NUL, a
    /// closed or read-only handle, a move of a directory into itself, a position past
    /// `file_max`, a geometry littlefs cannot use, another block size or count than the
    /// superblock's, an on-disk version other than 2.1.
    Invalid,
    /// No free blocks; or the metadata of one entry does not fit a block (a long name with
    /// large attributes on small blocks); or an attribute longer than `attr_max`.
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

/// What [`Filesystem::check`] found on a volume that is not damaged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Clean,
    /// Leftovers the first write after mount repairs: a half-done rename, orphaned or
    /// half-orphaned directories on the list of pairs.
    NeedsRepair,
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
    pub meta: Metadata,
}
