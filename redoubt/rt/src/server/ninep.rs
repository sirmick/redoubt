//! A 9P2000 server skeleton (NAMESPACES.md): it keeps the protocol state (connections, fids,
//! open modes, directory offsets) and applies every rule that does not depend on what the files
//! are; a [`FileServer`] supplies the files. `src/bin/echo-server.rs` is the model server.
//!
//! **On the wire.** A 9P request is a `call` whose words are all zero ([`WORDS_9P`]) with the
//! T-message at the start of its lend; the reply's words are all zero and the R-message is
//! written at the start of the same lend. A call with other words, or with no lend, is
//! malformed: its reply is status 1 ([`MALFORMED`]), as for every typed protocol (answers 41 and
//! 42), and nothing is written in the lend. Handles sent with a 9P call are closed unread.
//!
//! **Connections.** One badge is one client. The rule (answer 50): a launcher never passes its
//! own connection to a child; each child gets a fresh connection from the server. As a second
//! line of defence, a connection is keyed by (badge, account, label set), so holders of a copied
//! handle in different accounts or label sets never share fids or a `Tversion`. When the last
//! handle with a badge is closed, the kernel tells the endpoint's owner (answer 53), and
//! [`NineServer::badge_closed`] frees everything that badge held.
//!
//! **What the skeleton guarantees a [`FileServer`]**, whatever the client sends:
//! - At most [`MAX_FIDS`] fids per connection, each charged to its client's (account, label set)
//!   ([`Admission`], [`Resource::Files`]); both limits are checked before the server is asked to attach or
//!   walk.
//! - Every fid is looked up; no request reaches the server for a fid that does not exist.
//! - Walk names are valid path components ([`path::valid_name`]). A fid keeps the node and qid of every step
//!   from its attach root, so `..` is the step before, never a question to the server; at the root it stays
//!   at the root. A fid is at most [`path::MAX_COMPONENTS`] below its root.
//! - Only directories are walked from or created in; directories are opened only for reading; reads need a
//!   fid opened for reading, writes one opened for writing.
//! - `offset + count` never overflows; a read asks for at most what fits the caller's lend and the msize; a
//!   directory read continues only from where the last one ended.
//!
//! **Labels** ([`check`], on every request, against [`FileServer::labels`] of the object named):
//! - `Read` on the attach root; on the directory walked from, and on every node walked into (a qid is a read,
//!   answer 52); on the node for `Tstat`, `Tread` and opening for reading; on every directory entry listed
//!   (entries the caller cannot read are left out).
//! - `Write` (equal label sets, answer 51) on the node for `Twrite`, opening for writing, truncation
//!   (`OTRUNC`) and `Tremove`, and on the directory for `Tcreate`.
//!
//! **Protocol corners.** `Tversion` is accepted at any time and clunks every fid of the
//! connection; it is not required first, since the msize is fixed. `Tauth` is refused (access
//! is by capability). `Twstat` is refused. `Tflush` is answered at once: requests are handled one
//! at a time, so none is ever in flight to flush.
//!
//! **Memory.** Every allocation a request makes fails cleanly with an `Rerror` ("out of memory")
//! rather than killing the server. A server's budget needs headroom beyond its own use: lends
//! whose callers died stay charged to it until it replies (R3), up to `MAX_OPEN_CALLS` ×
//! `MAX_LEND_PAGES` pages (1024), and while that pushes it over its limit its allocations fail.

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_sys::Error;
use redoubt_wire::MSIZE;
use redoubt_wire::codec::Writer;
pub use redoubt_wire::ninep::Qid;
use redoubt_wire::ninep::{Body, IOHDRSZ, MAXWELEM, Message, NOFID, NOTAG, Qids, Stat, VERSION};

use super::admit::{Admission, AdmitKey, Limits, Resource};
use super::label::{Access, check};
use crate::ipc::{Caller, Request, Words};
use crate::path;

/// The words of a 9P request and of its reply.
pub const WORDS_9P: Words = [0; 4];
pub use super::MALFORMED;
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
    pub const NO_MEMORY: NineError = NineError("out of memory");
    pub const PERMISSION: NineError = NineError("permission denied");
    pub const TOO_DEEP: NineError = NineError("path too deep");
    pub const TOO_MANY: NineError = NineError("too many open files");
    pub const TOO_SMALL: NineError = NineError("count too small");
    pub const UNKNOWN_FID: NineError = NineError("unknown fid");
}

/// What `Tstat` and directory reads report about a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    /// A file or directory the server can find again: what a fid rests on. A plain value that
    /// holds no resource: the skeleton copies nodes freely (a fid keeps one per step from its
    /// root) and drops them without telling the server.
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

    /// The `index`th entry of the directory `dir` and its stat, or `None` past the last. The
    /// skeleton lists only entries the caller may read.
    fn dir_entry(
        &mut self,
        caller: &Caller,
        dir: &Self::Node,
        index: u64,
    ) -> Result<Option<(Self::Node, FileStat)>, NineError>;

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

    /// A fid went away (clunked, removed, or reset by `Tversion`); `node` is the one it rested on.
    /// Only that node is clunked: nodes passed on a walk, and nodes a failed request produced, are
    /// simply dropped, which is why a node must hold no resource.
    fn clunk(&mut self, _node: &Self::Node) {}
}

/// Whose a connection is: the badge it came through and the client using it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConnKey {
    badge: u64,
    client: AdmitKey,
}

impl ConnKey {
    fn of(caller: &Caller) -> ConnKey { ConnKey { badge: caller.badge, client: AdmitKey::of(caller) } }
}

/// One fid's state.
struct Fid<N> {
    /// Every step from the attach root (first) to where the fid rests (last); never empty.
    steps: Vec<(N, Qid)>,
    /// The mode it was opened with, if it is open.
    open: Option<u8>,
    /// Where the next directory read continues: (byte offset, entry index).
    dir_next: (u64, u64),
}

impl<N: Clone> Fid<N> {
    fn here(&self) -> &(N, Qid) { self.steps.last().expect("a fid always has its root") }

    fn node(&self) -> N { self.here().0.clone() }

    fn is_dir(&self) -> bool { self.here().1.kind & QTDIR != 0 }
}

/// A connection's fids, by number.
type Fids<N> = Vec<(u32, Fid<N>)>;

/// A copy of `steps`, allocated fallibly.
fn copy_steps<N: Clone>(steps: &[(N, Qid)]) -> Result<Vec<(N, Qid)>, NineError> {
    let mut copy = Vec::new();
    copy.try_reserve(steps.len() + 1).map_err(|_| NineError::NO_MEMORY)?;
    copy.extend_from_slice(steps);
    Ok(copy)
}

/// Serves 9P for a [`FileServer`].
pub struct NineServer<S: FileServer> {
    pub fs: S,
    /// Fid tables by connection; a connection with no fids has no entry. Plain vectors, searched
    /// linearly, so every growth can fail cleanly (`try_reserve`).
    conns: Vec<(ConnKey, Fids<S::Node>)>,
    admission: Admission,
    /// Where a reply's data is gathered before it is written into the lend: the request is
    /// decoded from the lend, so the reply cannot be built there until the request is done with.
    scratch: Vec<u8>,
    /// The last `Tstat`'s answer, for the same reason.
    stat: FileStat,
}

impl<S: FileServer> NineServer<S> {
    /// Serves `fs`; `limits.files` bounds the fids one client (account, label set) holds across
    /// all its connections.
    pub fn new(fs: S, limits: Limits) -> NineServer<S> {
        NineServer {
            fs,
            conns: Vec::new(),
            admission: Admission::new(limits),
            scratch: Vec::new(),
            stat: FileStat::default(),
        }
    }

    /// Fids open on the caller's connection.
    pub fn fids(&self, caller: &Caller) -> usize {
        self.conn(&ConnKey::of(caller)).map_or(0, |i| self.conns[i].1.len())
    }

    /// The last handle with `badge` is gone (the kernel's notice, answer 53): every connection
    /// through it is over, so its fids are clunked and their charges released. The notice's
    /// receive result arrives with a `redoubt-sys` update; a server's receive loop calls this
    /// when it gets one.
    pub fn badge_closed(&mut self, badge: u64) {
        while let Some(i) = self.conns.iter().position(|(key, _)| key.badge == badge) {
            let (key, fids) = self.conns.swap_remove(i);
            for (_, fid) in fids {
                self.drop_fid(&key, fid);
            }
        }
    }

    /// Handles one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        // Handles are no part of 9P; closing them keeps a client from filling our handle table.
        // A missing one (revoked on its way, R10) makes the request malformed, as in the typed
        // layer (WIRE.md).
        let missing = request.handles.as_slice().contains(&None);
        for handle in request.handles.as_slice().iter().flatten() {
            let _ = crate::handle::close(*handle);
        }
        let words = if missing || request.words != WORDS_9P {
            MALFORMED
        } else {
            let caller = request.caller;
            match self.answer_in_place(&caller, request.lend()) {
                Some(()) => WORDS_9P,
                None => MALFORMED,
            }
        };
        // Words and no handles always encode, so the request cannot come back.
        request.reply(&words, &[]).map_err(|(e, _)| e)
    }

    /// Reads the T-message at the front of `lend` and writes the R-message over it; `None` if
    /// there is not even room for an `Rerror`.
    pub fn answer_in_place(&mut self, caller: &Caller, lend: &mut [u8]) -> Option<()> {
        // Room for the reply: the lend, within the msize. Read data is bounded by it.
        let room = lend.len().min(MSIZE);
        let (tag, reply) = match Message::decode(lend) {
            Ok(message) => (message.tag, self.answer(caller, message.body, room)),
            // The tag, if the header is there, so the client can match the error.
            Err(_) => {
                let tag = lend.get(5..7).map_or(NOTAG, |t| u16::from_le_bytes([t[0], t[1]]));
                (tag, Err(NineError::BAD_MESSAGE))
            }
        };
        let lend = &mut lend[..room];
        let body = reply.unwrap_or_else(|e| Body::Rerror { ename: e.0 });
        if (Message { tag, body }).encode(lend).is_ok() {
            return Some(());
        }
        // The answer did not fit the lend: say so if even that fits.
        Message { tag, body: Body::Rerror { ename: "reply too large" } }.encode(lend).ok().map(|_| ())
    }

    /// The R-message for `body`; a reply's data borrows the server's scratch space.
    fn answer<'s>(&'s mut self, caller: &Caller, body: Body<'_>, room: usize) -> Result<Body<'s>, NineError> {
        let key = ConnKey::of(caller);
        match body {
            Body::Tversion { msize, version } => {
                // A new session on this connection: every fid on it goes (intro(5), version).
                if let Some(i) = self.conn(&key) {
                    let (_, fids) = self.conns.swap_remove(i);
                    for (_, fid) in fids {
                        self.drop_fid(&key, fid);
                    }
                }
                // A version we do not speak is answered "unknown" (intro(5), version).
                let version = if version.starts_with(VERSION) { VERSION } else { "unknown" };
                Ok(Body::Rversion { msize: msize.min(MSIZE as u32), version })
            }
            Body::Tauth { .. } => Err(NineError::NO_AUTH),
            Body::Tattach { fid, afid, aname, .. } => {
                if afid != NOFID {
                    return Err(NineError::NO_AUTH);
                }
                self.reserve_fid(&key, fid)?;
                let attached = self.fs.attach(caller, aname).and_then(|(node, qid)| {
                    self.may_read(caller, &node)?;
                    let mut steps = Vec::new();
                    steps.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
                    steps.push((node, qid));
                    Ok((Fid { steps, open: None, dir_next: (0, 0) }, qid))
                });
                let (state, qid) = self.unreserve_on_error(&key, attached)?;
                self.insert_fid(&key, fid, state)?;
                Ok(Body::Rattach { qid })
            }
            Body::Twalk { fid, newfid, wnames } => {
                let mut qids = [Qid::default(); MAXWELEM];
                let n = self.walk(caller, fid, newfid, wnames.as_slice(), &mut qids)?;
                Ok(Body::Rwalk { qids: Qids::new(&qids[..n]).map_err(|_| NineError::BAD_MESSAGE)? })
            }
            Body::Topen { fid, mode } => {
                let f = self.fid(&key, fid)?;
                if f.open.is_some() {
                    return Err(NineError::IS_OPEN);
                }
                let node = f.node();
                self.check_open(caller, &node, mode, f.is_dir())?;
                let qid = self.fs.open(caller, &node, mode)?;
                let f = self.fid_mut(&key, fid)?;
                f.open = Some(mode);
                f.dir_next = (0, 0);
                Ok(Body::Ropen { qid, iounit: 0 })
            }
            Body::Tcreate { fid, name, perm, mode } => {
                let f = self.fid(&key, fid)?;
                if f.open.is_some() {
                    return Err(NineError::IS_OPEN);
                }
                if !f.is_dir() {
                    return Err(NineError::NOT_DIR);
                }
                if !path::valid_name(name) {
                    return Err(NineError::BAD_NAME);
                }
                if f.steps.len() > path::MAX_COMPONENTS {
                    return Err(NineError::TOO_DEEP);
                }
                let dir = f.node();
                self.check_labels(caller, &dir, Access::Write)?;
                check_mode(mode, perm & DMDIR != 0)?;
                self.fid_mut(&key, fid)?.steps.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
                let (node, qid) = self.fs.create(caller, &dir, name, perm, mode)?;
                let f = self.fid_mut(&key, fid)?;
                f.steps.push((node, qid));
                f.open = Some(mode);
                f.dir_next = (0, 0);
                Ok(Body::Rcreate { qid, iounit: 0 })
            }
            Body::Tread { fid, offset, count } => {
                let n = self.read(caller, fid, offset, count, room)?;
                Ok(Body::Rread { data: self.scratch.get(..n).unwrap_or(&[]) })
            }
            Body::Twrite { fid, offset, data } => {
                let f = self.fid(&key, fid)?;
                if !f.open.is_some_and(|m| matches!(m & 3, mode::OWRITE | mode::ORDWR)) {
                    return Err(NineError::NOT_OPEN);
                }
                offset.checked_add(data.len() as u64).ok_or(NineError::BAD_OFFSET)?;
                let node = f.node();
                self.check_labels(caller, &node, Access::Write)?;
                let n = self.fs.write(caller, &node, offset, data)?;
                // A server claiming more than it was given is a bug; do not pass it on.
                let count = u32::try_from(n)
                    .ok()
                    .filter(|n| *n as usize <= data.len())
                    .ok_or(NineError::BAD_MESSAGE)?;
                Ok(Body::Rwrite { count })
            }
            Body::Tclunk { fid } => {
                let f = self.remove_fid(&key, fid)?;
                self.drop_fid(&key, f);
                Ok(Body::Rclunk)
            }
            Body::Tremove { fid } => {
                // The fid goes whether or not the remove succeeds (intro(5), remove).
                let f = self.remove_fid(&key, fid)?;
                let node = f.node();
                let result = self
                    .check_labels(caller, &node, Access::Write)
                    .and_then(|()| self.fs.remove(caller, &node));
                self.drop_fid(&key, f);
                result.map(|()| Body::Rremove)
            }
            Body::Tstat { fid } => {
                let node = self.fid(&key, fid)?.node();
                self.check_labels(caller, &node, Access::Read)?;
                self.stat = self.fs.stat(caller, &node)?;
                Ok(Body::Rstat { stat: self.stat.wire() })
            }
            Body::Twstat { .. } => Err(NineError::NOT_SUPPORTED),
            Body::Tflush { .. } => Ok(Body::Rflush),
            // R-messages travel only from servers.
            _ => Err(NineError::BAD_MESSAGE),
        }
    }

    /// Walks `newfid` from `fid` along `names` and writes the qids walked into `qids`; returns
    /// how many (fewer than `names` if a later name failed; then `newfid` is unchanged, intro(5)).
    fn walk(
        &mut self,
        caller: &Caller,
        fid: u32,
        newfid: u32,
        names: &[&str],
        qids: &mut [Qid; MAXWELEM],
    ) -> Result<usize, NineError> {
        let key = ConnKey::of(caller);
        let f = self.fid(&key, fid)?;
        if f.open.is_some() {
            return Err(NineError::IS_OPEN);
        }
        let mut steps = copy_steps(&f.steps)?;
        let charged = newfid != fid;
        if charged {
            self.reserve_fid(&key, newfid)?;
        }
        let mut walked = 0;
        for name in names {
            match self.step(caller, &mut steps, name) {
                Ok(qid) => {
                    // The codec caps a walk at MAXWELEM names, so this is always in range.
                    let Some(slot) = qids.get_mut(walked) else { break };
                    *slot = qid;
                    walked += 1;
                }
                // The first name failing is an error; a later one ends the walk short.
                Err(e) => {
                    if charged {
                        self.admission.release(key.client, Resource::Files);
                    }
                    return if walked == 0 { Err(e) } else { Ok(walked) };
                }
            }
        }
        if charged {
            self.insert_fid(&key, newfid, Fid { steps, open: None, dir_next: (0, 0) })?;
        } else {
            self.fid_mut(&key, fid)?.steps = steps;
        }
        Ok(walked)
    }

    /// One walk step by `name` from where `steps` ends; returns the new qid.
    fn step(
        &mut self,
        caller: &Caller,
        steps: &mut Vec<(S::Node, Qid)>,
        name: &str,
    ) -> Result<Qid, NineError> {
        let (dir, qid) = steps.last().cloned().ok_or(NineError::UNKNOWN_FID)?;
        if qid.kind & QTDIR == 0 {
            return Err(NineError::NOT_DIR);
        }
        self.check_labels(caller, &dir, Access::Read)?;
        if name == ".." {
            // The step before, as it was walked; at the root, the root.
            if steps.len() > 1 {
                steps.pop();
            }
            return steps.last().map(|(_, qid)| *qid).ok_or(NineError::UNKNOWN_FID);
        }
        if !path::valid_name(name) {
            return Err(NineError::BAD_NAME);
        }
        if steps.len() > path::MAX_COMPONENTS {
            return Err(NineError::TOO_DEEP);
        }
        steps.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
        let (node, qid) = self.fs.walk(caller, &dir, name)?;
        self.may_read(caller, &node)?;
        steps.push((node, qid));
        Ok(qid)
    }

    /// Reads into the scratch buffer; how many bytes.
    fn read(
        &mut self,
        caller: &Caller,
        fid: u32,
        offset: u64,
        count: u32,
        room: usize,
    ) -> Result<usize, NineError> {
        let key = ConnKey::of(caller);
        let f = self.fid(&key, fid)?;
        if !f.open.is_some_and(|m| matches!(m & 3, mode::OREAD | mode::ORDWR | mode::OEXEC)) {
            return Err(NineError::NOT_OPEN);
        }
        // Never more than the reply can carry, whatever the client asked.
        let count = (count as usize).min(room.saturating_sub(IOHDRSZ));
        offset.checked_add(count as u64).ok_or(NineError::BAD_OFFSET)?;
        let (node, is_dir, dir_next) = (f.node(), f.is_dir(), f.dir_next);
        self.check_labels(caller, &node, Access::Read)?;
        self.scratch.clear();
        self.scratch.try_reserve(count).map_err(|_| NineError::NO_MEMORY)?;
        self.scratch.resize(count, 0);
        if !is_dir {
            let n = self.fs.read(caller, &node, offset, &mut self.scratch)?;
            return if n <= count { Ok(n) } else { Err(NineError::BAD_MESSAGE) };
        }
        // A directory read starts at 0 or continues exactly where the last one ended
        // (intro(5), read); the entry index behind that offset is ours, never the client's.
        let mut index = match offset {
            0 => 0,
            o if o == dir_next.0 => dir_next.1,
            _ => return Err(NineError::BAD_OFFSET),
        };
        let mut w = Writer::new(&mut self.scratch);
        // Each entry is left out, takes bytes, or ends the loop: it ends by the directory's end or
        // by `count`.
        while let Some((entry, stat)) = self.fs.dir_entry(caller, &node, index)? {
            let readable = check(caller.labels.as_slice(), self.fs.labels(&entry), Access::Read).is_ok();
            if readable && stat.wire().write_entry(&mut w).is_err() {
                if w.position() == 0 {
                    return Err(NineError::TOO_SMALL);
                }
                break;
            }
            index += 1;
        }
        let n = w.position();
        self.fid_mut(&key, fid)?.dir_next = (offset + n as u64, index);
        Ok(n)
    }

    /// The label check for opening with `mode` (and the mode's own rules).
    fn check_open(&self, caller: &Caller, node: &S::Node, mode: u8, is_dir: bool) -> Result<(), NineError> {
        check_mode(mode, is_dir)?;
        if matches!(mode & 3, mode::OREAD | mode::ORDWR | mode::OEXEC) {
            self.check_labels(caller, node, Access::Read)?;
        }
        if matches!(mode & 3, mode::OWRITE | mode::ORDWR) || mode & mode::OTRUNC != 0 {
            self.check_labels(caller, node, Access::Write)?;
        }
        Ok(())
    }

    /// A node's qid reaches the caller only if the caller may read the node: answer 52, "a qid
    /// is a read". The one place walks and attaches enforce it.
    fn may_read(&self, caller: &Caller, node: &S::Node) -> Result<(), NineError> {
        self.check_labels(caller, node, Access::Read)
    }

    fn check_labels(&self, caller: &Caller, node: &S::Node, access: Access) -> Result<(), NineError> {
        check(caller.labels.as_slice(), self.fs.labels(node), access).map_err(|_| NineError::PERMISSION)
    }

    fn conn(&self, key: &ConnKey) -> Option<usize> { self.conns.iter().position(|(k, _)| k == key) }

    fn fid(&self, key: &ConnKey, fid: u32) -> Result<&Fid<S::Node>, NineError> {
        let conn = self.conn(key).ok_or(NineError::UNKNOWN_FID)?;
        self.conns[conn]
            .1
            .iter()
            .find(|(f, _)| *f == fid)
            .map(|(_, state)| state)
            .ok_or(NineError::UNKNOWN_FID)
    }

    fn fid_mut(&mut self, key: &ConnKey, fid: u32) -> Result<&mut Fid<S::Node>, NineError> {
        let conn = self.conn(key).ok_or(NineError::UNKNOWN_FID)?;
        let fids = &mut self.conns[conn].1;
        fids.iter_mut().find(|(f, _)| *f == fid).map(|(_, state)| state).ok_or(NineError::UNKNOWN_FID)
    }

    /// Checks that `fid` can be added to the connection and charges it to the client: all before
    /// the server does any work for it. Undone by `insert_fid` failing or `unreserve_on_error`.
    fn reserve_fid(&mut self, key: &ConnKey, fid: u32) -> Result<(), NineError> {
        if fid == NOFID || self.fid(key, fid).is_ok() {
            return Err(NineError::FID_IN_USE);
        }
        if self.conn(key).is_some_and(|i| self.conns[i].1.len() >= MAX_FIDS) {
            return Err(NineError::TOO_MANY);
        }
        self.admission.admit(key.client, Resource::Files).map_err(|_| NineError::TOO_MANY)
    }

    fn unreserve_on_error<T>(&mut self, key: &ConnKey, result: Result<T, NineError>) -> Result<T, NineError> {
        if result.is_err() {
            self.admission.release(key.client, Resource::Files);
        }
        result
    }

    /// Adds a reserved fid; on failure (no memory) its reservation is released.
    fn insert_fid(&mut self, key: &ConnKey, fid: u32, state: Fid<S::Node>) -> Result<(), NineError> {
        let conn = match self.conn(key) {
            Some(conn) => Ok(conn),
            None => self.conns.try_reserve(1).map(|()| {
                self.conns.push((*key, Vec::new()));
                self.conns.len() - 1
            }),
        };
        let fids = conn.and_then(|conn| {
            let fids = &mut self.conns[conn].1;
            fids.try_reserve(1).map(|()| fids)
        });
        match fids {
            Ok(fids) => {
                fids.push((fid, state));
                Ok(())
            }
            Err(_) => {
                // A connection pushed above with no fids must not stay.
                self.conns.retain(|(_, fids)| !fids.is_empty());
                self.admission.release(key.client, Resource::Files);
                Err(NineError::NO_MEMORY)
            }
        }
    }

    fn remove_fid(&mut self, key: &ConnKey, fid: u32) -> Result<Fid<S::Node>, NineError> {
        let conn = self.conn(key).ok_or(NineError::UNKNOWN_FID)?;
        let fids = &mut self.conns[conn].1;
        let index = fids.iter().position(|(f, _)| *f == fid).ok_or(NineError::UNKNOWN_FID)?;
        let (_, state) = fids.swap_remove(index);
        if fids.is_empty() {
            self.conns.swap_remove(conn);
        }
        Ok(state)
    }

    /// A fid is gone: release its charge and clunk the node it rested on.
    fn drop_fid(&mut self, key: &ConnKey, fid: Fid<S::Node>) {
        self.admission.release(key.client, Resource::Files);
        self.fs.clunk(&fid.node());
    }
}

/// Refuses unknown mode bits, and anything but plain reading for a directory.
fn check_mode(mode: u8, is_dir: bool) -> Result<(), NineError> {
    if mode & !(3 | mode::OTRUNC) != 0 || (is_dir && mode != mode::OREAD) {
        return Err(NineError::BAD_MODE);
    }
    Ok(())
}

#[cfg(test)]
#[path = "ninep_tests.rs"]
mod tests;
