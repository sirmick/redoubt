//! Metadata tags (SPEC.md "Metadata tags").
//!
//! ```text
//! [1|--- 11 ---|-- 10 --|-- 10 --]
//!  ^      ^        ^        ^- length of the data that follows (0x3ff: this tag deletes)
//!  |      |        '---------- id (0x3ff: not tied to a file)
//!  |      '------------------- type3 = type1 (3 bits) + chunk (8 bits)
//!  '-------------------------- valid bit: 0 when valid, after the XOR with the previous tag
//! ```
//!
//! Tags are the one big-endian thing on disk; each one is stored XORed with the tag before it.

/// A block pointer that points nowhere.
pub(crate) const BLOCK_NULL: u32 = 0xffff_ffff;

// type1 groups
pub(crate) const T1_NAME: u16 = 0x0;
pub(crate) const T1_STRUCT: u16 = 0x2;
pub(crate) const T1_USERATTR: u16 = 0x3;
pub(crate) const T1_SPLICE: u16 = 0x4;
pub(crate) const T1_TAIL: u16 = 0x6;
pub(crate) const T1_GSTATE: u16 = 0x7;

// type3 values
pub(crate) const TYPE_REG: u16 = 0x001;
pub(crate) const TYPE_DIR: u16 = 0x002;
pub(crate) const TYPE_SUPERBLOCK: u16 = 0x0ff;
pub(crate) const TYPE_DIRSTRUCT: u16 = 0x200;
pub(crate) const TYPE_INLINESTRUCT: u16 = 0x201;
pub(crate) const TYPE_CTZSTRUCT: u16 = 0x202;
pub(crate) const TYPE_USERATTR: u16 = 0x300;
pub(crate) const TYPE_CREATE: u16 = 0x401;
pub(crate) const TYPE_DELETE: u16 = 0x4ff;
pub(crate) const TYPE_CCRC: u16 = 0x500;
pub(crate) const TYPE_FCRC: u16 = 0x5ff;
pub(crate) const TYPE_SOFTTAIL: u16 = 0x600;
pub(crate) const TYPE_MOVESTATE: u16 = 0x7ff;

/// The id of tags that belong to the metadata pair rather than to a file.
pub(crate) const ID_NONE: u16 = 0x3ff;
/// The length field value that marks a tag as a deletion of what it names.
pub(crate) const SIZE_DELETE: u16 = 0x3ff;
/// The largest data length a tag can carry (0x3ff means "delete").
pub(crate) const MAX_TAG_DATA: usize = 0x3fe;

pub(crate) fn mk(type3: u16, id: u16, size: u16) -> u32 {
    ((type3 as u32 & 0x7ff) << 20) | ((id as u32 & 0x3ff) << 10) | (size as u32 & 0x3ff)
}

pub(crate) fn is_valid(tag: u32) -> bool { tag & 0x8000_0000 == 0 }

pub(crate) fn type1(tag: u32) -> u16 { ((tag >> 28) & 0x7) as u16 }

pub(crate) fn type3(tag: u32) -> u16 { ((tag >> 20) & 0x7ff) as u16 }

pub(crate) fn chunk(tag: u32) -> u8 { (tag >> 20) as u8 }

pub(crate) fn id(tag: u32) -> u16 { ((tag >> 10) & 0x3ff) as u16 }

pub(crate) fn size(tag: u32) -> u16 { (tag & 0x3ff) as u16 }

pub(crate) fn is_delete(tag: u32) -> bool { size(tag) == SIZE_DELETE }

/// Bytes the tag occupies on disk: itself plus its data (a deleting tag carries none).
pub(crate) fn dsize(tag: u32) -> u32 { 4 + if is_delete(tag) { 0 } else { size(tag) as u32 } }

/// Commit CRC tags are every 0x500-0x57f type (the low chunk bits are flags); 0x5ff (the
/// forward CRC) is not one.
pub(crate) fn is_commit_crc(tag: u32) -> bool { type3(tag) & 0x780 == TYPE_CCRC }

/// File-system entries (regular files and directories) are the names a lookup can see.
/// The superblock entry is a name too, but of type 0x0ff, which lookups never match.
pub(crate) fn is_file_or_dir(name_type: u16) -> bool { name_type == TYPE_REG || name_type == TYPE_DIR }
