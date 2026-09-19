//! Namespace operations: path lookup, directories, rename, stat, user attributes, and a
//! consistency check.

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::fs::*;
use crate::mdir::*;
use crate::tag::{self, *};
use crate::{BlockDevice, DirEntry, Error, FileType, Filesystem, Metadata};

/// Where a path leads.
pub(crate) enum Lookup {
    Root,
    /// Entry `id` of `dir`.
    Found { dir: MDir, id: u16 },
    /// The last name does not exist; it would be created as entry `id` of `dir` (the last
    /// pair of its directory).
    Missing { dir: MDir, id: u16 },
}

/// Splits a path into names. Empty components (repeated or trailing slashes) are skipped;
/// `.` and `..` are refused rather than resolved: callers walk one name at a time.
pub(crate) fn components(path: &str) -> Result<Vec<&[u8]>, Error> {
    let names: Vec<&[u8]> = path.split('/').filter(|n| !n.is_empty()).map(str::as_bytes).collect();
    if names.iter().any(|n| *n == b"." || *n == b"..") {
        return Err(Error::Invalid);
    }
    Ok(names)
}

/// The order of names in a directory, exactly the reference's `lfs_dir_find_match`: bytes
/// compared over the common length; when one name is a prefix of the other, the longer one
/// sorts first. A directory is sorted across all its pairs (SPEC.md "0x601 HARDTAIL"), and
/// lookups rely on it: they stop at the first pair holding a greater name.
fn name_order(on_disk: &[u8], name: &[u8]) -> Ordering {
    let n = on_disk.len().min(name.len());
    match on_disk[..n].cmp(&name[..n]) {
        Ordering::Equal => name.len().cmp(&on_disk.len()),
        o => o,
    }
}

impl<D: BlockDevice> Filesystem<D> {
    /// Entries of `dir` a lookup may see: files and directories, minus the source of a
    /// rename that was interrupted (it is already deleted, logically).
    fn visible<'a>(&self, dir: &'a MDir) -> impl Iterator<Item = (u16, &'a Entry)> + 'a {
        let moved = self.gdisk.move_in(&dir.pair);
        dir.c
            .entries
            .iter()
            .enumerate()
            .map(|(id, e)| (id as u16, e))
            .filter(move |(id, e)| Some(*id) != moved && is_file_or_dir(e.name_type))
    }

    /// Looks `name` up in the directory whose first pair is `head` (the reference's
    /// `lfs_dir_find`). Not found: the pair and id where it would be inserted to keep the
    /// directory sorted, which is where the search stopped.
    fn find(&mut self, head: Pair, name: &[u8]) -> Result<Lookup, Error> {
        let mut pair = head;
        let mut walk = self.walk();
        loop {
            let dir = self.fetch(pair)?;
            let (mut found, mut insert) = (None, None);
            for (id, e) in self.visible(&dir) {
                match name_order(&e.name, name) {
                    Ordering::Equal => found = Some(id),
                    Ordering::Greater if insert.is_none() => insert = Some(id),
                    _ => {}
                }
            }
            if let Some(id) = found {
                return Ok(Lookup::Found { dir, id });
            }
            // A greater name here means the name is not in any later pair either.
            if let Some(id) = insert {
                return Ok(Lookup::Missing { dir, id });
            }
            if !dir.c.split {
                let id = dir.c.entries.len() as u16;
                return Ok(Lookup::Missing { dir, id });
            }
            walk.step()?;
            pair = dir.c.tail;
        }
    }

    /// Resolves a path; also returns its last name.
    pub(crate) fn lookup<'p>(&mut self, path: &'p str) -> Result<(Lookup, &'p [u8]), Error> {
        let names = components(path)?;
        let Some((last, parents)) = names.split_last() else { return Ok((Lookup::Root, b"")) };
        let mut head = self.root;
        for name in parents {
            let Lookup::Found { dir, id } = self.find(head, name)? else { return Err(Error::NoEntry) };
            head = match self.decode(&dir.c.entries[id as usize])? {
                Struct::Dir(p) => p,
                _ => return Err(Error::NotDir),
            };
        }
        Ok((self.find(head, last)?, *last))
    }

    pub(crate) fn check_name(&self, name: &[u8]) -> Result<(), Error> {
        if name.is_empty() {
            Err(Error::Invalid)
        } else if name.len() as u32 > self.name_max {
            Err(Error::NameTooLong)
        } else {
            Ok(())
        }
    }

    fn metadata(&self, e: &Entry) -> Result<Metadata, Error> {
        Ok(match self.decode(e)? {
            Struct::Dir(_) => Metadata { kind: FileType::Dir, size: 0 },
            Struct::Inline(d) => Metadata { kind: FileType::File, size: d.len() as u32 },
            Struct::Ctz { size, .. } => Metadata { kind: FileType::File, size },
        })
    }

    /// The first pair of a directory, given its path.
    fn dir_head(&mut self, path: &str) -> Result<Pair, Error> {
        match self.lookup(path)?.0 {
            Lookup::Root => Ok(self.root),
            Lookup::Found { dir, id } => match self.decode(&dir.c.entries[id as usize])? {
                Struct::Dir(p) => Ok(p),
                _ => Err(Error::NotDir),
            },
            Lookup::Missing { .. } => Err(Error::NoEntry),
        }
    }

    pub fn stat(&mut self, path: &str) -> Result<Metadata, Error> {
        self.check_poison()?;
        match self.lookup(path)?.0 {
            Lookup::Root => Ok(Metadata { kind: FileType::Dir, size: 0 }),
            Lookup::Found { dir, id } => self.metadata(&dir.c.entries[id as usize]),
            Lookup::Missing { .. } => Err(Error::NoEntry),
        }
    }

    /// Calls `f` with each entry of a directory. There are no directory handles: a caller
    /// that reads a directory in pieces walks it again and skips what it has seen.
    pub fn read_dir(&mut self, path: &str, mut f: impl FnMut(&DirEntry)) -> Result<(), Error> {
        self.check_poison()?;
        let mut pair = self.dir_head(path)?;
        let mut walk = self.walk();
        loop {
            let dir = self.fetch(pair)?;
            for (_, e) in self.visible(&dir) {
                let m = self.metadata(e)?;
                f(&DirEntry { name: &e.name, kind: m.kind, size: m.size });
            }
            if !dir.c.split {
                return Ok(());
            }
            walk.step()?;
            pair = dir.c.tail;
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), Error> {
        self.mutate(|fs| {
            let (Lookup::Missing { dir, id }, name) = fs.lookup(path)? else { return Err(Error::Exists) };
            fs.check_name(name)?;
            // The new pair joins the list of all pairs after the last pair of the parent
            // directory (a hard-tail chain cannot be split).
            let mut last = dir.clone();
            let mut walk = fs.walk();
            while last.c.split {
                walk.step()?;
                last = fs.fetch(last.c.tail)?;
            }
            let child = fs.new_pair(&Contents::new(last.c.tail, false))?;
            let mut attrs = vec![
                attr_create(id),
                attr_name(TYPE_DIR, id, name)?,
                attr_struct(TYPE_DIRSTRUCT, id, &pair_bytes(child))?,
            ];
            if dir.c.split {
                // Linking and naming are two commits: until the second, the new directory is
                // an orphan, and the orphan flag says so.
                fs.prep_orphans(1);
                fs.commit(last.pair, &[attr_tail(false, child)])?;
                fs.prep_orphans(-1);
            } else {
                attrs.push(attr_tail(false, child));
            }
            fs.commit(dir.pair, &attrs)
        })
    }

    /// Removes a file, or an empty directory.
    pub fn remove(&mut self, path: &str) -> Result<(), Error> {
        self.mutate(|fs| {
            let (dir, id) = match fs.lookup(path)?.0 {
                Lookup::Found { dir, id } => (dir, id),
                Lookup::Root => return Err(Error::Invalid),
                Lookup::Missing { .. } => return Err(Error::NoEntry),
            };
            let child = fs.empty_dir(&dir.c.entries[id as usize])?;
            fs.commit(dir.pair, &[attr_delete(id)])?;
            if let Some(child) = child {
                fs.drop_orphan(child)?;
            }
            Ok(())
        })
    }

    /// For a directory entry: checks the directory is empty and marks the filesystem as
    /// about to have an orphan (its pair stays on the list until `drop_orphan`).
    fn empty_dir(&mut self, e: &Entry) -> Result<Option<Pair>, Error> {
        let Struct::Dir(p) = self.decode(e)? else { return Ok(None) };
        let d = self.fetch(p)?;
        if !d.c.entries.is_empty() || d.c.split {
            return Err(Error::NotEmpty);
        }
        self.prep_orphans(1);
        Ok(Some(d.pair))
    }

    /// Unlinks a removed directory's pair from the list of all pairs. If power fails before
    /// this, the orphan flag written with the removal makes the next mount do it.
    fn drop_orphan(&mut self, child: Pair) -> Result<(), Error> {
        self.prep_orphans(-1);
        let child = self.fetch(child)?;
        let Some(pred) = self.pred(&child.pair)? else { return Err(Error::Corrupt) };
        self.gdelta = self.gdelta.xor(&child.c.gdelta);
        self.commit(pred.pair, &[attr_tail(child.c.split, child.c.tail)])
    }

    /// Renames a file or directory, replacing a file of the same kind (or an empty
    /// directory) at `to`. Open files follow the rename.
    ///
    /// Across pairs this is two commits: the new entry is created together with a "pending
    /// move" in the global state, then the old one is deleted. If power fails in between,
    /// the pending move hides the old entry and the next write deletes it.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        self.mutate(|fs| {
            let (oldcwd, oldid) = match fs.lookup(from)?.0 {
                Lookup::Found { dir, id } => (dir, id),
                Lookup::Root => return Err(Error::Invalid),
                Lookup::Missing { .. } => return Err(Error::NoEntry),
            };
            let old = oldcwd.c.entries[oldid as usize].clone();
            // A directory cannot move inside itself: it would cut itself off from the root.
            let (old_names, new_names) = (components(from)?, components(to)?);
            if old.name_type == TYPE_DIR && new_names.len() > old_names.len() && new_names.starts_with(&old_names) {
                return Err(Error::Invalid);
            }

            let (target, newname) = fs.lookup(to)?;
            let (newcwd, newid, replacing) = match target {
                Lookup::Found { dir, id } => (dir, id, true),
                Lookup::Missing { dir, id } => (dir, id, false),
                Lookup::Root => return Err(Error::Invalid),
            };
            let samepair = pair_overlaps(&oldcwd.pair, &newcwd.pair);
            // The old entry's id once the new one is created, if both are in one pair.
            let mut newoldid = oldid;
            let mut orphan = None;
            if replacing {
                let prev = &newcwd.c.entries[newid as usize];
                if prev.name_type != old.name_type {
                    return Err(if prev.name_type == TYPE_DIR { Error::IsDir } else { Error::NotDir });
                }
                if samepair && newid == oldid {
                    return Ok(());
                }
                orphan = fs.empty_dir(prev)?;
            } else {
                fs.check_name(newname)?;
                if samepair && newid <= newoldid {
                    newoldid += 1;
                }
            }

            let following: Vec<usize> = fs.files_at(&oldcwd.pair, oldid);
            if !samepair {
                fs.prep_move(Some((newoldid, oldcwd.pair)));
            }
            let mut attrs = Vec::new();
            if replacing {
                attrs.push(attr_delete(newid));
            }
            attrs.push(attr_create(newid));
            attrs.push(attr_name(old.name_type, newid, newname)?);
            if let Some((t, d)) = &old.strct {
                attrs.push(attr_struct(*t, newid, d)?);
            }
            for (t, d) in &old.attrs {
                attrs.push(attr_struct(TYPE_USERATTR | *t as u16, newid, d)?);
            }
            if samepair {
                attrs.push(attr_delete(newoldid));
            }
            fs.commit(newcwd.pair, &attrs)?;

            if !samepair {
                fs.prep_move(None);
                fs.commit(oldcwd.pair, &[attr_delete(oldid)])?;
            }
            if let Some(child) = orphan {
                fs.drop_orphan(child)?;
            }

            if !following.is_empty() {
                if let (Lookup::Found { dir, id }, _) = fs.lookup(to)? {
                    for i in following {
                        if let Some(Some(f)) = fs.files.get_mut(i) {
                            f.loc = Some(crate::file::Loc { pair: dir.pair, id });
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// Open files whose entry is `id` in `pair`.
    fn files_at(&self, pair: &Pair, id: u16) -> Vec<usize> {
        let at = |f: &crate::file::OpenFile| f.loc.is_some_and(|l| l.id == id && pair_overlaps(&l.pair, pair));
        self.files.iter().enumerate().filter(|(_, f)| f.as_ref().is_some_and(at)).map(|(i, _)| i).collect()
    }

    /// Where a path's attributes live. The root's are on the superblock entry (id 0 of the
    /// root pair), as in the reference.
    fn attr_target(&mut self, path: &str) -> Result<(Pair, u16), Error> {
        match self.lookup(path)?.0 {
            Lookup::Root => Ok((self.root, 0)),
            Lookup::Found { dir, id } => Ok((dir.pair, id)),
            Lookup::Missing { .. } => Err(Error::NoEntry),
        }
    }

    /// Reads user attribute `typ` of a file or directory.
    pub fn get_attr(&mut self, path: &str, typ: u8) -> Result<Vec<u8>, Error> {
        self.check_poison()?;
        let (pair, id) = self.attr_target(path)?;
        let dir = self.fetch(pair)?;
        let e = dir.c.entries.get(id as usize).ok_or(Error::Corrupt)?;
        e.attr(typ).map(<[u8]>::to_vec).ok_or(Error::NoAttr)
    }

    /// Sets user attribute `typ` (at most `attr_max`, 1022 bytes by default).
    pub fn set_attr(&mut self, path: &str, typ: u8, data: &[u8]) -> Result<(), Error> {
        self.mutate(|fs| {
            if data.len() as u32 > fs.attr_max {
                return Err(Error::NoSpace);
            }
            let (pair, id) = fs.attr_target(path)?;
            fs.commit(pair, &[attr_struct(TYPE_USERATTR | typ as u16, id, data)?])
        })
    }

    pub fn remove_attr(&mut self, path: &str, typ: u8) -> Result<(), Error> {
        self.mutate(|fs| {
            let (pair, id) = fs.attr_target(path)?;
            fs.commit(pair, &[(tag::mk(TYPE_USERATTR | typ as u16, id, SIZE_DELETE), Vec::new())])
        })
    }

    /// Repairs what an interrupted operation left (as any write does), then checks the
    /// whole volume: every pair and file readable, no block used twice, the directory tree
    /// a tree, and every pair on the list either a superblock or reachable from the root.
    ///
    /// The orphan repair runs even when the orphan flag is clear: the C reference can leave
    /// a removed directory on the list with the flag cleared (a relocation during the
    /// removal resets its orphan count), which only leaks its two blocks, but this finds it.
    pub fn fsck(&mut self) -> Result<(), Error> {
        self.mutate(|fs| {
            fs.deorphan()?;
            fs.check_volume()
        })
    }

    fn check_volume(&mut self) -> Result<(), Error> {
        let words = (self.block_count as usize).div_ceil(64);
        let mut used = vec![0u64; words];
        let mut listed = BTreeSet::new();
        let mut superblocks = BTreeSet::new();
        let mark = |b: u32, used: &mut Vec<u64>| {
            let (w, bit) = ((b / 64) as usize, 1u64 << (b % 64));
            if used[w] & bit != 0 {
                return Err(Error::Corrupt);
            }
            used[w] |= bit;
            Ok(())
        };

        // Every pair on the list, and every file's blocks, exactly once.
        let mut tail = [0, 1];
        let mut walk = self.walk();
        while !pair_is_null(&tail) {
            walk.step()?;
            let d = self.fetch(tail)?;
            mark(d.pair[0], &mut used)?;
            mark(d.pair[1], &mut used)?;
            listed.insert(d.pair[0].min(d.pair[1]));
            if d.c.entries.first().is_some_and(|e| e.name_type == TYPE_SUPERBLOCK) {
                superblocks.insert(d.pair[0].min(d.pair[1]));
            }
            for (_, e) in self.visible(&d) {
                if let Struct::Ctz { head, size } = self.decode(e)? {
                    let mut blocks = Vec::new();
                    self.ctz_blocks(head, size, &mut blocks)?;
                    for b in blocks {
                        mark(b, &mut used)?;
                    }
                }
            }
            tail = d.c.tail;
        }

        // The tree from the root: each directory's pairs visited once.
        let mut reached = BTreeSet::new();
        let mut todo = vec![self.root];
        while let Some(head) = todo.pop() {
            let mut pair = head;
            loop {
                if !reached.insert(pair[0].min(pair[1])) {
                    return Err(Error::Corrupt);
                }
                let d = self.fetch(pair)?;
                for (_, e) in self.visible(&d) {
                    if let Struct::Dir(p) = self.decode(e)? {
                        todo.push(p);
                    }
                }
                if !d.c.split {
                    break;
                }
                pair = d.c.tail;
            }
        }
        let unreachable = listed.difference(&reached).any(|p| !superblocks.contains(p));
        if unreachable || !reached.is_subset(&listed) { Err(Error::Corrupt) } else { Ok(()) }
    }
}
