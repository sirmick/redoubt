//! The volume: reading checked blocks, building and committing transactions through the log,
//! recovery, the bitmap, block maps, and the orphan list.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::file::Open;
use crate::layout::{
    DOUBLE_BASE, HASH_SLOTS, Header, INODE_START, INODES_PER_BLOCK, Inode, KIND_DIR, KIND_FREE, LOG,
    LOG_START, Layout, SINGLE_BASE, seal, self_hash_ok, sha, u32_at,
};
use crate::{ATTRS, BLOCK, Block, BlockDevice, Error, Geometry, HASH, INODE, LOG_BLOCKS, PER_INDIRECT, ROOT};

/// Room a step that frees one block may need: the bitmap's block, the indirect block that held it,
/// the double-indirect block and the inode, each with its hash block.
const FREE_STEP: usize = 8;

/// The transaction being built: each block it writes, as it will be, in the order first touched,
/// and each bitmap bit it changed with the bit's old value, to undo.
#[derive(Default)]
pub(crate) struct Tx {
    blocks: Vec<(u32, Box<Block>)>,
    bits: Vec<(u32, bool)>,
}

pub struct Filesystem<D: BlockDevice> {
    pub(crate) dev: D,
    pub(crate) l: Layout,
    /// The bitmap region's bytes, as of the transaction being built.
    bitmap: Vec<u8>,
    pub(crate) tx: Tx,
    /// The last hash block read from the device.
    hashes: Option<(u32, Box<Block>)>,
    pub(crate) handles: Vec<Option<Open>>,
    poisoned: bool,
    pub(crate) now: u64,
    /// Data blocks free, as of the transaction being built.
    free: u32,
    alloc_from: u32,
    inode_from: u32,
}

fn zeroed() -> Box<Block> { Box::new([0u8; BLOCK]) }

impl<D: BlockDevice> Filesystem<D> {
    /// Writes an empty volume of geometry `g` to `dev`: the superblock, an empty log, the root
    /// directory, the bitmap and every hash a table, the superblock or the bitmap needs.
    pub fn format(mut dev: D, g: Geometry) -> Result<(), Error> {
        let l = Layout::new(g).ok_or(Error::Invalid)?;
        if dev.block_count() != g.block_count {
            return Err(Error::Invalid);
        }
        let sb = l.superblock();
        let mut first = [0u8; BLOCK];
        let root = Inode { kind: KIND_DIR, nlink: 1, generation: 1, ..Inode::default() };
        root.encode(&mut first[ROOT as usize * INODE..]);
        let bitmap = |k: u32| -> Block {
            let mut b = [0u8; BLOCK];
            for (i, byte) in b.iter_mut().enumerate() {
                for bit in 0..8 {
                    let block = (k * BLOCK as u32 + i as u32) as u64 * 8 + bit;
                    if block < l.data_start as u64 || block >= l.block_count as u64 {
                        *byte |= 1 << bit;
                    }
                }
            }
            b
        };
        let (zero, first_hash) = (sha(&[0u8; BLOCK]), sha(&first));
        let bitmap_hashes: Vec<[u8; HASH]> = (0..l.bitmap_blocks()).map(|k| sha(&bitmap(k))).collect();
        dev.write(0, &sb)?;
        dev.write(LOG_START, &Header::default().encode())?;
        for b in INODE_START..l.hash_start {
            dev.write(b, if b == INODE_START { &first } else { &[0u8; BLOCK] })?;
        }
        for k in 0..l.bitmap_blocks() {
            dev.write(l.bitmap_start + k, &bitmap(k))?;
        }
        for hb in l.hash_start..l.bitmap_start {
            let mut slots = [0u8; BLOCK];
            // Slot i is block i's for the superblock, block i + the log's length for the rest.
            let first = (hb - l.hash_start) * HASH_SLOTS;
            let slots_in_volume = l.block_count - LOG;
            for i in first..slots_in_volume.min(first + HASH_SLOTS) {
                let b = if i == 0 { 0 } else { i + LOG };
                let h = match b {
                    0 => sha(&sb),
                    INODE_START => first_hash,
                    _ if b > INODE_START && b < l.hash_start => zero,
                    _ if b >= l.bitmap_start && b < l.data_start => {
                        bitmap_hashes[(b - l.bitmap_start) as usize]
                    }
                    _ => continue,
                };
                let at = l.slot(b).1;
                slots[at..at + HASH].copy_from_slice(&h);
            }
            seal(&mut slots);
            dev.write(hb, &slots)?;
        }
        dev.sync()
    }

    /// Mounts the volume on `dev`: the superblock checked, a committed transaction in the log copied
    /// home, the bitmap read, and the orphan list finished.
    pub fn mount(mut dev: D) -> Result<Self, Error> {
        let mut sb = zeroed();
        dev.read(0, &mut sb)?;
        let l = Layout::parse(&sb, dev.block_count())?;
        let mut fs = Filesystem {
            dev,
            l,
            bitmap: Vec::new(),
            tx: Tx::default(),
            hashes: None,
            handles: Vec::new(),
            poisoned: false,
            now: 0,
            free: 0,
            alloc_from: l.data_start,
            inode_from: ROOT + 1,
        };
        fs.recover()?;
        fs.block(0)?;
        for k in 0..l.bitmap_blocks() {
            let b = fs.block(l.bitmap_start + k)?;
            fs.bitmap.extend_from_slice(&b[..]);
        }
        let bits = l.bitmap_blocks() as u64 * BLOCK as u64 * 8;
        if (0..l.data_start as u64).chain(l.block_count as u64..bits).any(|b| !fs.bit(b as u32)) {
            return Err(Error::Corrupt);
        }
        fs.free = (l.data_start..l.block_count).filter(|&b| !fs.bit(b)).count() as u32;
        let root = fs.inode(ROOT)?;
        if root.kind != KIND_DIR || root.nlink != 1 {
            return Err(Error::Corrupt);
        }
        // Each pass takes the list's head off it, or fails; a list longer than the inodes is a
        // cycle.
        for _ in 0..l.inode_count {
            let head = fs.inode(0)?.next;
            if head == 0 {
                break;
            }
            if fs.inode(head)?.kind == KIND_FREE {
                return Err(Error::Corrupt);
            }
            fs.finish(head)?;
            fs.commit()?;
        }
        if fs.inode(0)?.next != 0 {
            return Err(Error::Corrupt);
        }
        Ok(fs)
    }

    /// The time every later change records as its mtime, in microseconds since the Unix epoch.
    pub fn set_time(&mut self, micros: u64) { self.now = micros }

    /// Data blocks free.
    pub fn free_blocks(&self) -> u32 { self.free }

    /// Data blocks in all: the data region's.
    pub fn data_blocks(&self) -> u32 { self.l.block_count - self.l.data_start }

    /// Inodes in all, inode 0 and the root among them.
    pub fn inode_count(&self) -> u32 { self.l.inode_count }

    /// Runs one operation: refused once poisoned; what it left in the transaction committed if it
    /// succeeded, dropped if not; an I/O error poisons.
    pub(crate) fn op<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        match f(self).and_then(|v| self.commit().map(|()| v)) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.abort();
                if e == Error::Io {
                    self.poisoned = true;
                }
                Err(e)
            }
        }
    }

    fn io<T>(&mut self, r: Result<T, Error>) -> Result<T, Error> {
        if r.is_err() {
            self.poisoned = true;
            return Err(Error::Io);
        }
        r
    }

    fn dev_read(&mut self, b: u32, buf: &mut Block) -> Result<(), Error> {
        let r = self.dev.read(b, buf);
        self.io(r)
    }

    fn dev_write(&mut self, b: u32, buf: &Block) -> Result<(), Error> {
        let r = self.dev.write(b, buf);
        self.io(r)
    }

    fn dev_sync(&mut self) -> Result<(), Error> {
        let r = self.dev.sync();
        self.io(r)
    }

    /// The log at mount: a torn header is no transaction; a committed one is checked whole, then
    /// copied home and the header emptied.
    fn recover(&mut self) -> Result<(), Error> {
        let mut b = zeroed();
        self.dev_read(LOG_START, &mut b)?;
        let entries = match Header::parse(&b, &self.l)? {
            Some(h) if h.entries.is_empty() => return Ok(()),
            Some(h) => h.entries,
            None => Vec::new(),
        };
        for (k, (_, hash)) in entries.iter().enumerate() {
            self.dev_read(LOG_START + 1 + k as u32, &mut b)?;
            if sha(&b[..]) != *hash {
                return Err(Error::Corrupt);
            }
        }
        for (k, (home, hash)) in entries.iter().enumerate() {
            self.dev_read(LOG_START + 1 + k as u32, &mut b)?;
            if sha(&b[..]) != *hash {
                return Err(Error::Corrupt);
            }
            self.dev_write(*home, &b)?;
        }
        self.dev_sync()?;
        self.dev_write(LOG_START, &Header::default().encode())?;
        self.dev_sync()
    }

    /// Block `b`'s slot in the hash region, from the transaction, the last hash block read, or the
    /// device.
    fn slot_of(&mut self, b: u32) -> Result<[u8; HASH], Error> {
        let (hb, at) = self.l.slot(b);
        let mut out = [0u8; HASH];
        if let Some((_, buf)) = self.tx.blocks.iter().find(|e| e.0 == hb) {
            out.copy_from_slice(&buf[at..at + HASH]);
            return Ok(out);
        }
        if self.hashes.as_ref().is_none_or(|h| h.0 != hb) {
            let buf = self.hash_block(hb)?;
            self.hashes = Some((hb, buf));
        }
        let buf = &self.hashes.as_ref().unwrap().1;
        out.copy_from_slice(&buf[at..at + HASH]);
        Ok(out)
    }

    /// Hash block `hb` from the device, checked against its own hash.
    pub(crate) fn hash_block(&mut self, hb: u32) -> Result<Box<Block>, Error> {
        let mut buf = zeroed();
        self.dev_read(hb, &mut buf)?;
        if !self_hash_ok(&buf) {
            return Err(Error::Corrupt);
        }
        Ok(buf)
    }

    /// Block `b` as the transaction leaves it, or from the device checked against its slot.
    pub(crate) fn block(&mut self, b: u32) -> Result<Box<Block>, Error> {
        if let Some((_, buf)) = self.tx.blocks.iter().find(|e| e.0 == b) {
            return Ok(buf.clone());
        }
        if !self.l.is_hashed(b) {
            return Err(Error::Corrupt);
        }
        let mut buf = zeroed();
        self.dev_read(b, &mut buf)?;
        if sha(&buf[..]) != self.slot_of(b)? {
            return Err(Error::Corrupt);
        }
        Ok(buf)
    }

    /// Blocks the transaction can still take.
    pub(crate) fn room(&self) -> usize { LOG_BLOCKS - self.tx.blocks.len() }

    /// Block `b` in the transaction, with its hash block, as it is (`fresh` false) or zeroed.
    fn tx_block(&mut self, b: u32, fresh: bool) -> Result<&mut Block, Error> {
        if let Some(k) = self.tx.blocks.iter().position(|e| e.0 == b) {
            if fresh {
                self.tx.blocks[k].1.fill(0);
            }
            return Ok(&mut self.tx.blocks[k].1);
        }
        if !self.l.is_home(b) {
            return Err(Error::Corrupt);
        }
        let hashed = self.l.is_hashed(b);
        let hb = self.l.slot(b).0;
        let with_hash = hashed && !self.tx.blocks.iter().any(|e| e.0 == hb);
        // Every operation fits its transaction by construction; this is the guard, not a limit.
        if self.room() < 1 + with_hash as usize {
            return Err(Error::Invalid);
        }
        let buf = match (fresh, hashed) {
            (true, _) => zeroed(),
            (false, true) => self.block(b)?,
            (false, false) => self.hash_block(b)?,
        };
        if with_hash {
            let slots = self.hash_block(hb)?;
            self.tx.blocks.push((hb, slots));
        }
        self.tx.blocks.push((b, buf));
        Ok(&mut self.tx.blocks.last_mut().unwrap().1)
    }

    /// Block `b` in the transaction, to change.
    pub(crate) fn tx_get(&mut self, b: u32) -> Result<&mut Block, Error> { self.tx_block(b, false) }

    /// Block `b` in the transaction, zeroed: a block just allocated, or one overwritten whole.
    pub(crate) fn tx_new(&mut self, b: u32) -> Result<&mut Block, Error> { self.tx_block(b, true) }

    /// Commits the transaction: the page's four steps, each ended by a `sync`.
    pub(crate) fn commit(&mut self) -> Result<(), Error> {
        if !self.tx.blocks.is_empty() {
            let blocks = core::mem::take(&mut self.tx.blocks);
            let r = self.write_through(blocks);
            self.hashes = None;
            r?;
        }
        self.tx.bits.clear();
        Ok(())
    }

    fn write_through(&mut self, mut blocks: Vec<(u32, Box<Block>)>) -> Result<(), Error> {
        let slots: Vec<(u32, usize, [u8; HASH])> = blocks
            .iter()
            .filter(|e| self.l.is_hashed(e.0))
            .map(|e| {
                let (hb, at) = self.l.slot(e.0);
                (hb, at, sha(&e.1[..]))
            })
            .collect();
        for (hb, at, h) in slots {
            let Some(e) = blocks.iter_mut().find(|e| e.0 == hb) else { return Err(Error::Invalid) };
            e.1[at..at + HASH].copy_from_slice(&h);
        }
        for e in blocks.iter_mut().filter(|e| self.l.is_hash(e.0)) {
            seal(&mut e.1);
        }
        let header = Header { entries: blocks.iter().map(|e| (e.0, sha(&e.1[..]))).collect() };
        for (k, (_, buf)) in blocks.iter().enumerate() {
            self.dev_write(LOG_START + 1 + k as u32, buf)?;
        }
        self.dev_sync()?;
        self.dev_write(LOG_START, &header.encode())?;
        self.dev_sync()?;
        for (home, buf) in &blocks {
            self.dev_write(*home, buf)?;
        }
        self.dev_sync()?;
        self.dev_write(LOG_START, &Header::default().encode())?;
        self.dev_sync()
    }

    /// Drops the transaction, and the bitmap's changes with it.
    pub(crate) fn abort(&mut self) {
        for (b, old) in core::mem::take(&mut self.tx.bits).into_iter().rev() {
            self.flip(b, old);
        }
        self.tx.blocks.clear();
    }

    pub(crate) fn bit(&self, b: u32) -> bool { self.bitmap[(b / 8) as usize] & (1 << (b % 8)) != 0 }

    /// Sets data block `b`'s bit in memory, and counts it.
    fn flip(&mut self, b: u32, used: bool) {
        if self.bit(b) != used {
            let byte = &mut self.bitmap[(b / 8) as usize];
            *byte ^= 1 << (b % 8);
            if used { self.free -= 1 } else { self.free += 1 }
        }
    }

    fn set_bit(&mut self, b: u32, used: bool) -> Result<(), Error> {
        let (blk, at, mask) = self.l.bit_at(b);
        let buf = self.tx_get(blk)?;
        if used {
            buf[at] |= mask
        } else {
            buf[at] &= !mask
        }
        self.tx.bits.push((b, self.bit(b)));
        self.flip(b, used);
        Ok(())
    }

    /// A free data block, marked in use and zeroed in the transaction.
    pub(crate) fn alloc(&mut self) -> Result<u32, Error> {
        let (start, end) = (self.l.data_start, self.l.block_count);
        let mut b = self.alloc_from.clamp(start, end - 1);
        let mut seen = 0;
        while seen < end - start {
            if b.is_multiple_of(8) && end - b >= 8 && self.bitmap[(b / 8) as usize] == 0xff {
                (b, seen) = (b + 8, seen + 8);
            } else if self.bit(b) {
                (b, seen) = (b + 1, seen + 1);
            } else {
                self.set_bit(b, true)?;
                self.tx_new(b)?;
                self.alloc_from = b + 1;
                return Ok(b);
            }
            if b >= end {
                b = start;
            }
        }
        Err(Error::NoSpace)
    }

    /// Frees data block `b`; one already free, or outside the data region, is corrupt.
    pub(crate) fn release(&mut self, b: u32) -> Result<(), Error> {
        if !self.l.is_data(b) || !self.bit(b) {
            return Err(Error::Corrupt);
        }
        self.set_bit(b, false)
    }

    pub(crate) fn inode(&mut self, i: u32) -> Result<Inode, Error> {
        if i >= self.l.inode_count {
            return Err(Error::Corrupt);
        }
        let (blk, at) = self.l.inode_at(i);
        let buf = self.block(blk)?;
        Inode::parse(i, &buf[at..at + INODE], &self.l)
    }

    pub(crate) fn put_inode(&mut self, i: u32, n: &Inode) -> Result<(), Error> {
        let (blk, at) = self.l.inode_at(i);
        n.encode(&mut self.tx_get(blk)?[at..at + INODE]);
        Ok(())
    }

    /// A free inode made `kind`, its generation counted, its attribute area cleared.
    pub(crate) fn alloc_inode(&mut self, kind: u16) -> Result<(u32, Inode), Error> {
        let count = self.l.inode_count;
        let mut i = self.inode_from;
        for _ in 0..count {
            if i >= count {
                i = ROOT + 1;
            }
            let (blk, _) = self.l.inode_at(i);
            let buf = self.block(blk)?;
            let last = (blk - INODE_START + 1) * INODES_PER_BLOCK;
            while i < last.min(count) {
                let at = (i % INODES_PER_BLOCK) as usize * INODE;
                let n = Inode::parse(i, &buf[at..at + INODE], &self.l)?;
                if n.kind == KIND_FREE {
                    let n = Inode {
                        kind,
                        nlink: 1,
                        generation: n.generation.wrapping_add(1),
                        mtime: self.now,
                        ..Inode::default()
                    };
                    self.put_inode(i, &n)?;
                    let (ablk, aat) = self.l.attrs_at(i);
                    self.tx_get(ablk)?[aat..aat + ATTRS].fill(0);
                    self.inode_from = i + 1;
                    return Ok((i, n));
                }
                i += 1;
            }
        }
        Err(Error::NoSpace)
    }

    /// Address `k` of indirect block `blk`: a hole, or in the data region.
    fn entry(&mut self, blk: u32, k: u64) -> Result<u32, Error> {
        let a = u32_at(&self.block(blk)?[..], 4 * k as usize);
        if a != 0 && !self.l.is_data(a) {
            return Err(Error::Corrupt);
        }
        Ok(a)
    }

    fn set_entry(&mut self, blk: u32, k: u64, a: u32) -> Result<(), Error> {
        crate::layout::put_u32(self.tx_get(blk)?, 4 * k as usize, a);
        Ok(())
    }

    /// The address of file block `idx` of `n`, 0 for a hole.
    pub(crate) fn bmap(&mut self, n: &Inode, idx: u64) -> Result<u32, Error> {
        if idx < SINGLE_BASE {
            return Ok(n.direct[idx as usize]);
        }
        if idx < DOUBLE_BASE {
            return if n.single == 0 { Ok(0) } else { self.entry(n.single, idx - SINGLE_BASE) };
        }
        let idx = idx - DOUBLE_BASE;
        if n.double == 0 || idx >= PER_INDIRECT * PER_INDIRECT {
            return Ok(0);
        }
        let s = self.entry(n.double, idx / PER_INDIRECT)?;
        if s == 0 { Ok(0) } else { self.entry(s, idx % PER_INDIRECT) }
    }

    /// The blocks [`Self::bmap_alloc`] would allocate for file block `idx` of `n`: the data block and
    /// the indirect blocks on its way that are holes.
    pub(crate) fn alloc_need(&mut self, n: &Inode, idx: u64) -> Result<u32, Error> {
        if idx < SINGLE_BASE {
            return Ok((n.direct[idx as usize] == 0) as u32);
        }
        if idx < DOUBLE_BASE {
            return Ok(if n.single == 0 {
                2
            } else {
                (self.entry(n.single, idx - SINGLE_BASE)? == 0) as u32
            });
        }
        let idx = idx - DOUBLE_BASE;
        if n.double == 0 {
            return Ok(3);
        }
        Ok(match self.entry(n.double, idx / PER_INDIRECT)? {
            0 => 2,
            s => (self.entry(s, idx % PER_INDIRECT)? == 0) as u32,
        })
    }

    /// Indirect `*ptr`, allocated if it is a hole.
    fn indirect(&mut self, ptr: &mut u32) -> Result<u32, Error> {
        if *ptr == 0 {
            *ptr = self.alloc()?;
        }
        Ok(*ptr)
    }

    /// Address `k` of indirect block `blk`, allocated if it is a hole; and whether it was.
    fn entry_alloc(&mut self, blk: u32, k: u64) -> Result<(u32, bool), Error> {
        match self.entry(blk, k)? {
            0 => {
                let a = self.alloc()?;
                self.set_entry(blk, k, a)?;
                Ok((a, true))
            }
            a => Ok((a, false)),
        }
    }

    /// The address of file block `idx` of `n`, allocated (zeroed, in the transaction) if it is a
    /// hole; and whether it was.
    pub(crate) fn bmap_alloc(&mut self, n: &mut Inode, idx: u64) -> Result<(u32, bool), Error> {
        if idx < SINGLE_BASE {
            let d = &mut n.direct[idx as usize];
            if *d != 0 {
                return Ok((*d, false));
            }
            let a = self.alloc()?;
            n.direct[idx as usize] = a;
            return Ok((a, true));
        }
        if idx < DOUBLE_BASE {
            let s = self.indirect(&mut n.single)?;
            return self.entry_alloc(s, idx - SINGLE_BASE);
        }
        let idx = idx - DOUBLE_BASE;
        let d = self.indirect(&mut n.double)?;
        let (s, _) = self.entry_alloc(d, idx / PER_INDIRECT)?;
        self.entry_alloc(s, idx % PER_INDIRECT)
    }

    /// Commits now if the transaction cannot take another step of `need` blocks, with inode `i`
    /// written first.
    pub(crate) fn make_room(&mut self, i: u32, n: &Inode, need: usize) -> Result<(), Error> {
        if self.room() < need {
            self.put_inode(i, n)?;
            self.commit()?;
        }
        Ok(())
    }

    /// Frees the file blocks of inode `i` from `keep` on, and the indirect blocks that then map
    /// none, last first; committing as the transaction fills, so each commit leaves every freed
    /// block unreferenced.
    fn free_from(&mut self, i: u32, n: &mut Inode, keep: u64) -> Result<(), Error> {
        if n.double != 0 {
            let d = n.double;
            let top = self.block(d)?;
            for k in (0..PER_INDIRECT).rev() {
                let base = DOUBLE_BASE + k * PER_INDIRECT;
                if base + PER_INDIRECT <= keep {
                    break;
                }
                let s = u32_at(&top[..], 4 * k as usize);
                if s == 0 {
                    continue;
                }
                if !self.l.is_data(s) {
                    return Err(Error::Corrupt);
                }
                self.free_entries(i, n, s, base, keep)?;
                if base >= keep {
                    self.make_room(i, n, FREE_STEP)?;
                    self.release(s)?;
                    self.set_entry(d, k, 0)?;
                }
            }
            if DOUBLE_BASE >= keep {
                self.make_room(i, n, FREE_STEP)?;
                self.release(d)?;
                n.double = 0;
            }
        }
        if n.single != 0 {
            let s = n.single;
            self.free_entries(i, n, s, SINGLE_BASE, keep)?;
            if SINGLE_BASE >= keep {
                self.make_room(i, n, FREE_STEP)?;
                self.release(s)?;
                n.single = 0;
            }
        }
        for k in (keep.min(SINGLE_BASE)..SINGLE_BASE).rev() {
            let a = n.direct[k as usize];
            if a != 0 {
                self.make_room(i, n, FREE_STEP)?;
                self.release(a)?;
                n.direct[k as usize] = 0;
            }
        }
        self.put_inode(i, n)
    }

    /// Frees the blocks indirect block `blk` maps from file block `keep` on; its first maps file
    /// block `base`.
    fn free_entries(&mut self, i: u32, n: &Inode, blk: u32, base: u64, keep: u64) -> Result<(), Error> {
        let ents = self.block(blk)?;
        for k in (0..PER_INDIRECT).rev() {
            if base + k < keep {
                break;
            }
            let a = u32_at(&ents[..], 4 * k as usize);
            if a != 0 {
                self.make_room(i, n, FREE_STEP)?;
                self.release(a)?;
                self.set_entry(blk, k, 0)?;
            }
        }
        Ok(())
    }

    /// Puts inode `i` on the orphan list.
    pub(crate) fn orphan_link(&mut self, i: u32) -> Result<(), Error> {
        let (mut head, mut n) = (self.inode(0)?, self.inode(i)?);
        n.next = head.next;
        head.next = i;
        self.put_inode(i, &n)?;
        self.put_inode(0, &head)
    }

    /// Takes inode `i` off the orphan list.
    fn orphan_unlink(&mut self, i: u32) -> Result<(), Error> {
        let mut n = self.inode(i)?;
        let mut p = 0;
        for _ in 0..=self.l.inode_count {
            let mut pn = self.inode(p)?;
            if pn.next == i {
                pn.next = n.next;
                self.put_inode(p, &pn)?;
                n = self.inode(i)?;
                n.next = 0;
                return self.put_inode(i, &n);
            }
            if pn.next == 0 {
                break;
            }
            p = pn.next;
        }
        Err(Error::Corrupt)
    }

    /// Finishes orphan `i`: a removed inode's blocks and then the inode freed, a truncated one's
    /// blocks past its size; then off the list.
    pub(crate) fn finish(&mut self, i: u32) -> Result<(), Error> {
        let mut n = self.inode(i)?;
        let keep = if n.nlink == 0 { 0 } else { n.size.div_ceil(BLOCK as u64) };
        self.free_from(i, &mut n, keep)?;
        self.make_room(i, &n, FREE_STEP)?;
        self.orphan_unlink(i)?;
        if n.nlink == 0 {
            self.put_inode(i, &Inode { generation: n.generation, ..Inode::default() })?;
            let (ablk, aat) = self.l.attrs_at(i);
            self.tx_get(ablk)?[aat..aat + ATTRS].fill(0);
        }
        Ok(())
    }

    /// Inode `i` cut to its size: its blocks past it freed, through the orphan list unless it is
    /// already on it.
    pub(crate) fn trim(&mut self, i: u32) -> Result<(), Error> {
        let mut n = self.inode(i)?;
        if n.nlink == 0 {
            let keep = n.size.div_ceil(BLOCK as u64);
            return self.free_from(i, &mut n, keep);
        }
        self.orphan_link(i)?;
        self.finish(i)
    }

    pub(crate) fn is_open(&self, i: u32) -> bool { self.handles.iter().flatten().any(|o| o.ino == i) }
}
