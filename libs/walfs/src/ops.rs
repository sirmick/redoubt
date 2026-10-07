//! Paths and directories: lookup, create, mkdir, remove, rename, read_dir, stat, and user
//! attributes.

use alloc::vec::Vec;

use crate::fs::Filesystem;
use crate::layout::{
    DIRENTS_PER_BLOCK, Inode, KIND_DIR, KIND_FREE, dirent, encode_attrs, parse_attrs, put_dirent,
};
use crate::{
    ATTR_MAX, ATTRS, BLOCK, BlockDevice, DirEntry, Error, FileType, MAX_FILE_SIZE, Metadata, NAME_MAX, ROOT,
};

/// A path's names, root first; `/` and the empty path are the root.
pub(crate) fn components(path: &str) -> Result<Vec<&[u8]>, Error> {
    let mut out = Vec::new();
    for name in path.split('/').filter(|n| !n.is_empty()) {
        let name = name.as_bytes();
        if name == b"." || name == b".." || name.contains(&0) {
            return Err(Error::Invalid);
        }
        if name.len() > NAME_MAX {
            return Err(Error::NameTooLong);
        }
        out.push(name);
    }
    Ok(out)
}

/// Where a name is in a directory: the block and the entry, and the inode it names.
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub blk: u32,
    pub k: usize,
    pub ino: u32,
}

/// What a scan of a directory found.
pub(crate) struct Scan {
    pub found: Option<Slot>,
    /// The first free slot, if any.
    pub free: Option<(u32, usize)>,
    pub empty: bool,
}

pub(crate) fn metadata(ino: u32, n: &Inode) -> Metadata {
    let kind = if n.kind == KIND_DIR { FileType::Dir } else { FileType::File };
    Metadata { kind, size: n.size, inode: ino, generation: n.generation, mtime: n.mtime }
}

impl<D: BlockDevice> Filesystem<D> {
    /// Calls `f` with each entry of directory `n`, a block at a time: the volume, the entry's block,
    /// its index and what it names. A hole is corrupt where it is met, so a forged size costs
    /// nothing before it.
    fn each_entry(
        &mut self,
        n: &Inode,
        mut f: impl FnMut(&mut Self, u32, usize, Option<(u32, &[u8])>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        for idx in 0..n.size / BLOCK as u64 {
            let blk = self.bmap(n, idx)?;
            if blk == 0 {
                return Err(Error::Corrupt);
            }
            let buf = self.block(blk)?;
            for k in 0..DIRENTS_PER_BLOCK {
                let e = dirent(&buf, k, &self.l)?;
                f(self, blk, k, e)?;
            }
        }
        Ok(())
    }

    /// Reads directory `n` whole for `name`: a name found twice is corrupt.
    pub(crate) fn scan(&mut self, n: &Inode, name: &[u8]) -> Result<Scan, Error> {
        let mut s = Scan { found: None, free: None, empty: true };
        self.each_entry(n, |_, blk, k, e| {
            match e {
                None if s.free.is_none() => s.free = Some((blk, k)),
                None => {}
                Some((ino, nm)) => {
                    s.empty = false;
                    if nm == name {
                        if s.found.is_some() {
                            return Err(Error::Corrupt);
                        }
                        s.found = Some(Slot { blk, k, ino });
                    }
                }
            }
            Ok(())
        })?;
        Ok(s)
    }

    /// The inode `names` lead to from the root.
    fn walk(&mut self, names: &[&[u8]]) -> Result<(u32, Inode), Error> {
        let (mut ino, mut n) = (ROOT, self.inode(ROOT)?);
        for name in names {
            if n.kind != KIND_DIR {
                return Err(Error::NotDir);
            }
            ino = self.scan(&n, name)?.found.ok_or(Error::NoEntry)?.ino;
            n = self.named(ino)?;
        }
        Ok((ino, n))
    }

    /// Inode `ino`, which a directory entry names: a free or removed one is corrupt.
    fn named(&mut self, ino: u32) -> Result<Inode, Error> {
        let n = self.inode(ino)?;
        if n.kind == KIND_FREE || n.nlink == 0 {
            return Err(Error::Corrupt);
        }
        Ok(n)
    }

    pub(crate) fn resolve(&mut self, path: &str) -> Result<(u32, Inode), Error> {
        self.walk(&components(path)?)
    }

    /// The directory holding the last name of `names`, and what its scan for the name found.
    pub(crate) fn parent(&mut self, names: &[&[u8]]) -> Result<(u32, Inode, Scan), Error> {
        let (last, up) = names.split_last().ok_or(Error::Invalid)?;
        let (p, pn) = self.walk(up)?;
        if pn.kind != KIND_DIR {
            return Err(Error::NotDir);
        }
        let s = self.scan(&pn, last)?;
        Ok((p, pn, s))
    }

    /// Names `ino` as `name` in directory `p`: the first free slot, or a new block at its end.
    fn add_entry(&mut self, p: u32, free: Option<(u32, usize)>, name: &[u8], ino: u32) -> Result<(), Error> {
        let mut pn = self.inode(p)?;
        let (blk, k) = match free {
            Some(slot) => slot,
            None => {
                if pn.size + BLOCK as u64 > MAX_FILE_SIZE {
                    return Err(Error::NoSpace);
                }
                let idx = pn.size / BLOCK as u64;
                let (blk, _) = self.bmap_alloc(&mut pn, idx)?;
                pn.size += BLOCK as u64;
                (blk, 0)
            }
        };
        self.put_inode(p, &pn)?;
        put_dirent(self.tx_get(blk)?, k, ino, name);
        self.touch(p)
    }

    /// Clears an entry of directory `p`.
    fn clear_entry(&mut self, p: u32, s: Slot) -> Result<(), Error> {
        put_dirent(self.tx_get(s.blk)?, s.k, 0, &[]);
        self.touch(p)
    }

    /// Directory `p`'s mtime, set to now.
    fn touch(&mut self, p: u32) -> Result<(), Error> {
        let mut pn = self.inode(p)?;
        pn.mtime = self.now;
        self.put_inode(p, &pn)
    }

    /// A new file or directory at `names`, in the operation's transaction.
    pub(crate) fn create(&mut self, names: &[&[u8]], kind: u16) -> Result<u32, Error> {
        let (p, _, s) = self.parent(names)?;
        if s.found.is_some() {
            return Err(Error::Exists);
        }
        let (ino, _) = self.alloc_inode(kind)?;
        self.add_entry(p, s.free, names[names.len() - 1], ino)?;
        Ok(ino)
    }

    /// Unlinks inode `ino`: on the orphan list, and finished now unless a handle holds it.
    fn unlink(&mut self, ino: u32) -> Result<(), Error> {
        let mut n = self.inode(ino)?;
        n.nlink = 0;
        self.put_inode(ino, &n)?;
        self.orphan_link(ino)?;
        if !self.is_open(ino) {
            self.finish(ino)?;
        }
        Ok(())
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), Error> {
        self.op(|fs| {
            let names = components(path)?;
            if names.is_empty() {
                return Err(Error::Exists);
            }
            fs.create(&names, KIND_DIR).map(|_| ())
        })
    }

    /// Removes a file, or an empty directory. A file a handle holds open stays readable through it
    /// until it is closed.
    pub fn remove(&mut self, path: &str) -> Result<(), Error> {
        self.op(|fs| {
            let names = components(path)?;
            let (p, _, s) = fs.parent(&names)?;
            let slot = s.found.ok_or(Error::NoEntry)?;
            let n = fs.inode(slot.ino)?;
            if n.kind == KIND_DIR && !fs.scan(&n, &[])?.empty {
                return Err(Error::NotEmpty);
            }
            fs.clear_entry(p, slot)?;
            fs.unlink(slot.ino)
        })
    }

    /// Moves `from` to `to`, within the volume, as one transaction. Over an existing file, or an
    /// empty directory, it replaces it. Moving a directory into itself is refused.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        self.op(|fs| {
            let (fc, tc) = (components(from)?, components(to)?);
            let (fp, _, fs_scan) = fs.parent(&fc)?;
            let src = fs_scan.found.ok_or(Error::NoEntry)?;
            let n = fs.inode(src.ino)?;
            if tc.is_empty() || (n.kind == KIND_DIR && tc.len() > fc.len() && tc[..fc.len()] == fc[..]) {
                return Err(Error::Invalid);
            }
            let (tp, _, ts) = fs.parent(&tc)?;
            if fc == tc {
                return Ok(());
            }
            let name = tc[tc.len() - 1];
            match ts.found {
                Some(dst) => {
                    let victim = fs.inode(dst.ino)?;
                    match (n.kind == KIND_DIR, victim.kind == KIND_DIR) {
                        (false, true) => return Err(Error::IsDir),
                        (true, false) => return Err(Error::NotDir),
                        (true, true) if !fs.scan(&victim, &[])?.empty => return Err(Error::NotEmpty),
                        _ => {}
                    }
                    put_dirent(fs.tx_get(dst.blk)?, dst.k, src.ino, name);
                    fs.clear_entry(fp, src)?;
                    fs.touch(tp)?;
                    fs.unlink(dst.ino)
                }
                None => {
                    fs.add_entry(tp, ts.free, name, src.ino)?;
                    fs.clear_entry(fp, src)
                }
            }
        })
    }

    pub fn stat(&mut self, path: &str) -> Result<Metadata, Error> {
        self.op(|fs| fs.resolve(path).map(|(ino, n)| metadata(ino, &n)))
    }

    /// Calls `f` with each entry of the directory at `path`, in the order the directory holds them;
    /// returns how many.
    pub fn read_dir(&mut self, path: &str, mut f: impl FnMut(&DirEntry)) -> Result<u32, Error> {
        self.op(|fs| {
            let (_, n) = fs.resolve(path)?;
            if n.kind != KIND_DIR {
                return Err(Error::NotDir);
            }
            let mut count = 0;
            fs.each_entry(&n, |fs, _, _, e| {
                if let Some((ino, name)) = e {
                    let c = fs.named(ino)?;
                    f(&DirEntry { name, meta: metadata(ino, &c) });
                    count += 1;
                }
                Ok(())
            })?;
            Ok(count)
        })
    }

    /// The inode at `path`, for attribute `typ`, which must not be 0.
    fn attr_target(&mut self, path: &str, typ: u8) -> Result<u32, Error> {
        let (ino, _) = self.resolve(path)?;
        if typ == 0 {
            return Err(Error::Invalid);
        }
        Ok(ino)
    }

    fn attrs(&mut self, ino: u32) -> Result<Vec<(u8, Vec<u8>)>, Error> {
        let (blk, at) = self.l.attrs_at(ino);
        parse_attrs(&self.block(blk)?[at..at + ATTRS])
    }

    fn put_attrs(&mut self, ino: u32, attrs: &[(u8, Vec<u8>)]) -> Result<(), Error> {
        let area = encode_attrs(attrs).ok_or(Error::NoSpace)?;
        let (blk, at) = self.l.attrs_at(ino);
        self.tx_get(blk)?[at..at + ATTRS].copy_from_slice(&area);
        Ok(())
    }

    /// The value of attribute `typ` (1 to 255) of the file or directory at `path`.
    pub fn get_attr(&mut self, path: &str, typ: u8) -> Result<Vec<u8>, Error> {
        self.op(|fs| {
            let ino = fs.attr_target(path, typ)?;
            fs.attrs(ino)?.into_iter().find(|a| a.0 == typ).map(|a| a.1).ok_or(Error::NoAttr)
        })
    }

    /// Sets attribute `typ` (1 to 255); `NoSpace` if the inode's attributes would not fit its area.
    pub fn set_attr(&mut self, path: &str, typ: u8, value: &[u8]) -> Result<(), Error> {
        self.op(|fs| {
            let ino = fs.attr_target(path, typ)?;
            if value.len() > ATTR_MAX {
                return Err(Error::NoSpace);
            }
            let mut attrs = fs.attrs(ino)?;
            match attrs.iter_mut().find(|a| a.0 == typ) {
                Some(a) => a.1 = value.into(),
                None => attrs.push((typ, value.into())),
            }
            fs.put_attrs(ino, &attrs)
        })
    }

    /// Removes attribute `typ`, if the inode has it.
    pub fn remove_attr(&mut self, path: &str, typ: u8) -> Result<(), Error> {
        self.op(|fs| {
            let ino = fs.attr_target(path, typ)?;
            let mut attrs = fs.attrs(ino)?;
            let before = attrs.len();
            attrs.retain(|a| a.0 != typ);
            if attrs.len() != before { fs.put_attrs(ino, &attrs) } else { Ok(()) }
        })
    }
}
