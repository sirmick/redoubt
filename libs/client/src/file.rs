//! Files over 9P on a connection: walk, open, create, read, write, stat, read a directory, remove.
//!
//! A [`Connection`] is shared: cloning it is the same connection (one endpoint, one badge), its
//! root is attached once, and its fids come from one allocator, so threads open files on it at
//! once, each lending its own [`Lend`]. A fid goes back to the allocator only once the server has
//! let it go (its `Rclunk`, or the `Rerror` a clunk or remove also ends it with); a fid whose fate
//! is unknown (the call timed out) is never reused, and nor is the fid of a [`File`] dropped
//! without [`File::close`], which makes no call behind its caller's back. Both stay in use until
//! the connection ends.
//!
//! Nothing is buffered or cached: one read or write is one request of at most the lend's
//! `iounit`, and every open walks from the root, so a rename, a removal or a revoked connection
//! shows on the next open.

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use redoubt_rt::client::{self, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::MAX_FIDS;
use redoubt_rt::wire::ninep::{self, Qid};

use crate::error::{Error, Refusal};

/// The root's fid, attached when the connection is made.
const ROOT: u32 = 0;
/// Words of the fid bitmap: 32-bit, so rv32 has the atomics.
const WORDS: usize = MAX_FIDS.div_ceil(32);

/// The connection's fids: a bit per fid, set while it is in use.
struct Fids([AtomicU32; WORDS]);

impl Fids {
    fn new() -> Fids {
        let fids = Fids([const { AtomicU32::new(0) }; WORDS]);
        fids.0[0].store(1 << ROOT, Ordering::Relaxed);
        fids
    }

    /// A free fid, now in use.
    fn take(&self) -> Result<u32, Error> {
        for (i, word) in self.0.iter().enumerate() {
            let mut bits = word.load(Ordering::Relaxed);
            while bits != u32::MAX {
                let bit = bits.trailing_ones();
                let fid = i as u32 * 32 + bit;
                if fid as usize >= MAX_FIDS {
                    break;
                }
                match word.compare_exchange_weak(bits, bits | 1 << bit, Ordering::AcqRel, Ordering::Relaxed) {
                    Ok(_) => return Ok(fid),
                    Err(now) => bits = now,
                }
            }
        }
        Err(Refusal::NoFid.into())
    }

    fn free(&self, fid: u32) { self.0[fid as usize / 32].fetch_and(!(1 << (fid % 32)), Ordering::AcqRel); }
}

struct Inner {
    nine: client::Connection,
    fids: Fids,
}

/// A 9P connection, shared by clones and threads.
#[derive(Clone)]
pub struct Connection(Arc<Inner>);

impl Connection {
    /// A session on `endpoint` (`Tversion`) with its root attached. One per endpoint handle: a
    /// second `Tversion` on the same handle would end the first's fids (intro(5)).
    pub fn attach(endpoint: Endpoint, lend: &mut Lend) -> Result<Connection, Error> {
        let nine = client::Connection::new(endpoint);
        nine.version(lend)?;
        nine.attach(lend, ROOT, "")?;
        Ok(Connection(Arc::new(Inner { nine, fids: Fids::new() })))
    }

    /// The connection's endpoint: for typed calls on the same server, and `new_connection`.
    pub fn endpoint(&self) -> &Endpoint { self.0.nine.endpoint() }

    /// Whether `other` is this same connection.
    pub fn same(&self, other: &Connection) -> bool { Arc::ptr_eq(&self.0, &other.0) }

    /// Opens `path` (from the root; `..` never climbs above it) with `mode`.
    pub fn open(&self, lend: &mut Lend, path: &str, mode: u8) -> Result<File, Error> {
        let (fid, _) = self.walk(lend, path)?;
        match self.0.nine.open(lend, fid, mode) {
            Ok(qid) => Ok(File { conn: self.clone(), fid, qid }),
            Err(e) => {
                let _ = self.clunk(lend, fid);
                Err(e.into())
            }
        }
    }

    /// Creates `name` in the directory `dir` with `perm`, and opens it with `mode`.
    pub fn create(&self, lend: &mut Lend, dir: &str, name: &str, perm: u32, mode: u8) -> Result<File, Error> {
        let (fid, _) = self.walk(lend, dir)?;
        match self.0.nine.create(lend, fid, name, perm, mode) {
            Ok(qid) => Ok(File { conn: self.clone(), fid, qid }),
            Err(e) => {
                let _ = self.clunk(lend, fid);
                Err(e.into())
            }
        }
    }

    /// The directory entry of `path`.
    pub fn stat(&self, lend: &mut Lend, path: &str) -> Result<Stat, Error> {
        let (fid, _) = self.walk(lend, path)?;
        let stat = self.0.nine.stat(lend, fid).map(|s| Stat::from(&s));
        let clunked = self.clunk(lend, fid);
        let stat = stat?;
        clunked.map(|()| stat)
    }

    /// Removes `path`.
    pub fn remove(&self, lend: &mut Lend, path: &str) -> Result<(), Error> {
        let (fid, _) = self.walk(lend, path)?;
        let removed = self.0.nine.remove(lend, fid);
        // A remove clunks its fid whether or not the file went (intro(5)).
        self.settle(fid, &removed);
        Ok(removed?)
    }

    /// `new_connection`: a fresh connection to this server rooted at `root` (below this one's; it
    /// never climbs above it), with `quota` bytes carved from this connection's (0 shares it).
    /// Returns its endpoint and the id only this connection may disconnect it by.
    pub fn new_connection(&self, lend: &mut Lend, root: &str, quota: u64) -> Result<(Endpoint, u64), Error> {
        Ok(self.0.nine.new_connection(lend, root, quota)?)
    }

    /// `disconnect`: frees the connection with `id` and everything minted under it, waiting at
    /// most `timeout` µs for the server's answer.
    pub fn disconnect(&self, id: u64, timeout: u64) -> Result<(), Error> {
        Ok(self.0.nine.disconnect(id, timeout)?)
    }

    /// A free fid walked from the root along `path`. A failed walk leaves the fid unused
    /// (intro(5)), so it goes back unless the walk's fate is unknown.
    fn walk(&self, lend: &mut Lend, path: &str) -> Result<(u32, Qid), Error> {
        let fid = self.0.fids.take()?;
        let walked = self.0.nine.walk(lend, ROOT, fid, path);
        if walked.is_err() {
            self.settle(fid, &walked);
        }
        Ok((fid, walked?))
    }

    fn clunk(&self, lend: &mut Lend, fid: u32) -> Result<(), Error> {
        let clunked = self.0.nine.clunk(lend, fid);
        self.settle(fid, &clunked);
        Ok(clunked?)
    }

    /// Frees `fid` once the server has answered the request that ends it (a success or an
    /// `Rerror`), or once the request was refused before it was sent (a path that does not clean
    /// or does not encode, or a lend whose pages could not be mapped), so an untrusted path cannot
    /// drain the connection's fids. Anything else leaves it in use for good.
    fn settle<T>(&self, fid: u32, result: &Result<T, client::ClientError>) {
        match result {
            Ok(_) | Err(client::ClientError::Remote) => self.0.fids.free(fid),
            Err(e) if e.unsent() => self.0.fids.free(fid),
            Err(_) => {}
        }
    }
}

/// An open file: one fid, and its connection for as long as it is open. It keeps no offset and
/// buffers nothing. End it with [`File::close`]; dropped instead, its fid stays in use until the
/// connection ends.
pub struct File {
    conn: Connection,
    fid: u32,
    qid: Qid,
}

impl File {
    pub fn connection(&self) -> &Connection { &self.conn }

    /// The fid it rests on: what `fsd`'s typed operations name.
    pub fn fid(&self) -> u32 { self.fid }

    pub fn qid(&self) -> Qid { self.qid }

    /// Reads at most `out.len()` bytes, and at most the lend's `iounit`, at `offset`: one request.
    pub fn read_at(&self, lend: &mut Lend, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        Ok(self.conn.0.nine.read(lend, self.fid, offset, out)?)
    }

    /// Writes at most the lend's `iounit` bytes of `data` at `offset`: one request. Returns how
    /// many the server took.
    pub fn write_at(&self, lend: &mut Lend, offset: u64, data: &[u8]) -> Result<usize, Error> {
        Ok(self.conn.0.nine.write(lend, self.fid, offset, data)?)
    }

    pub fn stat(&self, lend: &mut Lend) -> Result<Stat, Error> {
        Ok(self.conn.0.nine.stat(lend, self.fid).map(|s| Stat::from(&s))?)
    }

    /// One read of a directory opened for reading, at `offset`: its whole entries, and the offset
    /// of the next read. No entries is the end.
    pub fn read_dir(&self, lend: &mut Lend, offset: u64) -> Result<(Vec<Stat>, u64), Error> {
        let mut data = vec![0u8; lend.iounit()];
        let n = self.read_at(lend, offset, &mut data)?;
        let entries =
            ninep::stats(&data[..n]).map(|s| s.map(|s| Stat::from(&s))).collect::<Result<_, _>>()?;
        Ok((entries, offset + n as u64))
    }

    /// Clunks the fid; it goes back to the connection once the server has let it go.
    pub fn close(self, lend: &mut Lend) -> Result<(), Error> { self.conn.clunk(lend, self.fid) }
}

/// A directory entry, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub qid: Qid,
    pub mode: u32,
    pub atime: u32,
    pub mtime: u32,
    pub length: u64,
    pub name: String,
    pub uid: String,
    pub gid: String,
    pub muid: String,
}

impl From<&ninep::Stat<'_>> for Stat {
    fn from(s: &ninep::Stat<'_>) -> Stat {
        Stat {
            qid: s.qid,
            mode: s.mode,
            atime: s.atime,
            mtime: s.mtime,
            length: s.length,
            name: s.name.to_string(),
            uid: s.uid.to_string(),
            gid: s.gid.to_string(),
            muid: s.muid.to_string(),
        }
    }
}
