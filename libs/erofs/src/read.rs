//! The parser: a superblock, an inode, a directory block, each checked whole before anything in
//! it is believed.

use crate::field::{dirent, inode, sb as field_sb};
use crate::{
    BLOCK, BLOCK_BITS, COMPACT, Corrupt, DIRENT, EXTENDED, LAYOUT_INLINE, LAYOUT_PLAIN, MAGIC, NAME_MAX,
    S_IFDIR, S_IFREG, SLOT, SUPERBLOCK_AT, SUPERBLOCK_LEN, XATTR_HEADER, u16_at, u32_at, u64_at,
};

/// `i_format`'s bits this subset knows: the inode's size (bit 0), its layout (bits 1 to 3) and
/// bit 4, which on a compact file says its link count is 1 and on a directory that `.` is left
/// out. Any other bit set is corrupt, as the kernel holds it.
const FORMAT_BITS: u16 = 0x1f;
const FORMAT_EXTENDED: u16 = 1;
const FORMAT_NLINK_1: u16 = 1 << 4;
const S_IFMT: u16 = 0o170_000;

const BLOCK64: u64 = BLOCK as u64;

/// What the superblock says, checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Superblock {
    /// The root directory's inode number.
    pub root: u64,
    /// The inode area's first block.
    pub meta: u32,
    /// The volume's blocks; no more than its range holds.
    pub blocks: u32,
}

impl Superblock {
    /// The superblock from `head`, the volume's first bytes (at least [`SUPERBLOCK_AT`] +
    /// [`SUPERBLOCK_LEN`]), for a range of `range_blocks` blocks: the magic, 4 KiB blocks and
    /// directory blocks, no incompatible feature and no extra device, a block count the range
    /// holds, an inode area inside it, and a root inode inside that.
    pub fn parse(head: &[u8], range_blocks: u64) -> Result<Superblock, Corrupt> {
        let sb = head.get(SUPERBLOCK_AT..SUPERBLOCK_AT + SUPERBLOCK_LEN).ok_or(Corrupt)?;
        if u32_at(sb, field_sb::MAGIC)? != MAGIC
            || sb[field_sb::BLOCK_BITS] != BLOCK_BITS
            || sb[field_sb::DIR_BLOCK_BITS] != 0
        {
            return Err(Corrupt);
        }
        // Every incompatible feature changes how something is laid out; the subset has none.
        if u32_at(sb, field_sb::INCOMPAT)? != 0 || u16_at(sb, field_sb::EXTRA_DEVICES)? != 0 {
            return Err(Corrupt);
        }
        let (blocks, meta) = (u32_at(sb, field_sb::BLOCKS)?, u32_at(sb, field_sb::META)?);
        if blocks == 0 || u64::from(blocks) > range_blocks || meta >= blocks {
            return Err(Corrupt);
        }
        let sb = Superblock { root: u64::from(u16_at(sb, field_sb::ROOT)?), meta, blocks };
        sb.inode_at(sb.root)?;
        Ok(sb)
    }

    /// The volume's bytes.
    pub fn bytes(&self) -> u64 { u64::from(self.blocks) * BLOCK64 }

    /// Where inode `nid` starts, in bytes: in the inode area, with a compact inode's room before
    /// the volume's end.
    pub fn inode_at(&self, nid: u64) -> Result<u64, Corrupt> {
        let at = nid
            .checked_mul(SLOT as u64)
            .and_then(|at| at.checked_add(u64::from(self.meta) * BLOCK64))
            .ok_or(Corrupt)?;
        match at.checked_add(COMPACT as u64) {
            Some(end) if end <= self.bytes() => Ok(at),
            _ => Err(Corrupt),
        }
    }
}

/// What an inode is: the subset serves files and directories, nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
}

/// How an inode's data lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Consecutive blocks from the first, the last cut to the size (`EROFS_INODE_FLAT_PLAIN`).
    Plain,
    /// The same for the whole blocks, and the last partial block right after the inode and its
    /// extended attributes (`EROFS_INODE_FLAT_INLINE`).
    Inline,
}

/// An inode, checked: its data lies inside the volume, its inline tail inside one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    nid: u64,
    kind: Kind,
    layout: Layout,
    size: u64,
    nlink: u32,
    /// The first data block; meaningful only when `blocks` is not 0.
    start: u32,
    /// Whole blocks from `start`: every block of a flat plain inode, all but the tail of an
    /// inline one.
    blocks: u64,
    /// Where an inline tail starts, in bytes.
    tail_at: u64,
}

impl Inode {
    /// Inode `nid` of `sb`'s volume, from `bytes`, the volume's bytes from where it starts
    /// ([`Superblock::inode_at`]): at least [`EXTENDED`] of them, or to the volume's end.
    /// Anything but a file or a directory, a layout outside the subset, an unknown format bit,
    /// extended attributes past the volume, data blocks past the block count or an inline tail
    /// that crosses a block is corrupt.
    pub fn parse(sb: &Superblock, nid: u64, bytes: &[u8]) -> Result<Inode, Corrupt> {
        let at = sb.inode_at(nid)?;
        let format = u16_at(bytes, inode::FORMAT)?;
        if format & !FORMAT_BITS != 0 {
            return Err(Corrupt);
        }
        let layout = match (format >> 1) & 7 {
            LAYOUT_PLAIN => Layout::Plain,
            LAYOUT_INLINE => Layout::Inline,
            _ => return Err(Corrupt),
        };
        let (xattr_count, mode) = (u16_at(bytes, inode::XATTR_COUNT)?, u16_at(bytes, inode::MODE)?);
        let kind = match mode & S_IFMT {
            S_IFREG => Kind::File,
            S_IFDIR => Kind::Dir,
            _ => return Err(Corrupt),
        };
        let (len, size, nlink) = if format & FORMAT_EXTENDED != 0 {
            (EXTENDED, u64_at(bytes, inode::SIZE)?, u32_at(bytes, inode::NLINK_EXTENDED)?)
        } else if format & FORMAT_NLINK_1 != 0 && kind == Kind::File {
            (COMPACT, u64::from(u32_at(bytes, inode::SIZE)?), 1)
        } else {
            (COMPACT, u64::from(u32_at(bytes, inode::SIZE)?), u32::from(u16_at(bytes, inode::NLINK)?))
        };
        let start = u32_at(bytes, inode::START)?;
        // Sized, never read: one header and four bytes per count after the first.
        let xattrs = match xattr_count {
            0 => 0,
            n => XATTR_HEADER as u64 + 4 * (u64::from(n) - 1),
        };
        let end = sb.bytes();
        let meta_end = at.checked_add(len as u64 + xattrs).filter(|e| *e <= end).ok_or(Corrupt)?;
        let (blocks, tail) = match layout {
            Layout::Plain => (size.div_ceil(BLOCK64), 0),
            Layout::Inline => (size / BLOCK64, size % BLOCK64),
        };
        if blocks > 0 && u64::from(start).checked_add(blocks).is_none_or(|last| last > u64::from(sb.blocks)) {
            return Err(Corrupt);
        }
        if tail > 0 && (meta_end % BLOCK64 + tail > BLOCK64 || meta_end + tail > end) {
            return Err(Corrupt);
        }
        Ok(Inode { nid, kind, layout, size, nlink, start, blocks, tail_at: meta_end })
    }

    pub fn nid(&self) -> u64 { self.nid }

    pub fn kind(&self) -> Kind { self.kind }

    pub fn layout(&self) -> Layout { self.layout }

    /// The data's bytes.
    pub fn size(&self) -> u64 { self.size }

    pub fn nlink(&self) -> u32 { self.nlink }

    /// Where byte `offset` of the data lies on the volume, and how many bytes run on from there
    /// in one piece (to the end of the whole blocks, or of the data); `None` at or past the end.
    pub fn extent(&self, offset: u64) -> Option<(u64, u64)> {
        if offset >= self.size {
            return None;
        }
        // Every term was bounded by the volume's size in `parse`.
        let whole = self.blocks * BLOCK64;
        if offset < whole {
            Some((u64::from(self.start) * BLOCK64 + offset, whole.min(self.size) - offset))
        } else {
            Some((self.tail_at + (offset - whole), self.size - offset))
        }
    }

    /// A directory's blocks: its size in blocks, the last one partial.
    pub fn dir_blocks(&self) -> u64 { self.size.div_ceil(BLOCK64) }

    /// Where a directory's block `index` lies, in bytes, and its length: [`BLOCK`], or less for
    /// the last.
    pub fn dir_block(&self, index: u64) -> Option<(u64, usize)> {
        let (at, run) = self.extent(index.checked_mul(BLOCK64)?)?;
        Some((at, run.min(BLOCK64) as usize))
    }
}

/// One directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dirent<'a> {
    /// The inode it names.
    pub nid: u64,
    /// Its name: never empty, at most [`NAME_MAX`] bytes, no `/` and no NUL.
    pub name: &'a [u8],
    /// The type the directory records (`1` a file, `2` a directory); the inode decides.
    pub file_type: u8,
}

/// A directory block, checked: a whole number of entries, every name inside the block and
/// well-formed, and the names strictly in order, so no two are the same.
#[derive(Clone, Copy, Debug)]
pub struct Dirents<'a> {
    block: &'a [u8],
    count: usize,
}

impl<'a> Dirents<'a> {
    /// The entries of `block`, at most a [`BLOCK`] long. The first name's offset gives the count
    /// (its entries end where the names begin); each name ends where the next begins, and the
    /// last at the block's end or its first NUL.
    pub fn parse(block: &'a [u8]) -> Result<Dirents<'a>, Corrupt> {
        if block.len() > BLOCK {
            return Err(Corrupt);
        }
        let first = usize::from(u16_at(block, dirent::NAME)?);
        if first == 0 || !first.is_multiple_of(DIRENT) || first > block.len() {
            return Err(Corrupt);
        }
        let dirents = Dirents { block, count: first / DIRENT };
        let mut previous: Option<&[u8]> = None;
        for i in 0..dirents.count {
            let name = dirents.name(i)?;
            if name.is_empty() || name.len() > NAME_MAX || name.iter().any(|b| *b == b'/' || *b == 0) {
                return Err(Corrupt);
            }
            if previous.is_some_and(|p| p >= name) {
                return Err(Corrupt);
            }
            previous = Some(name);
        }
        Ok(dirents)
    }

    fn name(&self, i: usize) -> Result<&'a [u8], Corrupt> {
        let from = usize::from(u16_at(self.block, i * DIRENT + dirent::NAME)?);
        let last = i + 1 == self.count;
        let to = if last {
            self.block.len()
        } else {
            usize::from(u16_at(self.block, (i + 1) * DIRENT + dirent::NAME)?)
        };
        if from < self.count * DIRENT || from > to || to > self.block.len() {
            return Err(Corrupt);
        }
        let name = &self.block[from..to];
        Ok(if last { name.split(|b| *b == 0).next().unwrap_or(name) } else { name })
    }

    pub fn len(&self) -> usize { self.count }

    pub fn is_empty(&self) -> bool { self.count == 0 }

    /// Entry `i`, in name order.
    pub fn get(&self, i: usize) -> Option<Dirent<'a>> {
        if i >= self.count {
            return None;
        }
        let name = self.name(i).ok()?;
        Some(Dirent {
            nid: u64_at(self.block, i * DIRENT + dirent::NID).ok()?,
            name,
            file_type: self.block[i * DIRENT + dirent::TYPE],
        })
    }

    /// The entry named `name`, by binary search.
    pub fn lookup(&self, name: &[u8]) -> Option<Dirent<'a>> {
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            let entry = self.get(mid)?;
            match entry.name.cmp(name) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return Some(entry),
            }
        }
        None
    }

    /// Every entry, in name order.
    pub fn iter(&self) -> impl Iterator<Item = Dirent<'a>> + use<'a> {
        let dirents = *self;
        (0..self.count).filter_map(move |i| dirents.get(i))
    }
}
