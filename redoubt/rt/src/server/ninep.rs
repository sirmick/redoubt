//! A 9P2000 server skeleton (NAMESPACES.md): it keeps the protocol state (connections, fids,
//! paths, open modes, directory offsets) and applies every rule that does not depend on what the
//! files are; a [`FileServer`] supplies the files.
//!
//! **On the wire.** A 9P request is a `call` whose words are all zero ([`WORDS_9P`]) with the
//! T-message at the start of its lend; the reply's words are all zero and the R-message is
//! written at the start of the same lend. A call with other words, or with no lend, gets
//! [`NO_MESSAGE`] and nothing in the lend. Handles sent with a 9P call are closed unread.
//!
//! **What the skeleton guarantees a [`FileServer`]**, whatever the client sends:
//! - Connections are keyed by badge (one connection = one endpoint handle); fids are per connection, at most
//!   [`MAX_FIDS`] each, and each fid is charged to its creator's account ([`Admission`],
//!   [`Resource::Files`]).
//! - Every fid is looked up; no request reaches the server for a fid that does not exist.
//! - Walk names are valid path components ([`path::valid_name`]); `..` is resolved lexically against the
//!   fid's path from its attach root, by re-walking from the root, so it never climbs above it, and a fid is
//!   never more than [`path::MAX_DEPTH`] below its root.
//! - Only directories are walked from or created in; directories are opened only for reading; reads need a
//!   fid opened for reading and writes one opened for writing.
//! - [`check`] on every request: `Read` for attach, walk, stat, read and opening for reading; `Write` for
//!   writing, create, remove and opening for writing or truncation.
//! - `offset + count` never overflows; a read asks for at most what fits the caller's lend and the msize; a
//!   directory read continues only from where the last one ended.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use redoubt_sys::Error;
use redoubt_wire::MSIZE;
use redoubt_wire::codec::Writer;
pub use redoubt_wire::ninep::Qid;
use redoubt_wire::ninep::{Body, IOHDRSZ, Message, NOFID, NOTAG, Qids, Stat, VERSION};

use super::admit::{Admission, AdmitKey, Limits, Resource};
use super::label::{Access, check};
use crate::ipc::{Caller, Request, Words};
use crate::path;

/// The words of a 9P request and of its reply.
pub const WORDS_9P: Words = [0; 4];
/// The words of a reply that carries no R-message: the call was not a 9P request.
pub const NO_MESSAGE: Words = [1, 0, 0, 0];
/// Fids per connection.
pub const MAX_FIDS: usize = 64;
/// `Qid::kind` of a directory.
pub const QTDIR: u8 = 0x80;
/// `Stat::mode` bit of a directory.
pub const DMDIR: u32 = 0x8000_0000;

/// Open modes (intro(5)): the access in the low two bits, then flags.
pub mod mode {
    pub const OREAD: u8 = 0;
    pub const OWRITE: u8 = 1;
    pub const ORDWR: u8 = 2;
    pub const OEXEC: u8 = 3;
    pub const OTRUNC: u8 = 0x10;
}

/// What an `Rerror` says: a fixed text, so a hostile request cannot choose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NineError(pub &'static str);

impl NineError {
    pub const BAD_MESSAGE: NineError = NineError("malformed message");
    pub const BAD_MODE: NineError = NineError("bad open mode");
    pub const BAD_NAME: NineError = NineError("bad file name");
    pub const BAD_OFFSET: NineError = NineError("bad offset");
    pub const FID_IN_USE: NineError = NineError("fid already in use");
    pub const IS_OPEN: NineError = NineError("fid is open");
    pub const NOT_DIR: NineError = NineError("not a directory");
    pub const NOT_FOUND: NineError = NineError("file does not exist");
    pub const NOT_OPEN: NineError = NineError("fid not open for this");
    pub const NOT_SUPPORTED: NineError = NineError("not supported");
    pub const NO_AUTH: NineError = NineError("authentication not required");
    pub const PERMISSION: NineError = NineError("permission denied");
    pub const TOO_DEEP: NineError = NineError("path too deep");
    pub const TOO_MANY: NineError = NineError("too many open files");
    pub const TOO_SMALL: NineError = NineError("count too small");
    pub const UNKNOWN_FID: NineError = NineError("unknown fid");
}

/// What `Tstat` and directory reads report about a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub qid: Qid,
    /// Permission bits (informational: access is by capability) and [`DMDIR`].
    pub mode: u32,
    pub mtime: u32,
    pub length: u64,
    pub name: String,
}

impl FileStat {
    fn wire(&self) -> Stat<'_> {
        Stat {
            kind: 0,
            dev: 0,
            qid: self.qid,
            mode: self.mode,
            atime: self.mtime,
            mtime: self.mtime,
            length: self.length,
            name: &self.name,
            uid: "",
            gid: "",
            muid: "",
        }
    }
}

/// The files. Every method is called only as the module docs promise; each one still treats
/// offsets and data as the client's.
pub trait FileServer {
    /// A file or directory the server can find again: what a fid refers to.
    type Node: Clone;

    /// The root of a new attach through the caller's badge (which grant it is).
    fn attach(&mut self, caller: &Caller, aname: &str) -> Result<(Self::Node, Qid), NineError>;

    /// The labels of the object `node` belongs to (for `fsd`, its volume's).
    fn labels(&self, node: &Self::Node) -> &[u64];

    /// The entry `name` (a valid component, never `.` or `..`) of the directory `dir`.
    fn walk(&mut self, caller: &Caller, dir: &Self::Node, name: &str)
    -> Result<(Self::Node, Qid), NineError>;

    /// `node` is being opened with `mode` (already checked against its kind and labels).
    fn open(&mut self, caller: &Caller, node: &Self::Node, mode: u8) -> Result<Qid, NineError>;

    /// Reads at most `out.len()` bytes of the file at `offset`.
    fn read(
        &mut self,
        caller: &Caller,
        node: &Self::Node,
        offset: u64,
        out: &mut [u8],
    ) -> Result<usize, NineError>;

    /// Writes `data` at `offset`; returns how much was written (at most `data.len()`).
    fn write(
        &mut self,
        caller: &Caller,
        node: &Self::Node,
        offset: u64,
        data: &[u8],
    ) -> Result<usize, NineError>;

    fn stat(&mut self, caller: &Caller, node: &Self::Node) -> Result<FileStat, NineError>;

    /// The `index`th entry of the directory `dir`, or `None` past the last.
    fn dir_entry(
        &mut self,
        caller: &Caller,
        dir: &Self::Node,
        index: u64,
    ) -> Result<Option<FileStat>, NineError>;

    /// Creates `name` in `dir` and opens it with `mode`.
    fn create(
        &mut self,
        _caller: &Caller,
        _dir: &Self::Node,
        _name: &str,
        _perm: u32,
        _mode: u8,
    ) -> Result<(Self::Node, Qid), NineError> {
        Err(NineError::NOT_SUPPORTED)
    }

    fn remove(&mut self, _caller: &Caller, _node: &Self::Node) -> Result<(), NineError> {
        Err(NineError::NOT_SUPPORTED)
    }

    /// A fid on `node` went away: clunked, removed, or reset by `Tversion`.
    fn clunk(&mut self, _node: &Self::Node) {}
}

/// One fid's state.
struct Fid<N> {
    /// Whose account it is charged to.
    owner: AdmitKey,
    root: (N, Qid),
    /// Components from the root to `node`: the lexical path `..` is resolved against.
    path: Vec<String>,
    node: N,
    qid: Qid,
    /// The mode it was opened with, if it is open.
    open: Option<u8>,
    /// Where the next directory read continues: (byte offset, entry index).
    dir_next: (u64, u64),
}

impl<N> Fid<N> {
    fn is_dir(&self) -> bool { self.qid.kind & QTDIR != 0 }
}

/// What a request comes to, before it is written back as an R-message.
enum Answer {
    /// The msize, and whether the version is ours (else the reply says "unknown").
    Version(u32, bool),
    Attach(Qid),
    Walk(Vec<Qid>),
    Open(Qid),
    Create(Qid),
    /// The data is the first n bytes of the scratch buffer.
    Read(usize),
    Write(u32),
    Clunk,
    Remove,
    Stat(FileStat),
    Flush,
}

/// Serves 9P for a [`FileServer`].
pub struct NineServer<S: FileServer> {
    pub fs: S,
    /// Fid tables by badge; a connection with no fids has no entry.
    conns: BTreeMap<u64, BTreeMap<u32, Fid<S::Node>>>,
    admission: Admission,
    /// Where read data is gathered before it is written into the lend: the request is decoded
    /// from the lend, so the reply cannot be built there until the request is done with.
    scratch: Vec<u8>,
}

impl<S: FileServer> NineServer<S> {
    /// Serves `fs`; `limits.files` bounds the fids one account holds across all connections.
    pub fn new(fs: S, limits: Limits) -> NineServer<S> {
        NineServer { fs, conns: BTreeMap::new(), admission: Admission::new(limits), scratch: Vec::new() }
    }

    /// Fids open on the connection with `badge`.
    pub fn fids(&self, badge: u64) -> usize { self.conns.get(&badge).map_or(0, BTreeMap::len) }

    /// Handles one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        // Handles are no part of 9P; closing them keeps a client from filling our handle table.
        for handle in request.handles.as_slice() {
            let _ = crate::handle::close(*handle);
        }
        if request.words != WORDS_9P {
            return request.reply(&NO_MESSAGE, &[]);
        }
        let caller = request.caller;
        let words = match self.answer_in_place(&caller, request.lend()) {
            Some(()) => WORDS_9P,
            None => NO_MESSAGE,
        };
        request.reply(&words, &[])
    }

    /// Reads the T-message at the front of `lend` and writes the R-message over it; `None` if
    /// there is not even room for an `Rerror`.
    pub fn answer_in_place(&mut self, caller: &Caller, lend: &mut [u8]) -> Option<()> {
        // Room for the reply: the lend, within the msize. Read data is bounded by it.
        let room = lend.len().min(MSIZE);
        let (tag, answer) = match Message::decode(lend) {
            Ok(message) => (message.tag, self.answer(caller, message.body, room)),
            // The tag, if the header is there, so the client can match the error.
            Err(_) => {
                let tag = lend.get(5..7).map_or(NOTAG, |t| u16::from_le_bytes([t[0], t[1]]));
                (tag, Err(NineError::BAD_MESSAGE))
            }
        };
        let lend = &mut lend[..room];
        let written = match &answer {
            Ok(answer) => self.encode(tag, answer, lend),
            Err(e) => Message { tag, body: Body::Rerror { ename: e.0 } }.encode(lend),
        };
        match written {
            Ok(_) => Some(()),
            // The answer did not fit the lend: say so if even that fits.
            Err(_) => {
                Message { tag, body: Body::Rerror { ename: "reply too large" } }.encode(lend).ok().map(|_| ())
            }
        }
    }

    fn encode(&self, tag: u16, answer: &Answer, out: &mut [u8]) -> Result<usize, redoubt_wire::Error> {
        let body = match answer {
            Answer::Version(msize, known) => {
                Body::Rversion { msize: *msize, version: if *known { VERSION } else { "unknown" } }
            }
            Answer::Attach(qid) => Body::Rattach { qid: *qid },
            Answer::Walk(qids) => Body::Rwalk { qids: Qids::new(qids)? },
            Answer::Open(qid) => Body::Ropen { qid: *qid, iounit: 0 },
            Answer::Create(qid) => Body::Rcreate { qid: *qid, iounit: 0 },
            Answer::Read(n) => Body::Rread { data: self.scratch.get(..*n).unwrap_or(&[]) },
            Answer::Write(count) => Body::Rwrite { count: *count },
            Answer::Clunk => Body::Rclunk,
            Answer::Remove => Body::Rremove,
            Answer::Stat(stat) => Body::Rstat { stat: stat.wire() },
            Answer::Flush => Body::Rflush,
        };
        Message { tag, body }.encode(out)
    }

    fn answer(&mut self, caller: &Caller, body: Body<'_>, room: usize) -> Result<Answer, NineError> {
        match body {
            Body::Tversion { msize, version } => {
                // A new session on this connection: every fid on it goes (intro(5), version).
                if let Some(fids) = self.conns.remove(&caller.badge) {
                    for (_, fid) in fids {
                        self.drop_fid(fid);
                    }
                }
                // A version we do not speak is answered "unknown" (intro(5), version).
                Ok(Answer::Version(msize.min(MSIZE as u32), version.starts_with(VERSION)))
            }
            Body::Tauth { .. } => Err(NineError::NO_AUTH),
            Body::Tattach { fid, afid, aname, .. } => {
                if afid != NOFID {
                    return Err(NineError::NO_AUTH);
                }
                self.check_new_fid(caller, fid)?;
                let (node, qid) = self.fs.attach(caller, aname)?;
                self.check_labels(caller, &node, Access::Read)?;
                let root = (node.clone(), qid);
                let fid_state = Fid {
                    owner: AdmitKey::of(caller),
                    root,
                    path: Vec::new(),
                    node,
                    qid,
                    open: None,
                    dir_next: (0, 0),
                };
                self.insert_fid(caller, fid, fid_state)?;
                Ok(Answer::Attach(qid))
            }
            Body::Twalk { fid, newfid, wnames } => self.walk(caller, fid, newfid, wnames.as_slice()),
            Body::Topen { fid, mode } => {
                let f = self.fid(caller, fid)?;
                if f.open.is_some() {
                    return Err(NineError::IS_OPEN);
                }
                let access = open_access(mode, f.is_dir())?;
                let node = f.node.clone();
                for a in access.iter().flatten() {
                    self.check_labels(caller, &node, *a)?;
                }
                let qid = self.fs.open(caller, &node, mode)?;
                let f = self.fid_mut(caller, fid)?;
                f.open = Some(mode);
                f.dir_next = (0, 0);
                Ok(Answer::Open(qid))
            }
            Body::Tcreate { fid, name, perm, mode } => {
                let f = self.fid(caller, fid)?;
                if f.open.is_some() {
                    return Err(NineError::IS_OPEN);
                }
                if !f.is_dir() {
                    return Err(NineError::NOT_DIR);
                }
                if !path::valid_name(name) {
                    return Err(NineError::BAD_NAME);
                }
                if f.path.len() >= path::MAX_DEPTH {
                    return Err(NineError::TOO_DEEP);
                }
                open_access(mode, perm & DMDIR != 0)?;
                let dir = f.node.clone();
                self.check_labels(caller, &dir, Access::Write)?;
                let (node, qid) = self.fs.create(caller, &dir, name, perm, mode)?;
                let f = self.fid_mut(caller, fid)?;
                f.path.push(name.to_string());
                f.node = node;
                f.qid = qid;
                f.open = Some(mode);
                f.dir_next = (0, 0);
                Ok(Answer::Create(qid))
            }
            Body::Tread { fid, offset, count } => self.read(caller, fid, offset, count, room),
            Body::Twrite { fid, offset, data } => {
                let f = self.fid(caller, fid)?;
                if !f.open.is_some_and(|m| matches!(m & 3, mode::OWRITE | mode::ORDWR)) {
                    return Err(NineError::NOT_OPEN);
                }
                offset.checked_add(data.len() as u64).ok_or(NineError::BAD_OFFSET)?;
                let node = f.node.clone();
                self.check_labels(caller, &node, Access::Write)?;
                let n = self.fs.write(caller, &node, offset, data)?;
                // A server claiming more than it was given is a bug; do not pass it on.
                let n = u32::try_from(n)
                    .ok()
                    .filter(|n| *n as usize <= data.len())
                    .ok_or(NineError::BAD_MESSAGE)?;
                Ok(Answer::Write(n))
            }
            Body::Tclunk { fid } => {
                let f = self.remove_fid(caller, fid)?;
                self.drop_fid(f);
                Ok(Answer::Clunk)
            }
            Body::Tremove { fid } => {
                // The fid goes whether or not the remove succeeds (intro(5), remove).
                let f = self.remove_fid(caller, fid)?;
                let result = self
                    .check_labels(caller, &f.node, Access::Write)
                    .and_then(|()| self.fs.remove(caller, &f.node));
                self.drop_fid(f);
                result.map(|()| Answer::Remove)
            }
            Body::Tstat { fid } => {
                let node = self.fid(caller, fid)?.node.clone();
                self.check_labels(caller, &node, Access::Read)?;
                Ok(Answer::Stat(self.fs.stat(caller, &node)?))
            }
            Body::Twstat { .. } => Err(NineError::NOT_SUPPORTED),
            // Requests are answered one at a time, so none is ever in flight to flush.
            Body::Tflush { .. } => Ok(Answer::Flush),
            // R-messages travel only from servers.
            _ => Err(NineError::BAD_MESSAGE),
        }
    }

    fn walk(&mut self, caller: &Caller, fid: u32, newfid: u32, names: &[&str]) -> Result<Answer, NineError> {
        let f = self.fid(caller, fid)?;
        if f.open.is_some() {
            return Err(NineError::IS_OPEN);
        }
        if newfid != fid {
            self.check_new_fid(caller, newfid)?;
        }
        let (root, mut path, mut node, mut qid) = (f.root.clone(), f.path.clone(), f.node.clone(), f.qid);
        let mut qids = Vec::new();
        for name in names {
            let step = self.step(caller, &root, &mut path, &node, qid, name);
            match step {
                Ok((next, next_qid)) => {
                    node = next;
                    qid = next_qid;
                    qids.push(qid);
                }
                // The first name failing is an error; a later one ends the walk short, and
                // `newfid` is not changed (intro(5), walk).
                Err(e) if qids.is_empty() => return Err(e),
                Err(_) => return Ok(Answer::Walk(qids)),
            }
        }
        let owner = AdmitKey::of(caller);
        let walked = Fid { owner, root, path, node, qid, open: None, dir_next: (0, 0) };
        if newfid == fid {
            let f = self.fid_mut(caller, fid)?;
            // Keep the original owner: the fid's charge stays where it was made.
            *f = Fid { owner: f.owner, ..walked };
        } else {
            self.insert_fid(caller, newfid, walked)?;
        }
        Ok(Answer::Walk(qids))
    }

    /// One walk step from `node` (at `path`, with `qid`) by `name`; updates `path`.
    fn step(
        &mut self,
        caller: &Caller,
        root: &(S::Node, Qid),
        path: &mut Vec<String>,
        node: &S::Node,
        qid: Qid,
        name: &str,
    ) -> Result<(S::Node, Qid), NineError> {
        if qid.kind & QTDIR == 0 {
            return Err(NineError::NOT_DIR);
        }
        self.check_labels(caller, node, Access::Read)?;
        if name == ".." {
            // Lexically: drop the last component and walk the rest again from the root, so the
            // server is never asked for a parent and the root is as high as it goes.
            let mut up = path.clone();
            up.pop();
            let (mut node, mut qid) = root.clone();
            for component in &up {
                self.check_labels(caller, &node, Access::Read)?;
                (node, qid) = self.fs.walk(caller, &node, component)?;
            }
            *path = up;
            return Ok((node, qid));
        }
        if !path::valid_name(name) {
            return Err(NineError::BAD_NAME);
        }
        if path.len() >= path::MAX_DEPTH {
            return Err(NineError::TOO_DEEP);
        }
        let next = self.fs.walk(caller, node, name)?;
        path.push(name.to_string());
        Ok(next)
    }

    fn read(
        &mut self,
        caller: &Caller,
        fid: u32,
        offset: u64,
        count: u32,
        room: usize,
    ) -> Result<Answer, NineError> {
        let f = self.fid(caller, fid)?;
        if !f.open.is_some_and(|m| matches!(m & 3, mode::OREAD | mode::ORDWR | mode::OEXEC)) {
            return Err(NineError::NOT_OPEN);
        }
        // Never more than the reply can carry, whatever the client asked.
        let count = (count as usize).min(room.saturating_sub(IOHDRSZ));
        offset.checked_add(count as u64).ok_or(NineError::BAD_OFFSET)?;
        let (node, is_dir, dir_next) = (f.node.clone(), f.is_dir(), f.dir_next);
        self.check_labels(caller, &node, Access::Read)?;
        self.scratch.clear();
        self.scratch.resize(count, 0);
        if !is_dir {
            let n = self.fs.read(caller, &node, offset, &mut self.scratch)?;
            return if n <= count { Ok(Answer::Read(n)) } else { Err(NineError::BAD_MESSAGE) };
        }
        // A directory read starts at 0 or continues exactly where the last one ended
        // (intro(5), read); the entry index behind that offset is ours, never the client's.
        let mut index = match offset {
            0 => 0,
            o if o == dir_next.0 => dir_next.1,
            _ => return Err(NineError::BAD_OFFSET),
        };
        let mut w = Writer::new(&mut self.scratch);
        // Each entry takes bytes or ends the loop, so it runs at most `count` times.
        while let Some(stat) = self.fs.dir_entry(caller, &node, index)? {
            if stat.wire().write_entry(&mut w).is_err() {
                if w.position() == 0 {
                    return Err(NineError::TOO_SMALL);
                }
                break;
            }
            index += 1;
        }
        let n = w.position();
        self.fid_mut(caller, fid)?.dir_next = (offset + n as u64, index);
        Ok(Answer::Read(n))
    }

    fn check_labels(&self, caller: &Caller, node: &S::Node, access: Access) -> Result<(), NineError> {
        check(caller.labels.as_slice(), self.fs.labels(node), access).map_err(|_| NineError::PERMISSION)
    }

    fn fid(&self, caller: &Caller, fid: u32) -> Result<&Fid<S::Node>, NineError> {
        self.conns.get(&caller.badge).and_then(|fids| fids.get(&fid)).ok_or(NineError::UNKNOWN_FID)
    }

    fn fid_mut(&mut self, caller: &Caller, fid: u32) -> Result<&mut Fid<S::Node>, NineError> {
        self.conns.get_mut(&caller.badge).and_then(|fids| fids.get_mut(&fid)).ok_or(NineError::UNKNOWN_FID)
    }

    fn check_new_fid(&self, caller: &Caller, fid: u32) -> Result<(), NineError> {
        if fid == NOFID || self.fid(caller, fid).is_ok() {
            return Err(NineError::FID_IN_USE);
        }
        if self.fids(caller.badge) >= MAX_FIDS {
            return Err(NineError::TOO_MANY);
        }
        Ok(())
    }

    /// Adds a new fid, charged to the caller's account.
    fn insert_fid(&mut self, caller: &Caller, fid: u32, state: Fid<S::Node>) -> Result<(), NineError> {
        self.check_new_fid(caller, fid)?;
        self.admission.admit(state.owner, Resource::Files).map_err(|_| NineError::TOO_MANY)?;
        self.conns.entry(caller.badge).or_default().insert(fid, state);
        Ok(())
    }

    fn remove_fid(&mut self, caller: &Caller, fid: u32) -> Result<Fid<S::Node>, NineError> {
        let fids = self.conns.get_mut(&caller.badge).ok_or(NineError::UNKNOWN_FID)?;
        let state = fids.remove(&fid).ok_or(NineError::UNKNOWN_FID)?;
        if fids.is_empty() {
            self.conns.remove(&caller.badge);
        }
        Ok(state)
    }

    /// A fid is gone: release its charge and tell the server.
    fn drop_fid(&mut self, fid: Fid<S::Node>) {
        self.admission.release(fid.owner, Resource::Files);
        self.fs.clunk(&fid.node);
    }
}

/// The accesses opening with `mode` needs (read, write), or `BAD_MODE`: unknown bits, and
/// anything but plain reading for a directory, are refused.
fn open_access(mode: u8, is_dir: bool) -> Result<[Option<Access>; 2], NineError> {
    if mode & !(3 | mode::OTRUNC) != 0 || (is_dir && mode != mode::OREAD) {
        return Err(NineError::BAD_MODE);
    }
    let read = matches!(mode & 3, mode::OREAD | mode::ORDWR | mode::OEXEC).then_some(Access::Read);
    let write =
        (matches!(mode & 3, mode::OWRITE | mode::ORDWR) || mode & mode::OTRUNC != 0).then_some(Access::Write);
    Ok([read, write])
}

#[cfg(test)]
#[path = "ninep_tests.rs"]
mod tests;
