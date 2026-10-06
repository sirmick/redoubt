//! Open files: open, read, write, seek, truncate, sync, close. A write reaches the medium in the
//! call that makes it, one transaction per [`crate::LOG_BLOCKS`]' worth of blocks.

use crate::fs::Filesystem;
use crate::layout::{KIND_DIR, KIND_FILE};
use crate::ops::components;
use crate::{BLOCK, BlockDevice, Error, MAX_FILE_SIZE};

/// An open file, as [`Filesystem::open`] returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileHandle(u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    /// Create the file if it does not exist; needs `write`.
    pub create: bool,
    /// Cut the file to nothing; needs `write`.
    pub truncate: bool,
}

/// What a handle holds: its inode, which follows renames and outlives removal, and its position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Open {
    pub ino: u32,
    pos: u64,
    read: bool,
    write: bool,
}

/// Room a write needs for one more data block: the block, a single- and a double-indirect block,
/// and up to three of the bitmap's blocks, each with its hash block; and the inode with its own.
const WRITE_STEP: usize = 14;

impl<D: BlockDevice> Filesystem<D> {
    fn handle(&self, h: FileHandle) -> Result<Open, Error> {
        self.handles.get(h.0 as usize).copied().flatten().ok_or(Error::Invalid)
    }

    fn set_pos(&mut self, h: FileHandle, pos: u64) {
        if let Some(Some(o)) = self.handles.get_mut(h.0 as usize) {
            o.pos = pos;
        }
    }

    pub fn open(&mut self, path: &str, o: OpenOptions) -> Result<FileHandle, Error> {
        self.op(|fs| {
            if (o.create || o.truncate) && !o.write {
                return Err(Error::Invalid);
            }
            let names = components(path)?;
            if names.is_empty() {
                return Err(Error::IsDir);
            }
            let (_, _, s) = fs.parent(&names)?;
            let ino = match s.found {
                Some(slot) => {
                    let mut n = fs.inode(slot.ino)?;
                    if n.kind == KIND_DIR {
                        return Err(Error::IsDir);
                    }
                    if o.truncate && n.size > 0 {
                        n.size = 0;
                        n.mtime = fs.now;
                        fs.put_inode(slot.ino, &n)?;
                        fs.trim(slot.ino)?;
                    }
                    slot.ino
                }
                None if o.create => fs.create(&names, KIND_FILE)?,
                None => return Err(Error::NoEntry),
            };
            let open = Some(Open { ino, pos: 0, read: o.read, write: o.write });
            let k = match fs.handles.iter().position(Option::is_none) {
                Some(k) => {
                    fs.handles[k] = open;
                    k
                }
                None => {
                    fs.handles.push(open);
                    fs.handles.len() - 1
                }
            };
            Ok(FileHandle(k as u32))
        })
    }

    /// Closes the handle; a removed file's blocks are freed with its last handle.
    pub fn close(&mut self, h: FileHandle) -> Result<(), Error> {
        let o = self.handle(h)?;
        self.handles[h.0 as usize] = None;
        self.op(|fs| {
            if !fs.is_open(o.ino) && fs.inode(o.ino)?.nlink == 0 {
                fs.finish(o.ino)?;
            }
            Ok(())
        })
    }

    /// Every write is on the medium when it returns, so this only checks the handle.
    pub fn sync(&mut self, h: FileHandle) -> Result<(), Error> { self.handle(h).map(|_| ()) }

    pub fn file_size(&mut self, h: FileHandle) -> Result<u64, Error> {
        let o = self.handle(h)?;
        self.op(|fs| Ok(fs.inode(o.ino)?.size))
    }

    pub fn seek(&mut self, h: FileHandle, pos: u64) -> Result<(), Error> {
        self.handle(h)?;
        if pos > MAX_FILE_SIZE {
            return Err(Error::FileTooBig);
        }
        self.set_pos(h, pos);
        Ok(())
    }

    /// Reads from the handle's position up to the file's size; holes read as zeros.
    pub fn read(&mut self, h: FileHandle, buf: &mut [u8]) -> Result<usize, Error> {
        let o = self.handle(h)?;
        if !o.read {
            return Err(Error::Invalid);
        }
        let got = self.op(|fs| {
            let n = fs.inode(o.ino)?;
            let len = (n.size.saturating_sub(o.pos)).min(buf.len() as u64) as usize;
            let mut done = 0;
            while done < len {
                let at = o.pos + done as u64;
                let (idx, within) = (at / BLOCK as u64, (at % BLOCK as u64) as usize);
                let take = (BLOCK - within).min(len - done);
                match fs.bmap(&n, idx)? {
                    0 => buf[done..done + take].fill(0),
                    a => buf[done..done + take].copy_from_slice(&fs.block(a)?[within..within + take]),
                }
                done += take;
            }
            Ok(done)
        })?;
        self.set_pos(h, o.pos + got as u64);
        Ok(got)
    }

    /// Writes `data` at the handle's position: one transaction while it fits one, several if not,
    /// each whole. A volume that fills part way ends the write at the last block it had room for,
    /// and the count written is returned; if there was room for none, `NoSpace`.
    pub fn write(&mut self, h: FileHandle, data: &[u8]) -> Result<usize, Error> {
        let o = self.handle(h)?;
        if !o.write {
            return Err(Error::Invalid);
        }
        if o.pos + data.len() as u64 > MAX_FILE_SIZE {
            return Err(Error::FileTooBig);
        }
        let wrote = self.op(|fs| {
            let mut n = fs.inode(o.ino)?;
            let mut done = 0;
            while done < data.len() {
                fs.make_room(o.ino, &n, WRITE_STEP)?;
                let at = o.pos + done as u64;
                let (idx, within) = (at / BLOCK as u64, (at % BLOCK as u64) as usize);
                let take = (BLOCK - within).min(data.len() - done);
                // A volume too full for this block ends the write here, with what it wrote.
                if fs.alloc_need(&n, idx)? > fs.free_blocks() {
                    break;
                }
                let (a, fresh) = fs.bmap_alloc(&mut n, idx)?;
                let buf = if fresh || take == BLOCK { fs.tx_new(a)? } else { fs.tx_get(a)? };
                buf[within..within + take].copy_from_slice(&data[done..done + take]);
                done += take;
                n.size = n.size.max(at + take as u64);
            }
            if done == 0 && !data.is_empty() {
                return Err(Error::NoSpace);
            }
            n.mtime = fs.now;
            fs.put_inode(o.ino, &n)?;
            Ok(done)
        })?;
        self.set_pos(h, o.pos + wrote as u64);
        Ok(wrote)
    }

    /// Sets the file's size: growing it reads as zeros, cutting it frees what lies past the end.
    pub fn truncate(&mut self, h: FileHandle, size: u64) -> Result<(), Error> {
        let o = self.handle(h)?;
        if !o.write {
            return Err(Error::Invalid);
        }
        if size > MAX_FILE_SIZE {
            return Err(Error::FileTooBig);
        }
        self.op(|fs| {
            let mut n = fs.inode(o.ino)?;
            let shrink = size < n.size;
            if shrink && !size.is_multiple_of(BLOCK as u64) {
                let a = fs.bmap(&n, size / BLOCK as u64)?;
                if a != 0 {
                    fs.tx_get(a)?[(size % BLOCK as u64) as usize..].fill(0);
                }
            }
            n.size = size;
            n.mtime = fs.now;
            fs.put_inode(o.ino, &n)?;
            if shrink { fs.trim(o.ino) } else { Ok(()) }
        })
    }
}
