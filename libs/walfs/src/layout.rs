//! The format's records, as docs/servers/walfsd.md's tables give them: the geometry, the
//! superblock, the log's header, the inode, the attribute area and the directory entry. Every
//! parse checks every field; every encode writes exactly the page's bytes.

use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use crate::{
    ATTRS, BLOCK, Block, DIRECT, DIRENT, Error, HASH, INODE, LOG_BLOCKS, MAX_FILE_SIZE, NAME_MAX,
    PER_INDIRECT, ROOT,
};

/// The superblock's magic.
pub const MAGIC: [u8; 8] = *b"walfs\0\0\0";
/// The log header's magic.
pub const LOG_MAGIC: [u8; 8] = *b"walfslog";
pub const VERSION: u32 = 1;
/// The log's first block, and its length: the header, then the logged blocks.
pub const LOG_START: u32 = 1;
pub const LOG: u32 = 1 + LOG_BLOCKS as u32;
/// The inode table's first block.
pub const INODE_START: u32 = LOG_START + LOG;
pub const INODES_PER_BLOCK: u32 = (BLOCK / INODE) as u32;
pub const ATTRS_PER_BLOCK: u32 = (BLOCK / ATTRS) as u32;
/// Slots per hash block: the last 32 bytes hold the block's own hash.
pub const HASH_SLOTS: u32 = (BLOCK / HASH) as u32 - 1;
pub const BITS_PER_BLOCK: u32 = (BLOCK * 8) as u32;
pub const DIRENTS_PER_BLOCK: usize = BLOCK / DIRENT;
/// The first file block the single-indirect block maps, and the double's.
pub const SINGLE_BASE: u64 = DIRECT as u64;
pub const DOUBLE_BASE: u64 = SINGLE_BASE + PER_INDIRECT;
/// Where a self-hashed block (the superblock, the log's header, a hash block) keeps its hash.
pub const SELF_HASH_AT: usize = BLOCK - HASH;

/// The SHA-256 of `data`.
pub fn sha(data: &[u8]) -> [u8; HASH] { Sha256::digest(data).into() }

/// Whether a self-hashed block's hash covers its bytes.
pub fn self_hash_ok(b: &Block) -> bool { b[SELF_HASH_AT..] == sha(&b[..SELF_HASH_AT]) }

/// Sets a self-hashed block's hash.
pub fn seal(b: &mut Block) {
    let h = sha(&b[..SELF_HASH_AT]);
    b[SELF_HASH_AT..].copy_from_slice(&h);
}

pub fn u16_at(b: &[u8], at: usize) -> u16 { u16::from_le_bytes([b[at], b[at + 1]]) }
pub fn u32_at(b: &[u8], at: usize) -> u32 { u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) }
pub fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(x)
}
pub fn put_u32(b: &mut [u8], at: usize, v: u32) { b[at..at + 4].copy_from_slice(&v.to_le_bytes()) }
fn put_u16(b: &mut [u8], at: usize, v: u16) { b[at..at + 2].copy_from_slice(&v.to_le_bytes()) }
fn put_u64(b: &mut [u8], at: usize, v: u64) { b[at..at + 8].copy_from_slice(&v.to_le_bytes()) }

/// What `format` lays out: `block_count` blocks with `inode_count` inodes (a multiple of 32, at
/// least 32).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub block_count: u32,
    pub inode_count: u32,
}

impl Geometry {
    /// A volume of `block_count` blocks with an inode for every 16 blocks (64 KiB), and at least 32.
    pub fn for_blocks(block_count: u32) -> Geometry {
        let inodes = (block_count / 16).div_ceil(INODES_PER_BLOCK).max(1) * INODES_PER_BLOCK;
        Geometry { block_count, inode_count: inodes }
    }
}

/// Where each region starts, from a geometry; the regions follow one another in the page's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub block_count: u32,
    pub inode_count: u32,
    pub attr_start: u32,
    pub hash_start: u32,
    pub bitmap_start: u32,
    pub data_start: u32,
}

/// Where the superblock keeps its block and inode counts; its fields are `u32`s from byte 8 in the
/// page's order.
const SB_FIELDS: usize = 8;
const SB_BLOCK_COUNT: usize = 16;
const SB_INODE_COUNT: usize = 32;

impl Layout {
    /// The layout of `g`, or `None` if the inode count is not a whole number of blocks of at least
    /// one, or the regions leave no data block.
    pub fn new(g: Geometry) -> Option<Layout> {
        let Geometry { block_count, inode_count } = g;
        if inode_count == 0 || !inode_count.is_multiple_of(INODES_PER_BLOCK) {
            return None;
        }
        let attr_start = INODE_START.checked_add(inode_count / INODES_PER_BLOCK)?;
        let hash_start = attr_start.checked_add(inode_count / ATTRS_PER_BLOCK)?;
        let slots = block_count.checked_sub(LOG)?;
        let bitmap_start = hash_start.checked_add(slots.div_ceil(HASH_SLOTS))?;
        let data_start = bitmap_start.checked_add(block_count.div_ceil(BITS_PER_BLOCK))?;
        if data_start >= block_count {
            return None;
        }
        Some(Layout { block_count, inode_count, attr_start, hash_start, bitmap_start, data_start })
    }

    pub fn is_data(&self, b: u32) -> bool { b >= self.data_start && b < self.block_count }

    /// A block a transaction may write home: the tables, the hash region, the bitmap and data.
    pub fn is_home(&self, b: u32) -> bool { b >= INODE_START && b < self.block_count }

    /// A block read through its slot: all but the log's, which the header checks, and the hash
    /// region's, which check themselves.
    pub fn is_hashed(&self, b: u32) -> bool { b < self.block_count && !self.is_log(b) && !self.is_hash(b) }

    pub fn is_log(&self, b: u32) -> bool { (LOG_START..LOG_START + LOG).contains(&b) }

    pub fn is_hash(&self, b: u32) -> bool { (self.hash_start..self.bitmap_start).contains(&b) }

    /// The hash block holding block `b`'s slot, and the slot's byte offset in it. Every block but
    /// the log's has a slot, the superblock's first, then the rest in order from the inode table.
    pub fn slot(&self, b: u32) -> (u32, usize) {
        let i = if b < LOG_START { b } else { b - LOG };
        (self.hash_start + i / HASH_SLOTS, (i % HASH_SLOTS) as usize * HASH)
    }

    /// The inode table's block holding inode `i`, and its offset.
    pub fn inode_at(&self, i: u32) -> (u32, usize) {
        (INODE_START + i / INODES_PER_BLOCK, (i % INODES_PER_BLOCK) as usize * INODE)
    }

    /// The attribute table's block holding inode `i`'s area, and its offset.
    pub fn attrs_at(&self, i: u32) -> (u32, usize) {
        (self.attr_start + i / ATTRS_PER_BLOCK, (i % ATTRS_PER_BLOCK) as usize * ATTRS)
    }

    /// The bitmap block holding block `b`'s bit, the byte in it, and the bit's mask.
    pub fn bit_at(&self, b: u32) -> (u32, usize, u8) {
        (self.bitmap_start + b / BITS_PER_BLOCK, (b % BITS_PER_BLOCK / 8) as usize, 1 << (b % 8))
    }

    pub fn bitmap_blocks(&self) -> u32 { self.data_start - self.bitmap_start }

    /// The superblock: the page's table, then its own hash.
    pub fn superblock(&self) -> Block {
        let mut b = [0u8; BLOCK];
        b[0..8].copy_from_slice(&MAGIC);
        let fields = [
            VERSION,
            BLOCK as u32,
            self.block_count,
            LOG_START,
            LOG,
            INODE_START,
            self.inode_count,
            self.attr_start,
            self.hash_start,
            self.bitmap_start,
            self.data_start,
            ROOT,
        ];
        for (k, v) in fields.iter().enumerate() {
            put_u32(&mut b, SB_FIELDS + 4 * k, *v);
        }
        seal(&mut b);
        b
    }

    /// The layout a superblock describes, on a device of `device_blocks` blocks: its own hash
    /// first, then every field against what its block and inode counts give.
    pub fn parse(b: &Block, device_blocks: u32) -> Result<Layout, Error> {
        if !self_hash_ok(b) || b[0..8] != MAGIC {
            return Err(Error::Corrupt);
        }
        let g = Geometry { block_count: u32_at(b, SB_BLOCK_COUNT), inode_count: u32_at(b, SB_INODE_COUNT) };
        let l = Layout::new(g).ok_or(Error::Corrupt)?;
        if l.superblock() != *b || g.block_count != device_blocks {
            return Err(Error::Corrupt);
        }
        Ok(l)
    }
}

/// The log's header: committed or empty, and per logged block its home and hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub entries: Vec<(u32, [u8; HASH])>,
}

const HDR_COMMIT: usize = 8;
const HDR_COUNT: usize = 12;
const HDR_ENTRIES: usize = 16;
const HDR_ENTRY: usize = 4 + HASH;

impl Header {
    /// The header block: committed if it holds entries, empty if not.
    pub fn encode(&self) -> Block {
        let mut b = [0u8; BLOCK];
        b[0..8].copy_from_slice(&LOG_MAGIC);
        put_u32(&mut b, HDR_COMMIT, !self.entries.is_empty() as u32);
        put_u32(&mut b, HDR_COUNT, self.entries.len() as u32);
        for (k, (home, hash)) in self.entries.iter().enumerate() {
            let at = HDR_ENTRIES + k * HDR_ENTRY;
            put_u32(&mut b, at, *home);
            b[at + 4..at + HDR_ENTRY].copy_from_slice(hash);
        }
        seal(&mut b);
        b
    }

    /// `None` for a header whose own hash fails: torn by a power cut, so no transaction. A header
    /// whose hash holds but whose fields do not is corrupt; so is a committed one naming a block
    /// that is not a home, or one block twice.
    pub fn parse(b: &Block, l: &Layout) -> Result<Option<Header>, Error> {
        if !self_hash_ok(b) {
            return Ok(None);
        }
        let (commit, count) = (u32_at(b, HDR_COMMIT), u32_at(b, HDR_COUNT) as usize);
        let ok = b[0..8] == LOG_MAGIC && commit <= 1 && (commit == 1) == (count > 0) && count <= LOG_BLOCKS;
        if !ok || b[HDR_ENTRIES + count * HDR_ENTRY..SELF_HASH_AT].iter().any(|&x| x != 0) {
            return Err(Error::Corrupt);
        }
        let mut h = Header::default();
        for k in 0..count {
            let at = HDR_ENTRIES + k * HDR_ENTRY;
            let home = u32_at(b, at);
            if !l.is_home(home) || h.entries.iter().any(|e| e.0 == home) {
                return Err(Error::Corrupt);
            }
            let mut hash = [0u8; HASH];
            hash.copy_from_slice(&b[at + 4..at + HDR_ENTRY]);
            h.entries.push((home, hash));
        }
        Ok(Some(h))
    }
}

pub const KIND_FREE: u16 = 0;
pub const KIND_FILE: u16 = 1;
pub const KIND_DIR: u16 = 2;

/// An inode as the table holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inode {
    pub kind: u16,
    pub nlink: u16,
    /// The next inode on the orphan list; for inode 0, the list's head.
    pub next: u32,
    pub size: u64,
    pub generation: u64,
    pub mtime: u64,
    pub direct: [u32; DIRECT],
    pub single: u32,
    pub double: u32,
}

/// The inode's fields' offsets, as the page's table gives them.
const I_KIND: usize = 0;
const I_NLINK: usize = 2;
const I_NEXT: usize = 4;
const I_SIZE: usize = 8;
const I_GENERATION: usize = 16;
const I_MTIME: usize = 24;
const I_DIRECT: usize = 32;
const I_SINGLE: usize = 80;
const I_DOUBLE: usize = 84;
const I_RESERVED: usize = 88;

impl Inode {
    pub fn encode(&self, out: &mut [u8]) {
        out[..INODE].fill(0);
        put_u16(out, I_KIND, self.kind);
        put_u16(out, I_NLINK, self.nlink);
        put_u32(out, I_NEXT, self.next);
        put_u64(out, I_SIZE, self.size);
        put_u64(out, I_GENERATION, self.generation);
        put_u64(out, I_MTIME, self.mtime);
        for (k, a) in self.direct.iter().enumerate() {
            put_u32(out, I_DIRECT + 4 * k, *a);
        }
        put_u32(out, I_SINGLE, self.single);
        put_u32(out, I_DOUBLE, self.double);
    }

    /// Inode `i` from its 128 bytes: every field in range for the volume, every address a hole or
    /// in the data region, a free inode zero but its generation (and, for inode 0, the list's
    /// head).
    pub fn parse(i: u32, b: &[u8], l: &Layout) -> Result<Inode, Error> {
        let mut direct = [0u32; DIRECT];
        for (k, a) in direct.iter_mut().enumerate() {
            *a = u32_at(b, I_DIRECT + 4 * k);
        }
        let n = Inode {
            kind: u16_at(b, I_KIND),
            nlink: u16_at(b, I_NLINK),
            next: u32_at(b, I_NEXT),
            size: u64_at(b, I_SIZE),
            generation: u64_at(b, I_GENERATION),
            mtime: u64_at(b, I_MTIME),
            direct,
            single: u32_at(b, I_SINGLE),
            double: u32_at(b, I_DOUBLE),
        };
        let addrs_ok = n.direct.iter().chain([&n.single, &n.double]).all(|&a| a == 0 || l.is_data(a));
        let ok = match n.kind {
            KIND_FREE => {
                let mut bare = Inode { generation: n.generation, ..Inode::default() };
                if i == 0 {
                    bare = Inode { next: n.next, ..Inode::default() };
                }
                n == bare && n.next < l.inode_count
            }
            KIND_FILE | KIND_DIR => {
                i >= ROOT
                    && n.nlink <= 1
                    && n.next < l.inode_count
                    && n.size <= MAX_FILE_SIZE
                    && (n.kind == KIND_FILE || n.size.is_multiple_of(BLOCK as u64))
                    && addrs_ok
            }
            _ => false,
        };
        if !ok || b[I_RESERVED..INODE].iter().any(|&x| x != 0) {
            return Err(Error::Corrupt);
        }
        Ok(n)
    }
}

/// An inode's attributes from its area: one record per type, each type 1 to 255, the rest zero.
pub fn parse_attrs(b: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, Error> {
    let mut out: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut at = 0;
    while at < ATTRS && b[at] != 0 {
        let (typ, len) = (b[at], *b.get(at + 1).ok_or(Error::Corrupt)? as usize);
        let value = b.get(at + 2..at + 2 + len).ok_or(Error::Corrupt)?;
        if out.iter().any(|a| a.0 == typ) {
            return Err(Error::Corrupt);
        }
        out.push((typ, value.into()));
        at += 2 + len;
    }
    if b[at.min(ATTRS)..ATTRS].iter().any(|&x| x != 0) {
        return Err(Error::Corrupt);
    }
    Ok(out)
}

/// An attribute area holding `attrs`, or `None` if they do not fit.
pub fn encode_attrs(attrs: &[(u8, Vec<u8>)]) -> Option<[u8; ATTRS]> {
    let mut out = [0u8; ATTRS];
    let mut at = 0;
    for (typ, value) in attrs {
        let end = at + 2 + value.len();
        if end > ATTRS {
            return None;
        }
        out[at] = *typ;
        out[at + 1] = value.len() as u8;
        out[at + 2..end].copy_from_slice(value);
        at = end;
    }
    Some(out)
}

/// A name a directory entry may hold.
pub fn name_ok(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name != b"."
        && name != b".."
        && !name.contains(&0)
        && !name.contains(&b'/')
}

/// Directory entry `k` of a block: `None` for a free slot, else the inode and the name. A slot or a
/// block's tail that the page says is zero and is not, or a name the page does not allow, is
/// corrupt.
pub fn dirent<'a>(b: &'a Block, k: usize, l: &Layout) -> Result<Option<(u32, &'a [u8])>, Error> {
    if k == 0 && b[DIRENTS_PER_BLOCK * DIRENT..].iter().any(|&x| x != 0) {
        return Err(Error::Corrupt);
    }
    let e = &b[k * DIRENT..(k + 1) * DIRENT];
    let (ino, len) = (u32_at(e, 0), e[4] as usize);
    if ino == 0 && len == 0 {
        return if e.iter().all(|&x| x == 0) { Ok(None) } else { Err(Error::Corrupt) };
    }
    let name = &e[5..5 + len];
    if ino < ROOT || ino >= l.inode_count || !name_ok(name) || e[5 + len..].iter().any(|&x| x != 0) {
        return Err(Error::Corrupt);
    }
    Ok(Some((ino, name)))
}

/// Writes entry `k` of a block: `ino` named `name`, or a free slot for inode 0.
pub fn put_dirent(b: &mut Block, k: usize, ino: u32, name: &[u8]) {
    let e = &mut b[k * DIRENT..(k + 1) * DIRENT];
    e.fill(0);
    if ino != 0 {
        put_u32(e, 0, ino);
        e[4] = name.len() as u8;
        e[5..5 + name.len()].copy_from_slice(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The records' sizes and offsets are the page's tables.
    #[test]
    fn the_records_are_the_pages_tables() {
        assert_eq!((INODE_START, LOG), (34, 33));
        assert_eq!((INODES_PER_BLOCK, ATTRS_PER_BLOCK, HASH_SLOTS, DIRENTS_PER_BLOCK), (32, 16, 127, 15));
        assert_eq!(HDR_ENTRIES + LOG_BLOCKS * HDR_ENTRY, 1168);
        assert_eq!(I_RESERVED + 40, INODE);
        assert_eq!(crate::MAX_FILE_SIZE, 4_299_210_752);

        let l = Layout::new(Geometry { block_count: 300_000, inode_count: 64 }).unwrap();
        let sb = l.superblock();
        let words: Vec<u32> = (0..13).map(|k| u32_at(&sb, 8 + 4 * k)).collect();
        let want = [1, 4096, 300_000, 1, 33, 34, 64, 36, 40, 2402, 2412, 1];
        assert_eq!(&words[..12], &want);
        assert_eq!(&sb[0..8], b"walfs\0\0\0");
        assert!(sb[56..4064].iter().all(|&x| x == 0));
        assert_eq!(Layout::parse(&sb, 300_000), Ok(l));
        assert_eq!(Layout::parse(&sb, 299_999), Err(Error::Corrupt));

        let n = Inode {
            kind: KIND_FILE,
            nlink: 1,
            next: 3,
            size: 0x0102_0304,
            generation: 7,
            mtime: 9,
            direct: [100; DIRECT],
            single: 101,
            double: 102,
        };
        let mut b = [0u8; INODE];
        n.encode(&mut b);
        assert_eq!((u16_at(&b, 0), u16_at(&b, 2), u32_at(&b, 4)), (1, 1, 3));
        assert_eq!((u64_at(&b, 8), u64_at(&b, 16), u64_at(&b, 24)), (0x0102_0304, 7, 9));
        assert_eq!((u32_at(&b, 32), u32_at(&b, 76), u32_at(&b, 80), u32_at(&b, 84)), (100, 100, 101, 102));
        let l = Layout::new(Geometry { block_count: 4096, inode_count: 64 }).unwrap();
        assert_eq!(Inode::parse(5, &b, &l), Ok(n));

        let h = Header { entries: [(40u32, [7u8; HASH])].into() };
        let hb = h.encode();
        assert_eq!((u32_at(&hb, 8), u32_at(&hb, 12), u32_at(&hb, 16)), (1, 1, 40));
        assert_eq!(&hb[20..52], &[7u8; HASH]);
        assert_eq!(Header::parse(&hb, &l), Ok(Some(h)));
    }
}
