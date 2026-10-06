//! EROFS, the Enhanced Read-Only File System, in the uncompressed subset `erofsd` serves and the
//! bench's packer writes (docs/servers/erofsd.md, "The format", "The packer"): one definition of
//! the on-disk structures, shared by the parser and the writer.
//!
//! The layout is the kernel's, read from Linux v7.0's `Documentation/filesystems/erofs.rst` and
//! `fs/erofs/erofs_fs.h` (the field widths), not from its implementation. The subset:
//!
//! - **4 KiB blocks**, and directory blocks the same size; no incompatible feature at all (no compression,
//!   chunks, extra devices, 48-bit addresses, fragments or metabox).
//! - **The superblock** at byte 1024: the magic, the block size, the root's inode number, the inode area's
//!   first block and the block count, which must fit the range ([`Superblock`]).
//! - **Inodes**, compact (32 bytes) or extended (64), at `meta_blkaddr * 4096 + 32 * nid`: a regular file or
//!   a directory, laid out *flat plain* (consecutive blocks from its first) or *flat inline* (the same, with
//!   the last partial block after the inode and its extended attributes). Every other layout is [`Corrupt`].
//!   Extended attributes are sized and bounds-checked, never read ([`Inode`]).
//! - **Directories** as blocks of 12-byte entries followed by their names, sorted, so a lookup is a binary
//!   search ([`Dirents`]).
//!
//! Every offset the medium gives is checked before it is used, with overflow-checked arithmetic:
//! a volume that fails a check, or asks for anything outside the subset, is [`Corrupt`] (R49).
//! The parser allocates nothing and needs no more than the bytes it is handed.
//!
//! [`pack`] writes a tree in this subset: compact inodes (extended where a size needs 64 bits),
//! sorted directory blocks, flat inline tails where they fit, each file's SHA-256 as the extended
//! attribute `user.sha256`, a fixed timestamp of 0, and the same bytes for the same tree.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod read;
#[cfg(not(target_os = "none"))]
mod tree;
mod write;

pub use read::{Dirent, Dirents, Inode, Kind, Layout, Superblock};
#[cfg(not(target_os = "none"))]
pub use tree::{Tree, read_tree};
pub use write::{Entry, PackError, pack};

/// The block, and a directory's block: 4 KiB, the only size this subset reads.
pub const BLOCK: usize = 4096;
/// `log2(BLOCK)`, as the superblock records it.
pub const BLOCK_BITS: u8 = 12;
/// The superblock's offset in the volume.
pub const SUPERBLOCK_AT: usize = 1024;
/// The superblock's bytes without extension slots.
pub const SUPERBLOCK_LEN: usize = 128;
/// `EROFS_SUPER_MAGIC_V1`.
pub const MAGIC: u32 = 0xE0F5_E1E2;
/// An inode slot: inode numbers count these from the inode area's start.
pub const SLOT: usize = 32;
/// A compact inode's bytes.
pub const COMPACT: usize = 32;
/// An extended inode's bytes.
pub const EXTENDED: usize = 64;
/// A directory entry's fixed part: the inode number, the name's offset, the type.
pub const DIRENT: usize = 12;
/// The longest name.
pub const NAME_MAX: usize = 255;
/// A flat inline inode with no whole block records this as its first block.
pub const NULL_BLOCK: u32 = u32::MAX;

/// Where each field the subset uses lies, in bytes from the start of its structure
/// (`erofs_fs.h`): the parser reads it there, and the writer writes it there.
pub mod field {
    /// `erofs_super_block`, from [`crate::SUPERBLOCK_AT`].
    pub mod sb {
        pub const MAGIC: usize = 0;
        pub const BLOCK_BITS: usize = 12;
        pub const ROOT: usize = 14;
        pub const INODES: usize = 16;
        pub const BLOCKS: usize = 36;
        pub const META: usize = 40;
        pub const INCOMPAT: usize = 80;
        pub const EXTRA_DEVICES: usize = 86;
        pub const DIR_BLOCK_BITS: usize = 90;
    }
    /// `erofs_inode_compact` and `erofs_inode_extended`, which share all but the size's width
    /// and the link count's place.
    pub mod inode {
        pub const FORMAT: usize = 0;
        pub const XATTR_COUNT: usize = 2;
        pub const MODE: usize = 4;
        /// A compact inode's 16-bit link count.
        pub const NLINK: usize = 6;
        /// 32 bits in a compact inode, 64 in an extended one.
        pub const SIZE: usize = 8;
        pub const START: usize = 16;
        pub const INO: usize = 20;
        /// An extended inode's 32-bit link count.
        pub const NLINK_EXTENDED: usize = 44;
    }
    /// `erofs_dirent`.
    pub mod dirent {
        pub const NID: usize = 0;
        pub const NAME: usize = 8;
        pub const TYPE: usize = 10;
    }
}

/// `i_format`'s data layouts the subset reads: flat plain and flat inline (`EROFS_INODE_FLAT_*`).
pub const LAYOUT_PLAIN: u16 = 0;
pub const LAYOUT_INLINE: u16 = 2;
/// `i_mode`'s file types the subset serves.
pub const S_IFREG: u16 = 0o100_000;
pub const S_IFDIR: u16 = 0o040_000;
/// An inline attribute area's header: the name filter, the shared count, reserved bytes.
pub const XATTR_HEADER: usize = 12;

/// The volume is not in the subset, or not consistent: one answer for every way of failing, as
/// `erofsd` serves it (R49).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corrupt;

fn u16_at(b: &[u8], at: usize) -> Result<u16, Corrupt> {
    let bytes = b.get(at..at + 2).ok_or(Corrupt)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, Corrupt> {
    let bytes = b.get(at..at + 4).ok_or(Corrupt)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, Corrupt> {
    Ok(u64::from(u32_at(b, at)?) | u64::from(u32_at(b, at + 4)?) << 32)
}

#[cfg(test)]
mod tests;
