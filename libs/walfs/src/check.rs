//! The volume check: every directory from the root, every file's blocks read against their
//! hashes, the orphan list, every inode and attribute area, and the bitmap against what is in
//! use; each finding named.

use alloc::vec;
use alloc::vec::Vec;

use crate::fs::Filesystem;
use crate::layout::{
    DIRENTS_PER_BLOCK, DOUBLE_BASE, Inode, KIND_DIR, KIND_FREE, SINGLE_BASE, dirent, parse_attrs, u32_at,
};
use crate::{ATTRS, BLOCK, BlockDevice, Error, PER_INDIRECT, ROOT};

/// What [`Filesystem::check`] finds. A volume that mounts and has none of these is sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// A block the walk reads fails its hash or does not parse: a hash block, a table's, a
    /// directory's, an indirect one's or a file's data; or an indirect block names a block outside
    /// the data region.
    BadBlock(u32),
    /// An inode that does not parse.
    BadInode(u32),
    /// An inode's attribute area that does not parse.
    BadAttrs(u32),
    /// A block more than one reference names.
    SharedBlock(u32),
    /// A block in use whose bit says free.
    UsedButFree(u32),
    /// A data block whose bit says in use that nothing uses.
    LeakedBlock(u32),
    /// An inode more than one directory entry names.
    NamedTwice(u32),
    /// A directory entry naming a free or removed inode.
    NamedFree(u32),
    /// An allocated inode that no entry names and the orphan list does not hold.
    LeakedInode(u32),
    /// A name twice in one directory: the directory's inode.
    DuplicateName(u32),
    /// A block mapped past the inode's size.
    PastSize(u32),
    /// A directory's block that is a hole.
    DirectoryHole(u32),
    /// Bytes past a file's size in its last block that are not zero.
    TailNotZero(u32),
    /// The orphan list runs through a free inode, or does not end.
    OrphanList,
}

/// A bit per item.
struct Bits(Vec<u8>);

impl Bits {
    fn new(n: u32) -> Bits { Bits(vec![0; n.div_ceil(8) as usize]) }

    fn get(&self, i: u32) -> bool { self.0[(i / 8) as usize] & (1 << (i % 8)) != 0 }

    /// Sets bit `i`; whether it was already set.
    fn set(&mut self, i: u32) -> bool {
        let was = self.get(i);
        self.0[(i / 8) as usize] |= 1 << (i % 8);
        was
    }
}

/// The check's state: the blocks found in use, the hash blocks that fail their own hash, and
/// what it found.
struct Walk {
    used: Bits,
    bad_hashes: Vec<u32>,
    found: Vec<Problem>,
}

/// Reads a block for the check: a failure but the device's is a finding, not an error, and is the
/// hash block's if that is what failed.
macro_rules! readable {
    ($fs:expr, $b:expr, $w:expr) => {
        match $fs.block($b) {
            Ok(buf) => Some(buf),
            Err(Error::Io) => return Err(Error::Io),
            Err(_) => {
                if !$w.bad_hashes.contains(&$fs.l.slot($b).0) {
                    $w.found.push(Problem::BadBlock($b));
                }
                None
            }
        }
    };
}

impl<D: BlockDevice> Filesystem<D> {
    /// Walks the whole volume and names every problem it finds; an error only if the device fails.
    pub fn check(&mut self) -> Result<Vec<Problem>, Error> { self.op(|fs| fs.check_all()) }

    fn check_inode(&mut self, i: u32, w: &mut Walk) -> Result<Option<Inode>, Error> {
        match self.inode(i) {
            Ok(n) => Ok(Some(n)),
            Err(Error::Io) => Err(Error::Io),
            Err(_) => {
                w.found.push(Problem::BadInode(i));
                Ok(None)
            }
        }
    }

    fn check_all(&mut self) -> Result<Vec<Problem>, Error> {
        let l = self.l;
        let mut w = Walk { used: Bits::new(l.block_count), bad_hashes: Vec::new(), found: Vec::new() };
        for hb in l.hash_start..l.bitmap_start {
            match self.hash_block(hb) {
                Ok(_) => {}
                Err(Error::Io) => return Err(Error::Io),
                Err(_) => {
                    w.bad_hashes.push(hb);
                    w.found.push(Problem::BadBlock(hb));
                }
            }
        }
        let mut orphans = Bits::new(l.inode_count);
        let mut p = 0;
        for steps in 0..=l.inode_count {
            let Some(n) = self.check_inode(p, &mut w)? else { break };
            if n.next == 0 {
                break;
            }
            if steps == l.inode_count || orphans.set(n.next) {
                w.found.push(Problem::OrphanList);
                break;
            }
            p = n.next;
        }

        let mut named = Bits::new(l.inode_count);
        named.set(ROOT);
        let mut todo = vec![ROOT];
        while let Some(d) = todo.pop() {
            let Some(n) = self.check_inode(d, &mut w)? else { continue };
            let blocks = self.check_blocks(d, &n, &mut w)?;
            let mut names: Vec<Vec<u8>> = Vec::new();
            for blk in blocks {
                let Some(buf) = readable!(self, blk, w) else { continue };
                for k in 0..DIRENTS_PER_BLOCK {
                    let (ino, name) = match dirent(&buf, k, &l) {
                        Ok(Some(e)) => e,
                        Ok(None) => continue,
                        Err(_) => {
                            w.found.push(Problem::BadBlock(blk));
                            break;
                        }
                    };
                    if names.iter().any(|m| m[..] == *name) {
                        w.found.push(Problem::DuplicateName(d));
                    }
                    names.push(name.into());
                    if named.set(ino) {
                        w.found.push(Problem::NamedTwice(ino));
                        continue;
                    }
                    let Some(c) = self.check_inode(ino, &mut w)? else { continue };
                    if c.kind == KIND_FREE || c.nlink == 0 {
                        w.found.push(Problem::NamedFree(ino));
                    } else if c.kind == KIND_DIR {
                        todo.push(ino);
                    } else {
                        self.check_blocks(ino, &c, &mut w)?;
                    }
                }
            }
        }

        for i in 1..l.inode_count {
            let Some(n) = self.check_inode(i, &mut w)? else { continue };
            let (blk, at) = l.attrs_at(i);
            if let Some(area) = readable!(self, blk, w) {
                let bad = match parse_attrs(&area[at..at + ATTRS]) {
                    Ok(a) => n.kind == KIND_FREE && !a.is_empty(),
                    Err(_) => true,
                };
                if bad {
                    w.found.push(Problem::BadAttrs(i));
                }
            }
            if orphans.get(i) {
                if n.kind == KIND_FREE {
                    w.found.push(Problem::OrphanList);
                } else if !named.get(i) {
                    self.check_blocks(i, &n, &mut w)?;
                }
            } else if n.kind != KIND_FREE && !named.get(i) {
                w.found.push(Problem::LeakedInode(i));
            }
        }

        for b in l.data_start..l.block_count {
            match (self.bit(b), w.used.get(b)) {
                (false, true) => w.found.push(Problem::UsedButFree(b)),
                (true, false) => w.found.push(Problem::LeakedBlock(b)),
                _ => {}
            }
        }
        Ok(w.found)
    }

    /// Marks a block in use; one already in use is shared.
    fn mark(&mut self, b: u32, w: &mut Walk) {
        if w.used.set(b) {
            w.found.push(Problem::SharedBlock(b));
        }
    }

    /// Marks inode `i`'s indirect and data blocks in use, reads each data block against its hash,
    /// and checks them against its size; returns its data blocks in file order.
    fn check_blocks(&mut self, i: u32, n: &Inode, w: &mut Walk) -> Result<Vec<u32>, Error> {
        let mut data = Vec::new();
        for (idx, &a) in n.direct.iter().enumerate() {
            self.check_data(i, n, idx as u64, a, &mut data, w)?;
        }
        match n.single {
            // A hole where an indirect block would be is a hole at its first file block.
            0 => self.check_data(i, n, SINGLE_BASE, 0, &mut data, w)?,
            s => self.check_indirect(i, n, s, SINGLE_BASE, &mut data, w)?,
        }
        if n.double == 0 {
            return self.check_data(i, n, DOUBLE_BASE, 0, &mut data, w).map(|()| data);
        }
        self.mark(n.double, w);
        if let Some(top) = readable!(self, n.double, w) {
            for k in 0..PER_INDIRECT {
                let (s, base) = (u32_at(&top[..], 4 * k as usize), DOUBLE_BASE + k * PER_INDIRECT);
                match s {
                    0 => self.check_data(i, n, base, 0, &mut data, w)?,
                    s if self.l.is_data(s) => self.check_indirect(i, n, s, base, &mut data, w)?,
                    _ => w.found.push(Problem::BadBlock(n.double)),
                }
            }
        }
        Ok(data)
    }

    /// Indirect block `blk` of inode `i`, whose first address maps file block `base`.
    fn check_indirect(
        &mut self,
        i: u32,
        n: &Inode,
        blk: u32,
        base: u64,
        data: &mut Vec<u32>,
        w: &mut Walk,
    ) -> Result<(), Error> {
        self.mark(blk, w);
        if base >= n.size.div_ceil(BLOCK as u64) {
            w.found.push(Problem::PastSize(i));
        }
        let Some(ents) = readable!(self, blk, w) else { return Ok(()) };
        for k in 0..PER_INDIRECT {
            let a = u32_at(&ents[..], 4 * k as usize);
            if a != 0 && !self.l.is_data(a) {
                w.found.push(Problem::BadBlock(blk));
                continue;
            }
            self.check_data(i, n, base + k, a, data, w)?;
        }
        Ok(())
    }

    /// File block `idx` of inode `i` at address `a` (0 for a hole).
    fn check_data(
        &mut self,
        i: u32,
        n: &Inode,
        idx: u64,
        a: u32,
        data: &mut Vec<u32>,
        w: &mut Walk,
    ) -> Result<(), Error> {
        let count = n.size.div_ceil(BLOCK as u64);
        if a == 0 {
            if n.kind == KIND_DIR && idx < count {
                w.found.push(Problem::DirectoryHole(i));
            }
            return Ok(());
        }
        self.mark(a, w);
        if idx >= count {
            w.found.push(Problem::PastSize(i));
            return Ok(());
        }
        let Some(buf) = readable!(self, a, w) else { return Ok(()) };
        let tail = (n.size % BLOCK as u64) as usize;
        if idx + 1 == count && tail != 0 && buf[tail..].iter().any(|&x| x != 0) {
            w.found.push(Problem::TailNotZero(i));
        }
        data.push(a);
        Ok(())
    }
}
