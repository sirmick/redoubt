//! Files: open handles, reading, and copy-on-write writing.
//!
//! Small files live inline in their directory's metadata (up to `inline_max` bytes, as the
//! reference). Larger files are CTZ skip-lists (`ctz.rs`). A write never modifies a block the
//! committed file uses: it copies the block it starts in and builds new blocks from there;
//! `sync` then commits the new head and size to the metadata in one step. Until then the old
//! contents stay intact on disk, which is what makes an interrupted write harmless.
//!
//! The block being filled is kept in memory (one block per writing handle) and programmed
//! once, when full or on flush; each data block is therefore programmed exactly once.

use alloc::vec;
use alloc::vec::Vec;

use crate::fs::{attr_create, attr_name, attr_struct, Struct};
use crate::mdir::{align_up, Pair};
use crate::ops::{names_dir, Lookup};
use crate::tag::*;
use crate::{ctz, BlockDevice, Error, Filesystem};

/// An open file. Handles are only meaningful to the filesystem that returned them.
///
/// A closed handle's slot is reused by later opens; the generation number makes a stale
/// handle fail with [`Error::Invalid`] rather than reach the new file. `fsd` still owns the
/// map from 9P fids to handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileHandle {
    slot: u32,
    generation: u32,
}

/// How to open a file. At least one of `read` and `write`; the rest need `write`.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    /// Create the file if it does not exist.
    pub create: bool,
    /// Create the file, failing if it exists.
    pub create_new: bool,
    /// Empty the file (committed on the next sync).
    pub truncate: bool,
}

/// Where a file's entry is: `None` once the file was removed while open (its data stays
/// readable through the handle; sync then commits nothing).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Loc {
    pub pair: Pair,
    pub id: u16,
}

/// A file's contents as of its last flush.
pub(crate) enum Content {
    Inline(Vec<u8>),
    Ctz { head: u32, size: u32 },
}

/// The block a writing handle is filling: allocated, erased on programming, contents in
/// memory up to `off` (including the skip-list pointers at its start).
pub(crate) struct Writer {
    pub block: u32,
    pub buf: Vec<u8>,
    pub off: u32,
}

pub(crate) struct OpenFile {
    generation: u32,
    pub loc: Option<Loc>,
    /// The entry's name, checked before a sync commits: the handle must still name the
    /// entry it was opened on.
    pub name: Vec<u8>,
    read: bool,
    write: bool,
    pub pos: u32,
    /// The contents as of the last flush. While a writer is active this is the old version,
    /// which the flush copies from after the written range.
    pub content: Content,
    pub writer: Option<Writer>,
    /// Contents differ from what the metadata says: the next sync commits.
    dirty: bool,
    /// A write-side operation failed: nothing more is committed through this handle, as the
    /// reference does. Its first error is reported again until it is closed.
    erred: Option<Error>,
}

impl OpenFile {
    fn size(&self) -> u32 {
        let flushed = match &self.content {
            Content::Inline(v) => v.len() as u32,
            Content::Ctz { size, .. } => *size,
        };
        if self.writer.is_some() { flushed.max(self.pos) } else { flushed }
    }
}

impl<D: BlockDevice> Filesystem<D> {
    fn slot(&self, h: FileHandle) -> Result<usize, Error> {
        match self.files.get(h.slot as usize) {
            Some(Some(f)) if f.generation == h.generation => Ok(h.slot as usize),
            _ => Err(Error::Invalid),
        }
    }

    fn file(&mut self, i: usize) -> Result<&mut OpenFile, Error> {
        self.files.get_mut(i).and_then(Option::as_mut).ok_or(Error::Invalid)
    }

    /// A handle usable for writing: open for writing and not errored.
    fn writable(&mut self, h: FileHandle) -> Result<usize, Error> {
        let i = self.slot(h)?;
        let f = self.file(i)?;
        match (f.write, f.erred) {
            (false, _) => Err(Error::Invalid),
            (true, Some(e)) => Err(e),
            (true, None) => Ok(i),
        }
    }

    /// Runs a write-side step on handle `i`; a failure marks the handle errored.
    fn on_file<T>(&mut self, i: usize, op: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        let r = self.mutate(op);
        if let Err(e) = r {
            if let Ok(f) = self.file(i) {
                f.erred.get_or_insert(e);
            }
        }
        r
    }

    pub fn open(&mut self, path: &str, o: OpenOptions) -> Result<FileHandle, Error> {
        let needs_write = o.create || o.create_new || o.truncate;
        if !o.write && (!o.read || needs_write) {
            return Err(Error::Invalid);
        }
        let mut f = if o.write {
            self.mutate(|fs| fs.open_file(path, o))?
        } else {
            self.check_poison()?;
            self.open_file(path, o)?
        };
        let slot = match self.files.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.files.push(None);
                self.files.len() - 1
            }
        };
        self.file_generation = self.file_generation.wrapping_add(1);
        f.generation = self.file_generation;
        self.files[slot] = Some(f);
        Ok(FileHandle { slot: slot as u32, generation: self.file_generation })
    }

    fn open_file(&mut self, path: &str, o: OpenOptions) -> Result<OpenFile, Error> {
        let (lookup, name) = self.lookup(path)?;
        let (loc, content, dirty) = match lookup {
            Lookup::Root => return Err(Error::IsDir),
            Lookup::Missing { dir, id } => {
                if !(o.create || o.create_new) {
                    return Err(Error::NoEntry);
                }
                if names_dir(path) {
                    return Err(Error::NotDir);
                }
                self.check_name(name)?;
                self.commit(dir.pair, &[
                    attr_create(id),
                    attr_name(TYPE_REG, id, name)?,
                    attr_struct(TYPE_INLINESTRUCT, id, &[])?,
                ])?;
                // The commit may have split the pair; look again rather than predict.
                let (Lookup::Found { dir, id }, _) = self.lookup(path)? else { return Err(Error::Corrupt) };
                (Loc { pair: dir.pair, id }, Content::Inline(Vec::new()), false)
            }
            Lookup::Found { dir, id } => {
                if o.create_new {
                    return Err(Error::Exists);
                }
                let content = match self.decode(&dir.c.entries[id as usize])? {
                    Struct::Dir(_) => return Err(Error::IsDir),
                    Struct::Inline(d) => Content::Inline(d),
                    Struct::Ctz { head, size } => Content::Ctz { head, size },
                };
                let loc = Loc { pair: dir.pair, id };
                if o.truncate { (loc, Content::Inline(Vec::new()), true) } else { (loc, content, false) }
            }
        };
        Ok(OpenFile {
            generation: 0,
            loc: Some(loc),
            name: name.to_vec(),
            read: o.read,
            write: o.write,
            pos: 0,
            content,
            writer: None,
            dirty,
            erred: None,
        })
    }

    /// Syncs (if open for writing) and forgets the handle, even if the sync fails. An errored
    /// handle commits nothing and reports its error.
    pub fn close(&mut self, h: FileHandle) -> Result<(), Error> {
        let i = self.slot(h)?;
        let f = self.file(i)?;
        let r = match (f.write, f.erred) {
            (true, Some(e)) => Err(e),
            (true, None) => self.mutate(|fs| fs.sync_file(i)),
            (false, _) => Ok(()),
        };
        self.files[i] = None;
        r
    }

    /// Makes everything written through the handle durable, in one metadata commit.
    pub fn sync(&mut self, h: FileHandle) -> Result<(), Error> {
        let i = self.slot(h)?;
        if !self.file(i)?.write {
            return Ok(());
        }
        let i = self.writable(h)?;
        self.on_file(i, |fs| fs.sync_file(i))
    }

    pub fn file_size(&mut self, h: FileHandle) -> Result<u32, Error> {
        let i = self.slot(h)?;
        Ok(self.file(i)?.size())
    }

    pub fn read(&mut self, h: FileHandle, buf: &mut [u8]) -> Result<usize, Error> {
        let i = self.slot(h)?;
        if !self.file(i)?.read {
            return Err(Error::Invalid);
        }
        if self.file(i)?.writer.is_some() {
            let i = self.writable(h)?;
            self.on_file(i, |fs| fs.flush(i))?;
        }
        self.check_poison()?;
        let f = self.file(i)?;
        let (pos, size) = (f.pos, f.size());
        if pos >= size {
            return Ok(0);
        }
        let n = buf.len().min((size - pos) as usize);
        self.read_content(i, pos, &mut buf[..n])?;
        self.file(i)?.pos += n as u32;
        Ok(n)
    }

    /// Writes at the handle's position. On failure (typically `NoSpace`) the handle is
    /// errored: nothing written through it is committed.
    pub fn write(&mut self, h: FileHandle, data: &[u8]) -> Result<usize, Error> {
        let i = self.writable(h)?;
        self.on_file(i, |fs| {
            let file_max = fs.file_max;
            let f = fs.file(i)?;
            if f.pos as u64 + data.len() as u64 > file_max as u64 {
                return Err(Error::FileTooBig);
            }
            // Writing past the end first fills the gap with zeros.
            let (target, size) = (f.pos, f.size());
            if target > size {
                f.pos = size;
                fs.write_zeros(i, target - size)?;
            }
            fs.write_data(i, data)?;
            Ok(data.len())
        })
    }

    /// Moves the handle's position (9P reads and writes carry absolute offsets). Past the
    /// end is allowed; a write there fills the gap with zeros.
    pub fn seek(&mut self, h: FileHandle, pos: u32) -> Result<(), Error> {
        let i = self.slot(h)?;
        if pos > self.file_max {
            return Err(Error::Invalid);
        }
        let f = self.file(i)?;
        if pos != f.pos && f.writer.is_some() {
            let i = self.writable(h)?;
            self.on_file(i, |fs| fs.flush(i))?;
        }
        self.file(i)?.pos = pos;
        Ok(())
    }

    /// Shrinks or zero-extends a file; the position is kept.
    pub fn truncate(&mut self, h: FileHandle, size: u32) -> Result<(), Error> {
        let i = self.writable(h)?;
        if size > self.file_max {
            return Err(Error::Invalid);
        }
        self.on_file(i, |fs| {
            let (pos, old) = (fs.file(i)?.pos, fs.file(i)?.size());
            fs.flush(i)?;
            if size < old {
                let small = size <= fs.inline_max;
                let content = match &fs.file(i)?.content {
                    // Small enough to live inline again.
                    _ if small => {
                        let mut v = vec![0u8; size as usize];
                        fs.read_content(i, 0, &mut v)?;
                        Content::Inline(v)
                    }
                    // The skip-list's prefix up to the new end is itself a valid skip-list.
                    &Content::Ctz { head, size: old } => {
                        let (block, _) = fs.ctz_find(head, old, size - 1)?;
                        Content::Ctz { head: block, size }
                    }
                    // An inline file longer than `inline_max` (another writer's settings).
                    Content::Inline(v) => Content::Inline(v[..size as usize].to_vec()),
                };
                let f = fs.file(i)?;
                f.content = content;
                f.dirty = true;
            } else if size > old {
                fs.file(i)?.pos = old;
                fs.write_zeros(i, size - old)?;
                fs.flush(i)?;
            }
            fs.file(i)?.pos = pos;
            Ok(())
        })
    }

    // ---- internals ----

    /// Reads `buf.len()` bytes at `pos` of the handle's flushed contents.
    fn read_content(&mut self, i: usize, pos: u32, buf: &mut [u8]) -> Result<(), Error> {
        match &self.file(i)?.content {
            Content::Inline(v) => {
                let src = v.get(pos as usize..pos as usize + buf.len()).ok_or(Error::Invalid)?;
                buf.copy_from_slice(src);
                Ok(())
            }
            &Content::Ctz { head, size } => self.read_ctz(head, size, pos, buf),
        }
    }

    /// Reads from a skip-list, one block at a time.
    fn read_ctz(&mut self, head: u32, size: u32, mut pos: u32, buf: &mut [u8]) -> Result<(), Error> {
        let mut done = 0;
        while done < buf.len() {
            let (block, off) = self.ctz_find(head, size, pos)?;
            let n = (buf.len() - done).min((self.block_size - off) as usize);
            self.bd_read(block, off, &mut buf[done..done + n])?;
            done += n;
            pos += n as u32;
        }
        Ok(())
    }

    fn write_zeros(&mut self, i: usize, mut n: u32) -> Result<(), Error> {
        let zeros = vec![0u8; self.block_size as usize];
        while n > 0 {
            let k = n.min(self.block_size);
            self.write_data(i, &zeros[..k as usize])?;
            n -= k;
        }
        Ok(())
    }

    /// Writes at the handle's position (which is at most the file's size).
    fn write_data(&mut self, i: usize, mut data: &[u8]) -> Result<(), Error> {
        // Writing nothing changes nothing (and must not make the handle commit its view).
        if data.is_empty() {
            return Ok(());
        }
        let (inline_max, bs) = (self.inline_max, self.block_size);
        let f = self.file(i)?;
        if let (Content::Inline(v), None) = (&mut f.content, &f.writer) {
            let end = f.pos as usize + data.len();
            if end.max(v.len()) <= inline_max as usize {
                if v.len() < end {
                    v.resize(end, 0);
                }
                v[f.pos as usize..end].copy_from_slice(data);
                f.pos = end as u32;
                f.dirty = true;
                return Ok(());
            }
            self.outline(i)?;
        }

        while !data.is_empty() {
            let f = self.file(i)?;
            let (pos, full) = (f.pos, f.writer.as_ref().map(|w| w.off == bs));
            if full != Some(false) {
                // Start a block: after the full one being written, or else after the block
                // holding the byte before the write position (copying its start).
                let prev = if full == Some(true) {
                    self.program_writer(i)?;
                    self.file(i)?.writer.as_ref().map(|w| w.block)
                } else if let (Content::Ctz { head, size }, true) = (&self.file(i)?.content, pos > 0) {
                    let (head, size) = (*head, *size);
                    Some(self.ctz_find(head, size, pos - 1)?.0)
                } else {
                    None
                };
                self.alloc_ckpoint();
                let w = self.extend(prev, pos)?;
                self.file(i)?.writer = Some(w);
            }
            let f = self.file(i)?;
            let w = f.writer.as_mut().ok_or(Error::Invalid)?;
            let n = data.len().min((bs - w.off) as usize);
            w.buf[w.off as usize..w.off as usize + n].copy_from_slice(&data[..n]);
            w.off += n as u32;
            f.pos += n as u32;
            data = &data[n..];
            // Everything allocated so far is reachable from this handle.
            self.alloc_ckpoint();
        }
        Ok(())
    }

    /// Starts moving an inline file that outgrows `inline_max` into blocks: its first block
    /// holds the bytes before the position; the inline contents stay as the old version, so
    /// the flush copies whatever lies after the write (an inline file can be longer than our
    /// `inline_max` if another writer made it).
    fn outline(&mut self, i: usize) -> Result<(), Error> {
        self.alloc_ckpoint();
        let block = self.alloc_block()?;
        let mut buf = vec![0xffu8; self.block_size as usize];
        let f = self.file(i)?;
        let Content::Inline(v) = &f.content else { return Ok(()) };
        let n = (f.pos as usize).min(v.len());
        buf[..n].copy_from_slice(&v[..n]);
        f.writer = Some(Writer { block, buf, off: n as u32 });
        Ok(())
    }

    /// Allocates the block that continues a skip-list of `size` bytes whose last block is
    /// `prev` (the reference's `lfs_ctz_extend`).
    fn extend(&mut self, prev: Option<u32>, size: u32) -> Result<Writer, Error> {
        let block = self.alloc_block()?;
        let bs = self.block_size;
        let mut buf = vec![0xffu8; bs as usize];
        let Some(prev) = prev.filter(|_| size > 0) else { return Ok(Writer { block, buf, off: 0 }) };
        let (index, off) = ctz::index(bs, size - 1);
        let used = off + 1;
        if used != bs {
            // The last block is partly full: the new block replaces it, starting as a copy.
            self.bd_read(prev, 0, &mut buf[..used as usize])?;
            return Ok(Writer { block, buf, off: used });
        }
        // A new block after a full one: pointer k goes 2^k blocks back, found as pointer k-1
        // of the block pointer k-1 names.
        let skips = ctz::skips(index + 1);
        let mut target = prev;
        for k in 0..skips {
            buf[4 * k as usize..4 * k as usize + 4].copy_from_slice(&target.to_le_bytes());
            if k + 1 < skips {
                target = self.read_pointer(target, 4 * k)?;
            }
        }
        Ok(Writer { block, buf, off: 4 * skips })
    }

    /// Programs the writer's block as it stands.
    fn program_writer(&mut self, i: usize) -> Result<(), Error> {
        let prog = self.prog_size;
        let w = self.file(i)?.writer.as_ref().ok_or(Error::Invalid)?;
        let (block, data) = (w.block, w.buf[..align_up(w.off, prog) as usize].to_vec());
        self.bd_erase(block)?;
        self.bd_prog(block, 0, &data)
    }

    /// Finishes a write: copies the rest of the old contents after it, programs the last
    /// block, and makes the new skip-list the handle's contents (not yet committed).
    fn flush(&mut self, i: usize) -> Result<(), Error> {
        let f = self.file(i)?;
        if f.writer.is_none() {
            return Ok(());
        }
        let saved = f.pos;
        let old_size = match &f.content {
            Content::Ctz { size, .. } => *size,
            Content::Inline(v) => v.len() as u32,
        };
        let mut tmp = vec![0u8; self.block_size as usize];
        loop {
            let pos = self.file(i)?.pos;
            if pos >= old_size {
                break;
            }
            let n = (old_size - pos).min(self.block_size) as usize;
            self.read_content(i, pos, &mut tmp[..n])?;
            self.write_data(i, &tmp[..n])?;
        }
        self.program_writer(i)?;
        let f = self.file(i)?;
        let w = f.writer.take().ok_or(Error::Invalid)?;
        f.content = Content::Ctz { head: w.block, size: f.pos };
        f.pos = saved;
        f.dirty = true;
        Ok(())
    }

    fn sync_file(&mut self, i: usize) -> Result<(), Error> {
        self.flush(i)?;
        let f = self.file(i)?;
        if !f.dirty {
            return Ok(());
        }
        let Some(loc) = f.loc else {
            f.dirty = false;
            return Ok(());
        };
        let name = f.name.clone();
        let attr = match &f.content {
            Content::Inline(v) => attr_struct(TYPE_INLINESTRUCT, loc.id, v)?,
            &Content::Ctz { head, size } => {
                attr_struct(TYPE_CTZSTRUCT, loc.id, &[head.to_le_bytes(), size.to_le_bytes()].concat())?
            }
        };
        // The handle must still name its own entry: committing into anything else would
        // overwrite another file. (Handles follow every commit; this is the backstop.)
        let dir = self.fetch(loc.pair)?;
        match dir.c.entries.get(loc.id as usize) {
            Some(e) if e.name_type == TYPE_REG && e.name == name => {}
            _ => return Err(Error::Corrupt),
        }
        // Data must be durable before the metadata that points at it.
        self.bd_sync()?;
        self.commit(loc.pair, &[attr])?;
        self.file(i)?.dirty = false;
        Ok(())
    }
}
