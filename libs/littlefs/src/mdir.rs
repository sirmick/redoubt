//! Metadata pairs: parsing one block's log into what it currently says, and encoding commits.
//!
//! A metadata block is a revision count followed by a log of commits (SPEC.md "Directories /
//! Metadata pairs"). This module turns the log into [`Contents`] by replaying every tag of
//! every valid commit in order with [`Contents::apply`]. Writing uses the same function: a
//! commit is a list of tags, applied in memory to predict the result and appended to the log
//! (or, when the block is full, the predicted result is written out fresh: a compaction).
//! One function defines what a tag means, for reading and for writing.
//!
//! Everything here is pure: bytes in, values out. The medium is untrusted, so every offset
//! and length is checked before use and a malformed log is an error, never a panic.

use alloc::vec::Vec;

use crate::crc::crc32;
use crate::tag::{self, *};
use crate::Error;

/// A metadata pair: two blocks, one of them holding the current log.
pub(crate) type Pair = [u32; 2];

pub(crate) const PAIR_NULL: Pair = [BLOCK_NULL, BLOCK_NULL];

pub(crate) fn pair_is_null(p: &Pair) -> bool { p[0] == BLOCK_NULL || p[1] == BLOCK_NULL }

/// Two pairs refer to the same metadata if they share a block (either order).
pub(crate) fn pair_overlaps(a: &Pair, b: &Pair) -> bool {
    a[0] == b[0] || a[0] == b[1] || a[1] == b[0] || a[1] == b[1]
}

/// Two pairs are the same pair (either order).
pub(crate) fn pair_same(a: &Pair, b: &Pair) -> bool {
    (a[0] == b[0] && a[1] == b[1]) || (a[0] == b[1] && a[1] == b[0])
}

/// The largest number of entries one pair may hold: ids are 10 bits and 0x3ff is reserved.
pub(crate) const MAX_ENTRIES: usize = 0x3ff;

/// Receives a tag and its data.
pub(crate) type TagSink<'a> = dyn FnMut(u32, &[u8]) -> Result<(), Error> + 'a;
/// Reads bytes at an offset of the block being committed to.
pub(crate) type ReadAt<'a> = dyn FnMut(u32, &mut [u8]) -> Result<(), Error> + 'a;

pub(crate) fn le32(b: &[u8]) -> u32 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }

/// Global state (SPEC.md "0x7ff LFS_TYPE_MOVESTATE"): the XOR of one delta per metadata pair.
///
/// On disk: bit 31 of `tag` says "there may be orphans"; its type and id name a pending move
/// (type 0x4ff: delete `id` in `pair`). The length bits are never written; in memory, like
/// the C reference, they hold the orphan count.
///
/// SPEC.md is stale here: it calls bit 31 a "sync bit" meaning the list of pairs may be out
/// of sync, and says nothing of the length bits. This follows the reference's code (v2.11.3,
/// `lfs_fs_preporphans`, `lfs_gstate_hasorphans`), which is what its images contain.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct GState {
    pub tag: u32,
    pub pair: Pair,
}

impl GState {
    pub fn xor(&self, o: &GState) -> GState {
        GState { tag: self.tag ^ o.tag, pair: [self.pair[0] ^ o.pair[0], self.pair[1] ^ o.pair[1]] }
    }

    pub fn is_zero(&self) -> bool { *self == GState::default() }

    /// The on-disk part: the in-memory length bits cleared.
    pub fn without_size(&self) -> GState { GState { tag: self.tag & !0x3ff, pair: self.pair } }

    /// Reads a delta, zero-filling a short one as the reference does.
    pub fn from_bytes(data: &[u8]) -> GState {
        let mut b = [0u8; 12];
        let n = data.len().min(12);
        b[..n].copy_from_slice(&data[..n]);
        GState { tag: le32(&b[0..]), pair: [le32(&b[4..]), le32(&b[8..])] }
    }

    pub fn to_bytes(self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0..4].copy_from_slice(&self.tag.to_le_bytes());
        b[4..8].copy_from_slice(&self.pair[0].to_le_bytes());
        b[8..12].copy_from_slice(&self.pair[1].to_le_bytes());
        b
    }

    pub fn orphans(&self) -> u16 { tag::size(self.tag) }

    pub fn has_move(&self) -> bool { tag::type1(self.tag) != 0 }

    /// The id a pending move will delete from `pair`, if the move is in that pair.
    pub fn move_in(&self, pair: &Pair) -> Option<u16> {
        (self.has_move() && pair_overlaps(&self.pair, pair)).then(|| tag::id(self.tag))
    }
}

/// One file or directory in a pair: its name tag, its struct tag and its user attributes.
/// Struct data is kept raw; [`crate::fs`] decodes and validates it where it is used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The name tag's type3: 0x001 file, 0x002 directory, 0x0ff superblock. `UNNAMED` until a
    /// name tag arrives.
    pub name_type: u16,
    pub name: Vec<u8>,
    /// Struct tag type3 (0x200 directory, 0x201 inline, 0x202 CTZ) and its data.
    pub strct: Option<(u16, Vec<u8>)>,
    /// User attributes, sorted by type.
    pub attrs: Vec<(u8, Vec<u8>)>,
}

const UNNAMED: u16 = 0xffff;

impl Entry {
    fn unnamed() -> Entry { Entry { name_type: UNNAMED, name: Vec::new(), strct: None, attrs: Vec::new() } }

    pub fn attr(&self, typ: u8) -> Option<&[u8]> {
        self.attrs.iter().find(|a| a.0 == typ).map(|a| a.1.as_slice())
    }

    /// Calls `f` with each tag (and its data) that describes this entry at position `id`:
    /// what a compaction writes for it.
    pub fn tags(&self, id: u16, f: &mut TagSink) -> Result<(), Error> {
        f(tag::mk(self.name_type, id, len16(&self.name)?), &self.name)?;
        if let Some((t, d)) = &self.strct {
            f(tag::mk(*t, id, len16(d)?), d)?;
        }
        for (t, d) in &self.attrs {
            f(tag::mk(TYPE_USERATTR | *t as u16, id, len16(d)?), d)?;
        }
        Ok(())
    }
}

/// A tag's data length as its 10-bit field; longer data cannot be encoded.
pub(crate) fn len16(d: &[u8]) -> Result<u16, Error> {
    if d.len() > MAX_TAG_DATA { Err(Error::Invalid) } else { Ok(d.len() as u16) }
}

/// What a metadata block currently says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Contents {
    /// Entries in id order.
    pub entries: Vec<Entry>,
    /// The next pair in the list of pairs; `split` means it continues this directory
    /// (a hard tail) rather than only threading the list (a soft tail).
    pub tail: Pair,
    pub split: bool,
    /// This block's contribution to the global state (its latest move-state tag).
    pub gdelta: GState,
}

impl Contents {
    pub fn new(tail: Pair, split: bool) -> Contents {
        Contents { entries: Vec::new(), tail, split, gdelta: GState::default() }
    }

    fn entry_mut(&mut self, id: u16) -> Result<&mut Entry, Error> {
        let id = id as usize;
        if id >= MAX_ENTRIES {
            return Err(Error::Corrupt);
        }
        // A compacted log has no create tags: ids come into being by being named, in whatever
        // order the compaction wrote them.
        if id >= self.entries.len() {
            self.entries.resize(id + 1, Entry::unnamed());
        }
        Ok(&mut self.entries[id])
    }

    /// Applies one tag, as the log replay does. `data` is the tag's data (empty for a
    /// deleting tag). This is the single definition of what each tag means.
    pub fn apply(&mut self, t: u32, data: &[u8]) -> Result<(), Error> {
        let id = tag::id(t);
        match tag::type1(t) {
            T1_NAME => {
                if tag::is_delete(t) {
                    return Err(Error::Corrupt);
                }
                let e = self.entry_mut(id)?;
                e.name_type = tag::type3(t);
                e.name = data.to_vec();
            }
            T1_STRUCT => {
                // Any struct replaces any other struct of the same id.
                let e = self.entry_mut(id)?;
                e.strct = if tag::is_delete(t) { None } else { Some((tag::type3(t), data.to_vec())) };
            }
            T1_USERATTR => {
                let e = self.entry_mut(id)?;
                let typ = tag::chunk(t);
                match e.attrs.binary_search_by_key(&typ, |a| a.0) {
                    Ok(i) if tag::is_delete(t) => {
                        e.attrs.remove(i);
                    }
                    Ok(i) => e.attrs[i].1 = data.to_vec(),
                    Err(_) if tag::is_delete(t) => {}
                    Err(i) => e.attrs.insert(i, (typ, data.to_vec())),
                }
            }
            T1_SPLICE => match tag::type3(t) {
                TYPE_CREATE if (id as usize) <= self.entries.len() && self.entries.len() < MAX_ENTRIES => {
                    self.entries.insert(id as usize, Entry::unnamed());
                }
                TYPE_DELETE if (id as usize) < self.entries.len() => {
                    self.entries.remove(id as usize);
                }
                _ => return Err(Error::Corrupt),
            },
            T1_TAIL => {
                if data.len() != 8 {
                    return Err(Error::Corrupt);
                }
                self.tail = [le32(&data[0..]), le32(&data[4..])];
                self.split = tag::chunk(t) & 1 == 1;
            }
            T1_GSTATE if tag::type3(t) == TYPE_MOVESTATE => self.gdelta = GState::from_bytes(data),
            // Type 0x1 is never on disk and CRC tags (0x5) are consumed by the parser.
            _ => {}
        }
        Ok(())
    }

    /// Every id must have been named: a log that creates an entry and never names it is not
    /// something littlefs writes.
    /// Nor two files or directories of the same name in one pair: which one a lookup finds
    /// would depend on the implementation.
    pub fn check(&self) -> Result<(), Error> {
        if self.entries.iter().any(|e| e.name_type == UNNAMED) {
            return Err(Error::Corrupt);
        }
        let mut names: Vec<&[u8]> =
            self.entries.iter().filter(|e| is_file_or_dir(e.name_type)).map(|e| e.name.as_slice()).collect();
        names.sort_unstable();
        if names.windows(2).any(|w| w[0] == w[1]) { Err(Error::Corrupt) } else { Ok(()) }
    }

    /// Bytes the entries take in a compacted block.
    pub fn entries_size(entries: &[Entry]) -> Result<u32, Error> {
        let mut size = 0;
        for (id, e) in entries.iter().enumerate() {
            e.tags(id as u16, &mut |t, _| {
                size += tag::dsize(t);
                Ok(())
            })?;
        }
        Ok(size)
    }
}

/// The result of parsing one block of a pair.
pub(crate) struct Parsed {
    /// Where the last valid commit ends: the next commit goes here.
    pub off: u32,
    /// The tag state after the last commit, which the next commit's first tag is XORed with.
    pub etag: u32,
    /// Whether the rest of the block is known to be erased, so a commit can be appended.
    pub erased: bool,
    pub contents: Contents,
}

/// A commit that passed its CRC: where it ends and how much of the tag list it covers.
struct Committed {
    off: u32,
    etag: u32,
    ntags: usize,
    fcrc: Option<(u32, u32)>,
}

/// Parses a whole metadata block. `Ok(None)`: the block holds no valid commit (never written,
/// or its first commit was torn). `Err(Corrupt)`: CRC-valid commits that do not make sense.
pub(crate) fn parse_block(data: &[u8], prog_size: u32) -> Result<Option<Parsed>, Error> {
    let bs = data.len() as u32;
    if bs < 8 {
        return Err(Error::Corrupt);
    }
    // The revision count is covered by the first commit's CRC.
    let mut crc = crc32(0xffff_ffff, &data[0..4]);
    let mut off: u32 = 0;
    let mut ptag: u32 = 0xffff_ffff;
    let mut tags: Vec<(u32, u32)> = Vec::new(); // (tag, offset of its data)
    let mut fcrc = None;
    let mut last: Option<Committed> = None;
    let mut maybe_erased = false;

    // Each step moves `off` forward by at least 4 bytes, so this ends within bs/4 steps.
    loop {
        off += tag::dsize(ptag);
        if off + 4 > bs {
            break;
        }
        let raw = &data[off as usize..off as usize + 4];
        crc = crc32(crc, raw);
        let t = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) ^ ptag;
        if !tag::is_valid(t) {
            // Unwritten space reads as invalid. If it directly follows a commit, what comes
            // next may be erased (the forward CRC decides).
            maybe_erased = tag::is_commit_crc(ptag);
            break;
        }
        let end = off + tag::dsize(t);
        if end > bs {
            break;
        }
        ptag = t;
        let d = &data[off as usize + 4..end as usize];

        if tag::is_commit_crc(t) {
            if d.len() < 4 || crc != le32(d) {
                break;
            }
            // The CRC tag's lowest chunk bit flips the valid bit expected of the next commit,
            // so that erased storage after this commit can never parse as a valid tag.
            ptag ^= ((tag::chunk(t) & 1) as u32) << 31;
            last = Some(Committed { off: end, etag: ptag, ntags: tags.len(), fcrc });
            fcrc = None;
            crc = 0xffff_ffff;
            continue;
        }

        crc = crc32(crc, d);
        if tag::type3(t) == TYPE_FCRC && d.len() >= 8 {
            fcrc = Some((le32(&d[0..]), le32(&d[4..])));
        }
        tags.push((t, off + 4));
    }

    let Some(c) = last else { return Ok(None) };
    let mut contents = Contents::new(crate::mdir::PAIR_NULL, false);
    for &(t, doff) in &tags[..c.ntags] {
        let dlen = tag::dsize(t) - 4;
        contents.apply(t, &data[doff as usize..(doff + dlen) as usize])?;
    }
    contents.check()?;

    // Appending is safe only if the space after the last commit is still exactly as it was
    // when that commit's forward CRC was taken: an interrupted program would have changed it.
    let erased = maybe_erased
        && c.off % prog_size == 0
        && match c.fcrc {
            Some((size, want)) => {
                let end = c.off as u64 + size as u64;
                end <= bs as u64 && crc32(0xffff_ffff, &data[c.off as usize..end as usize]) == want
            }
            None => false,
        };
    Ok(Some(Parsed { off: c.off, etag: c.etag, erased, contents }))
}

/// A commit being built in memory before it is programmed in one go.
///
/// Commits always start on a program boundary, so the buffer starts at `start` (aligned) and
/// `finish` pads it to the next boundary.
pub(crate) struct CommitBuf {
    pub start: u32,
    pub buf: Vec<u8>,
    ptag: u32,
    crc: u32,
    /// Tags may not reach past this offset, leaving room for the CRC tag.
    limit: u32,
}

impl CommitBuf {
    pub fn new(start: u32, ptag: u32, limit: u32) -> CommitBuf {
        CommitBuf { start, buf: Vec::new(), ptag, crc: 0xffff_ffff, limit }
    }

    fn off(&self) -> u32 { self.start + self.buf.len() as u32 }

    /// Appends checksummed bytes that are not a tag (the revision count).
    pub fn push_raw(&mut self, bytes: &[u8]) {
        self.crc = crc32(self.crc, bytes);
        self.buf.extend_from_slice(bytes);
    }

    /// Appends a tag and its data. `NoSpace` if it does not fit in the block.
    pub fn push_tag(&mut self, t: u32, data: &[u8]) -> Result<(), Error> {
        if tag::dsize(t) as usize != 4 + data.len() {
            return Err(Error::Invalid);
        }
        if self.off() as u64 + tag::dsize(t) as u64 > self.limit as u64 {
            return Err(Error::NoSpace);
        }
        let t = t & 0x7fff_ffff;
        self.push_raw(&(t ^ self.ptag).to_be_bytes());
        self.push_raw(data);
        self.ptag = t;
        Ok(())
    }

    /// Ends the commit with its CRC and pads it to a program boundary (the reference's
    /// `lfs_dir_commitcrc`). In the middle of a block a forward CRC goes first: a checksum of
    /// the next `prog_size` bytes, which `read_next` reads from the (erased) block, so a later
    /// mount can tell whether an append was attempted there. Those bytes also choose the
    /// valid-bit polarity of the next commit, so a partial program always flips something.
    pub fn finish(
        &mut self,
        block_size: u32,
        prog_size: u32,
        read_next: &mut ReadAt,
    ) -> Result<(), Error> {
        // Room for the forward CRC tag (4 + 8 bytes) and the CRC tag (4 + 4), padded to a
        // program unit.
        let end = align_up((self.off() + 20).min(block_size), prog_size);
        while self.off() < end {
            // A CRC tag carries at most 0x3fe bytes, so a long pad needs several commits.
            let mut noff = (end - (self.off() + 4)).min(MAX_TAG_DATA as u32) + self.off() + 4;
            if noff < end {
                noff = noff.min(end - 20);
            }

            let mut eperturb: u8 = 0xff;
            if noff >= end && noff <= block_size - prog_size {
                let mut next = alloc::vec![0u8; prog_size as usize];
                read_next(noff, &mut next)?;
                eperturb = next[0];
                let mut fcrc = [0u8; 8];
                fcrc[0..4].copy_from_slice(&prog_size.to_le_bytes());
                fcrc[4..8].copy_from_slice(&crc32(0xffff_ffff, &next).to_le_bytes());
                self.limit = block_size;
                self.push_tag(tag::mk(TYPE_FCRC, ID_NONE, 8), &fcrc)?;
            }

            let ntag = tag::mk(TYPE_CCRC + ((!eperturb) >> 7) as u16, ID_NONE, (noff - (self.off() + 4)) as u16);
            let raw = (ntag ^ self.ptag).to_be_bytes();
            self.crc = crc32(self.crc, &raw);
            self.buf.extend_from_slice(&raw);
            self.buf.extend_from_slice(&self.crc.to_le_bytes());
            // Padding is not checksummed; it is programmed as erased-looking bytes.
            self.buf.resize((noff - self.start) as usize, 0xff);
            self.ptag = ntag ^ (((0x80 & !eperturb) as u32) << 24);
            self.crc = 0xffff_ffff;
        }
        Ok(())
    }
}

pub(crate) fn align_up(x: u32, a: u32) -> u32 { x.div_ceil(a) * a }
