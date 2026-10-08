//! Files over 9P (docs/userland/files.md, "Files over 9P"): the VM's `Files` on Redoubt.
//!
//! - **The namespace resolves the path**: the longest bound prefix names the connection, and the rest is
//!   walked on it from its root. A path above a binding is a directory the namespace answers itself: its
//!   names are the bindings' next names, and nothing in it changes (`eacces`). A path neither inside nor
//!   above a binding is `enoent`: there is nothing there to refuse (docs/userland/sessions.md, "Namespaces").
//! - **A fid is the file descriptor**, and the position lives here, in the VM; nothing else is kept between
//!   calls. Data moves in pieces: a read asks at most what one answer carries, a write at most a page.
//! - **Every request goes through the hub**, from the VM's thread. An operation of several requests (a walk,
//!   then an open, then reads up to the count) goes on as each answer comes, and only the Erlang process that
//!   asked waits for it ([`Files::asker`]). With no asker, the VM's own code loading, the operation is waited
//!   for in place.
//! - **Rename** is `littlefsd`'s typed operation, which no hub carries: it is made on the VM's thread, as the
//!   console's size is.
//! - **Refuse visibly; report only real fields.** A `stat` gives the size, the kind and the times 9P has, and
//!   nothing else; what the file server refuses (`Twstat`: times, a cut at a position) is `enotsup`.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use beamlet_vm::platform::{FileError, FileInfo, FileKind, Files, OpenMode, SeekFrom};
use redoubt_client::aio::{COMPLETION_PAGES, Conn, Done, MAX_WRITE, Outcome, RETRY_US};
use redoubt_client::file::{Connection, ROOT};
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend, Name, littlefsd};
use redoubt_rt::abi::{FOREVER, PAGE_SIZE};
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::ninep::{DMDIR, QTDIR, mode};
use redoubt_rt::wire::ninep::{self, Body, IOHDRSZ, Message, Names, Qid};
use redoubt_rt::wire::proto::littlefsd::ErrorCode;

use crate::Redoubt;
use crate::io::Io;

/// The most one read asks for: what one answer in the hub's completion buffer carries.
const MAX_READ: usize = COMPLETION_PAGES * PAGE_SIZE - IOHDRSZ;
/// The asker the VM's own calls are made as: waited for in place, never named finished.
const VM: u64 = u64::MAX;

/// An open file: its connection and fid, and its position.
struct Open {
    conn: Connection,
    hub: Conn,
    fid: u32,
    pos: u64,
    append: bool,
}

/// What an operation ends with.
enum Answer {
    Handle(u64),
    Data(Vec<u8>),
    Info(FileInfo),
    Names(Vec<Vec<u8>>),
    Pos(u64),
    Done,
}

/// The request an operation has out, or asks for next. All but a walk name the operation's fid.
#[derive(Clone)]
enum Ask {
    Walk(Vec<String>),
    Open(u8),
    Create(String, u32, u8),
    Read(u64, usize),
    Write(u64, Vec<u8>),
    Stat,
    Clunk,
    Remove,
}

/// What the server said to a request.
enum Reply {
    /// A whole walk, and the last qid (`None` for a walk of no names: the binding's root).
    Walked(Option<Qid>),
    Opened,
    Data(Vec<u8>),
    Wrote(usize),
    Stat(FileInfo),
    /// A clunk or remove answered.
    Gone,
    Refused(FileError),
    /// Over the connection's share for now: asked again.
    Busy,
    /// The connection ended with the request out: its fate, and its fid's, are unknown.
    Lost,
}

/// What an operation does, and where it has got to.
enum Kind {
    Info,
    ListDir {
        offset: u64,
        names: Vec<Vec<u8>>,
    },
    ReadFile {
        max: usize,
        data: Vec<u8>,
    },
    /// Opens the file `names` walks to; when `create` names the last, creates it first in the
    /// directory before it, and opens it as it is if it was there already (unless `exclusive`).
    Open {
        names: Vec<String>,
        how: OpenMode,
        create: Option<String>,
    },
    MakeDir {
        name: String,
    },
    Delete {
        dir: bool,
    },
    Read {
        handle: u64,
        offset: u64,
        len: usize,
        data: Vec<u8>,
        moves: bool,
    },
    Write {
        handle: u64,
        offset: Option<u64>,
        data: Vec<u8>,
        sent: usize,
        moves: bool,
    },
    SeekEnd {
        handle: u64,
        delta: i64,
    },
}

/// One asker's operation.
struct Op {
    kind: Kind,
    conn: Connection,
    hub: Conn,
    fid: u32,
    /// The fid is walked and is this operation's to clunk.
    walked: bool,
    /// The request out, and its tag.
    out: Option<(Ask, u16)>,
    /// The server answered the request `busy` (over the connection's share for a moment): when
    /// it goes again, [`RETRY_US`] later rather than at once, as the hub's rule for a queued
    /// request has it.
    retry_at: Option<u64>,
    result: Option<Result<Answer, FileError>>,
    abandoned: bool,
}

/// Whose answer a request's is.
enum Owner {
    Op(u64),
    /// A closed file's clunk, on its hub connection: its fid goes back on the answer.
    Close(Connection, Conn, u32),
}

/// A closed file's clunk the server answered `busy`: sent again when its retry is due, the fid
/// kept until the clunk is answered, since the server still holds it.
struct PendingClose {
    conn: Connection,
    hub: Conn,
    fid: u32,
    retry_at: u64,
}

/// The VM's files: the namespace, the open files, and each asker's operation.
pub(crate) struct Table {
    pub(crate) ns: Namespace,
    open: BTreeMap<u64, Open>,
    next: u64,
    asker: Option<u64>,
    ops: BTreeMap<u64, Op>,
    requests: Vec<(Conn, u16, Owner)>,
    finished: VecDeque<u64>,
    /// Clunks answered `busy`, waiting to go again.
    closes: Vec<PendingClose>,
    /// How many requests the servers have answered `busy`.
    busy: u64,
}

impl Table {
    pub(crate) fn new(ns: Namespace) -> Table {
        Table {
            ns,
            open: BTreeMap::new(),
            next: 1,
            asker: None,
            ops: BTreeMap::new(),
            requests: Vec::new(),
            finished: VecDeque::new(),
            closes: Vec::new(),
            busy: 0,
        }
    }

    /// Sends every request whose retry has come due; an abandoned operation waiting to retry ends
    /// instead, asking nothing more.
    pub(crate) fn pump(&mut self, io: &mut Io) {
        let now = crate::now();
        let mut i = 0;
        while i < self.closes.len() {
            if self.closes[i].retry_at > now {
                i += 1;
                continue;
            }
            let close = self.closes.remove(i);
            // Not sent: the fid stays in use until the connection ends, as `close_handle` leaves it.
            if let Ok(tag) = send(io, close.hub, close.fid, &Ask::Clunk) {
                self.requests.push((close.hub, tag, Owner::Close(close.conn, close.hub, close.fid)));
            }
        }
        let due: Vec<u64> = self
            .ops
            .iter()
            .filter(|(_, op)| op.retry_at.is_some_and(|at| at <= now))
            .map(|(a, _)| *a)
            .collect();
        for asker in due {
            let Some(mut op) = self.ops.remove(&asker) else { continue };
            op.retry_at = None;
            if op.abandoned {
                op.out = None;
                op.result = Some(Err(FileError::Eio));
            }
            self.go_on(io, asker, op);
        }
    }

    /// How long until a request answered `busy` goes again, if one waits to: a bound on any wait
    /// meanwhile, so nothing else need wake the VM for it.
    pub(crate) fn retry_in(&self, now: u64) -> Option<u64> {
        let ops = self.ops.values().filter_map(|op| op.retry_at);
        ops.chain(self.closes.iter().map(|c| c.retry_at)).map(|at| at.saturating_sub(now)).min()
    }

    /// How many requests the servers have answered `busy`.
    pub(crate) fn busy_answers(&self) -> u64 { self.busy }

    /// Takes a completion if it is this table's.
    pub(crate) fn take(&mut self, io: &mut Io, done: Done) {
        let Some(at) = self.requests.iter().position(|(c, t, _)| *c == done.conn && *t == done.tag) else {
            return;
        };
        let (_, _, owner) = self.requests.swap_remove(at);
        match owner {
            Owner::Close(conn, hub, fid) => match done.outcome {
                // Over the connection's share for a moment: the server still holds the fid, so it
                // is clunked again after the retry interval, and freed here only then.
                Outcome::Busy => {
                    self.busy += 1;
                    let retry_at = crate::now().saturating_add(RETRY_US);
                    self.closes.push(PendingClose { conn, hub, fid, retry_at });
                }
                Outcome::Ended | Outcome::Flushed => {}
                _ => conn.free_fid(fid),
            },
            Owner::Op(asker) => {
                if let Some(mut op) = self.ops.remove(&asker) {
                    let reply = op.out.take().map(|(ask, _)| (reply(&ask, done), ask));
                    if let Some((reply, ask)) = reply {
                        self.answer(&mut op, ask, reply);
                    }
                    if op.abandoned && op.result.is_none() {
                        // Its asker is gone: nothing more is asked, and what it holds goes.
                        op.out = None;
                        op.result = Some(Err(FileError::Eio));
                    }
                    self.go_on(io, asker, op);
                }
            }
        }
    }

    /// Whether any request is out, or waits to go again.
    pub(crate) fn busy(&self) -> bool {
        !self.requests.is_empty()
            || !self.closes.is_empty()
            || self.ops.values().any(|op| op.retry_at.is_some())
    }

    /// Whether an operation has ended and its asker has not been told.
    pub(crate) fn has_finished(&self) -> bool { !self.finished.is_empty() }

    /// Takes `reply` to `ask` into `op`.
    fn answer(&mut self, op: &mut Op, ask: Ask, reply: Reply) {
        match (&ask, &reply) {
            (_, Reply::Busy) => {
                // Over the connection's share for a moment: asked again after the retry interval.
                self.busy += 1;
                op.retry_at = Some(crate::now().saturating_add(RETRY_US));
                op.out = Some((ask, 0));
                return;
            }
            (_, Reply::Lost) => {
                // Neither clunked nor reusable: the fid stays in use for good, as the client's does.
                op.walked = false;
                op.result.get_or_insert(Err(FileError::Eio));
                return;
            }
            (Ask::Walk(_), Reply::Refused(_)) => op.conn.free_fid(op.fid),
            (Ask::Clunk | Ask::Remove, _) => {
                op.conn.free_fid(op.fid);
                op.walked = false;
            }
            _ => {}
        }
        if op.result.is_some() {
            // The clunk after the end: the result stands.
            return;
        }
        // A create refused, for an open that may find the file there: the file is opened instead,
        // walked afresh once the directory's fid is clunked.
        if let (Ask::Create(..), Reply::Refused(_), Kind::Open { create: create @ Some(_), how, .. }) =
            (&ask, &reply, &mut op.kind)
        {
            if !how.exclusive {
                *create = None;
                op.out = Some((Ask::Clunk, 0));
                return;
            }
        }
        match (ask, reply) {
            (_, Reply::Refused(e)) => op.result = Some(Err(e)),
            (Ask::Walk(_), Reply::Walked(qid)) => {
                op.walked = true;
                // A walk of no names is the binding's root, a directory.
                let dir = qid.is_none_or(|q| q.kind & QTDIR != 0);
                let next = after_walk(op, dir);
                op.out = Some((next, 0));
            }
            // Only an open's second try clunks before its end.
            (Ask::Clunk, Reply::Gone) => match (&op.kind, op.conn.take_fid()) {
                (Kind::Open { names, .. }, Ok(fid)) => {
                    op.fid = fid;
                    op.out = Some((Ask::Walk(names.clone()), 0));
                }
                (_, Ok(fid)) => {
                    op.conn.free_fid(fid);
                    op.result = Some(Err(FileError::Eio));
                }
                (_, Err(_)) => op.result = Some(Err(FileError::Emfile)),
            },
            (Ask::Open(_) | Ask::Create(..), Reply::Opened) => self.opened(op),
            (Ask::Stat, Reply::Stat(info)) => self.stated(op, info),
            (Ask::Read(_, asked), Reply::Data(data)) => self.read_data(op, asked, data),
            (Ask::Write(..), Reply::Wrote(n)) => self.wrote(op, n),
            (Ask::Remove, Reply::Gone) => op.result = Some(Ok(Answer::Done)),
            // Any other answer does not answer what was asked.
            _ => op.result = Some(Err(FileError::Eio)),
        }
    }

    /// An open or create answered.
    fn opened(&mut self, op: &mut Op) {
        match &op.kind {
            Kind::Open { how, .. } => {
                let handle = self.next;
                self.next += 1;
                let open =
                    Open { conn: op.conn.clone(), hub: op.hub, fid: op.fid, pos: 0, append: how.append };
                self.open.insert(handle, open);
                // The fid is the open file's now.
                op.walked = false;
                op.result = Some(Ok(Answer::Handle(handle)));
            }
            Kind::MakeDir { .. } => op.result = Some(Ok(Answer::Done)),
            Kind::ListDir { offset, .. } => op.out = Some((Ask::Read(*offset, MAX_READ), 0)),
            Kind::ReadFile { .. } => op.out = Some((Ask::Read(0, MAX_READ), 0)),
            _ => op.result = Some(Err(FileError::Eio)),
        }
    }

    /// A stat answered.
    fn stated(&mut self, op: &mut Op, info: FileInfo) {
        match &mut op.kind {
            Kind::Info => op.result = Some(Ok(Answer::Info(info))),
            Kind::Write { offset, .. } => {
                *offset = Some(info.size);
                op.out = Some((write_piece(&op.kind), 0));
            }
            Kind::SeekEnd { handle, delta } => {
                let pos = info.size.checked_add_signed(*delta);
                op.result = Some(match (pos, self.open.get_mut(handle)) {
                    (Some(pos), Some(open)) => {
                        open.pos = pos;
                        Ok(Answer::Pos(pos))
                    }
                    (None, _) => Err(FileError::Einval),
                    (_, None) => Err(FileError::Ebadf),
                });
            }
            _ => op.result = Some(Err(FileError::Eio)),
        }
    }

    /// A read answered with `more`, for `asked` bytes.
    fn read_data(&mut self, op: &mut Op, asked: usize, more: Vec<u8>) {
        match &mut op.kind {
            Kind::ListDir { offset, names } => {
                if more.is_empty() {
                    op.result = Some(Ok(Answer::Names(core::mem::take(names))));
                    return;
                }
                for stat in ninep::stats(&more) {
                    match stat {
                        Ok(stat) => names.push(stat.name.as_bytes().to_vec()),
                        Err(_) => {
                            op.result = Some(Err(FileError::Eio));
                            return;
                        }
                    }
                }
                *offset += more.len() as u64;
                op.out = Some((Ask::Read(*offset, MAX_READ), 0));
            }
            Kind::ReadFile { max, data } => {
                if more.is_empty() {
                    op.result = Some(Ok(Answer::Data(core::mem::take(data))));
                } else if data.len() + more.len() > *max {
                    op.result = Some(Err(FileError::Einval));
                } else {
                    data.extend_from_slice(&more);
                    op.out = Some((Ask::Read(data.len() as u64, MAX_READ), 0));
                }
            }
            Kind::Read { handle, offset, len, data, moves } => {
                let short = more.len() < asked;
                data.extend_from_slice(&more);
                if short || data.len() >= *len {
                    if *moves {
                        if let Some(open) = self.open.get_mut(handle) {
                            open.pos = *offset + data.len() as u64;
                        }
                    }
                    op.result = Some(Ok(Answer::Data(core::mem::take(data))));
                } else {
                    let want = (*len - data.len()).min(MAX_READ);
                    op.out = Some((Ask::Read(*offset + data.len() as u64, want), 0));
                }
            }
            _ => op.result = Some(Err(FileError::Eio)),
        }
    }

    /// A write answered: `n` bytes taken.
    fn wrote(&mut self, op: &mut Op, n: usize) {
        let Kind::Write { handle, offset, data, sent, moves } = &mut op.kind else {
            op.result = Some(Err(FileError::Eio));
            return;
        };
        if n == 0 {
            op.result = Some(Err(FileError::Eio));
            return;
        }
        *sent += n;
        if *sent < data.len() {
            op.out = Some((write_piece(&op.kind), 0));
            return;
        }
        if *moves {
            let end = offset.unwrap_or(0) + data.len() as u64;
            if let Some(open) = self.open.get_mut(handle) {
                open.pos = end;
            }
        }
        op.result = Some(Ok(Answer::Done));
    }

    /// Sends what `op` asks next, or ends it: its result, then a clunk of the fid it walked. An ask
    /// answered `busy` waits here until its retry is due, and [`Table::pump`] sends it.
    fn go_on(&mut self, io: &mut Io, asker: u64, mut op: Op) {
        loop {
            if op.out.is_none() && op.result.is_some() && op.walked {
                op.out = Some((Ask::Clunk, 0));
            }
            if op.out.is_some() && op.retry_at.is_some_and(|at| crate::now() < at) {
                self.ops.insert(asker, op);
                return;
            }
            let Some((ask, _)) = op.out.take() else {
                return self.end(io, asker, op);
            };
            match send(io, op.hub, op.fid, &ask) {
                Ok(tag) => {
                    op.out = Some((ask, tag));
                    self.requests.push((op.hub, tag, Owner::Op(asker)));
                    self.ops.insert(asker, op);
                    return;
                }
                Err(_) => {
                    // Never sent: a walk's fid was never made, and the connection is gone or full.
                    if matches!(ask, Ask::Walk(_)) {
                        op.conn.free_fid(op.fid);
                    }
                    op.walked = false;
                    op.result.get_or_insert(Err(FileError::Eio));
                }
            }
        }
    }

    /// `op` has ended: its result waits for its asker, or is dropped with what it holds.
    fn end(&mut self, io: &mut Io, asker: u64, op: Op) {
        if op.abandoned {
            if let Some(Ok(Answer::Handle(handle))) = op.result {
                self.close_handle(handle, io);
            }
            return;
        }
        self.ops.insert(asker, op);
        if asker != VM {
            self.finished.push_back(asker);
        }
    }

    /// Begins `kind` on the path's connection, walking `names` from its root.
    fn begin(
        &mut self,
        io: &mut Io,
        asker: u64,
        path: &str,
        kind: Kind,
        parent: bool,
    ) -> Result<(), FileError> {
        let (conn, rest) = self.ns.lookup(path).ok_or(FileError::Enoent)?;
        let conn = conn.clone();
        let mut names: Vec<String> = redoubt_rt::path::clean(rest)
            .map_err(|_| FileError::Einval)?
            .into_iter()
            .map(String::from)
            .collect();
        let mut kind = kind;
        if let Kind::Open { names: full, .. } = &mut kind {
            full.clone_from(&names);
        }
        if parent && names.pop().is_none() {
            // A binding's own root has no name to make or remove in its directory.
            return Err(FileError::Eexist);
        }
        if names.len() > ninep::MAXWELEM {
            return Err(FileError::Enametoolong);
        }
        let hub = io.connect(&conn).map_err(|_| FileError::Eio)?;
        let fid = conn.take_fid().map_err(|_| FileError::Emfile)?;
        let op = Op {
            kind,
            conn,
            hub,
            fid,
            walked: false,
            out: Some((Ask::Walk(names), 0)),
            retry_at: None,
            result: None,
            abandoned: false,
        };
        self.go_on(io, asker, op);
        Ok(())
    }

    /// Begins `kind` on the open file `handle`, asking `first`.
    fn begin_open(
        &mut self,
        io: &mut Io,
        asker: u64,
        handle: u64,
        kind: Kind,
        first: Ask,
    ) -> Result<(), FileError> {
        let open = self.open.get(&handle).ok_or(FileError::Ebadf)?;
        let op = Op {
            kind,
            conn: open.conn.clone(),
            hub: open.hub,
            fid: open.fid,
            walked: false,
            out: Some((first, 0)),
            retry_at: None,
            result: None,
            abandoned: false,
        };
        self.go_on(io, asker, op);
        Ok(())
    }

    /// Closes `handle`: its fid is clunked, and goes back on the answer.
    fn close_handle(&mut self, handle: u64, io: &mut Io) {
        let Some(open) = self.open.remove(&handle) else { return };
        // Not sent: the fid stays in use until the connection ends, as a dropped file's does.
        if let Ok(tag) = send(io, open.hub, open.fid, &Ask::Clunk) {
            self.requests.push((open.hub, tag, Owner::Close(open.conn, open.hub, open.fid)));
        }
    }

    /// The finished operation of `asker`, taken.
    fn take_result(&mut self, asker: u64) -> Option<Result<Answer, FileError>> {
        let op = self.ops.get(&asker)?;
        if op.result.is_none() || op.out.is_some() {
            return None;
        }
        self.finished.retain(|a| *a != asker);
        self.ops.remove(&asker)?.result
    }
}

/// What an operation asks once its walk has reached a directory (`dir`) or a file; one that
/// cannot go on there ends, and asks the clunk of its fid.
fn after_walk(op: &mut Op, dir: bool) -> Ask {
    let refuse = |op: &mut Op, e| {
        op.result = Some(Err(e));
        Ask::Clunk
    };
    match &op.kind {
        Kind::Info => Ask::Stat,
        Kind::ListDir { .. } if dir => Ask::Open(mode::OREAD),
        Kind::ListDir { .. } => refuse(op, FileError::Enotdir),
        Kind::Open { create: Some(name), how, .. } => Ask::Create(name.clone(), 0o644, nine_mode(*how)),
        Kind::ReadFile { .. } | Kind::Open { .. } if dir => refuse(op, FileError::Eisdir),
        Kind::ReadFile { .. } => Ask::Open(mode::OREAD),
        Kind::Open { how, .. } => Ask::Open(nine_mode(*how) | if how.truncate { mode::OTRUNC } else { 0 }),
        Kind::MakeDir { name } => Ask::Create(name.clone(), DMDIR | 0o755, mode::OREAD),
        Kind::Delete { dir: wanted } if *wanted != dir => {
            refuse(op, if dir { FileError::Eisdir } else { FileError::Enotdir })
        }
        Kind::Delete { .. } => Ask::Remove,
        _ => refuse(op, FileError::Eio),
    }
}

/// The 9P open mode for `how`, without `OTRUNC`.
fn nine_mode(how: OpenMode) -> u8 {
    match (how.read, how.write) {
        (true, true) => mode::ORDWR,
        (false, true) => mode::OWRITE,
        _ => mode::OREAD,
    }
}

/// The next piece of a write: at most a page, at what is sent so far.
fn write_piece(kind: &Kind) -> Ask {
    match kind {
        Kind::Write { offset, data, sent, .. } => {
            let end = data.len().min(sent + MAX_WRITE);
            Ask::Write(offset.unwrap_or(0) + *sent as u64, data[*sent..end].to_vec())
        }
        _ => Ask::Stat,
    }
}

/// Sends `ask` on `hub` for `fid`.
fn send(io: &mut Io, hub: Conn, fid: u32, ask: &Ask) -> Result<u16, Error> {
    let hub_ = io.request();
    match ask {
        Ask::Walk(names) => {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let wnames = Names::new(&names)?;
            hub_.submit(hub, Body::Twalk { fid: ROOT, newfid: fid, wnames }, None)
        }
        Ask::Open(m) => hub_.submit(hub, Body::Topen { fid, mode: *m }, None),
        Ask::Create(name, perm, m) => {
            hub_.submit(hub, Body::Tcreate { fid, name: name.as_str(), perm: *perm, mode: *m }, None)
        }
        // A buffer of whole pages: the server may send more than was asked, and what is past the
        // count is dropped (`reply`).
        Ask::Read(offset, count) => {
            let pages = count.saturating_add(IOHDRSZ).div_ceil(PAGE_SIZE).clamp(1, COMPLETION_PAGES);
            hub_.read(hub, fid, *offset, Buffer::new(pages)?)
        }
        Ask::Write(offset, data) => {
            let mut buffer = Buffer::new(1)?;
            buffer[..data.len()].copy_from_slice(data);
            hub_.write(hub, fid, *offset, buffer, data.len())
        }
        Ask::Stat => hub_.submit(hub, Body::Tstat { fid }, None),
        Ask::Clunk => hub_.submit(hub, Body::Tclunk { fid }, None),
        Ask::Remove => hub_.submit(hub, Body::Tremove { fid }, None),
    }
}

/// What the server said to `ask`, from its completion.
fn reply(ask: &Ask, done: Done) -> Reply {
    let bytes = match done.outcome {
        Outcome::Read(n) => {
            let asked = if let Ask::Read(_, count) = ask { *count } else { 0 };
            let n = n.min(asked);
            return Reply::Data(done.buffer.map(|b| b[..n.min(b.len())].to_vec()).unwrap_or_default());
        }
        Outcome::Wrote(n) => return Reply::Wrote(n as usize),
        Outcome::Busy => return Reply::Busy,
        Outcome::Rerror(name) => return Reply::Refused(posix(name)),
        Outcome::Flushed | Outcome::Ended => return Reply::Lost,
        Outcome::Reply(bytes) => bytes,
    };
    let Ok(message) = Message::decode(&bytes) else { return Reply::Lost };
    match (ask, message.body) {
        (Ask::Walk(names), Body::Rwalk { qids }) if qids.as_slice().len() == names.len() => {
            Reply::Walked(qids.as_slice().last().copied())
        }
        // A walk that stopped short: the name is not there.
        (Ask::Walk(_), Body::Rwalk { .. }) => Reply::Refused(FileError::Enoent),
        (Ask::Open(_), Body::Ropen { .. }) | (Ask::Create(..), Body::Rcreate { .. }) => Reply::Opened,
        (Ask::Stat, Body::Rstat { stat }) => Reply::Stat(info(&stat)),
        (Ask::Clunk, Body::Rclunk) | (Ask::Remove, Body::Rremove) => Reply::Gone,
        _ => Reply::Refused(FileError::Eio),
    }
}

/// What a 9P stat says, and only that: the size, the kind and the times; no Unix fields.
fn info(stat: &ninep::Stat<'_>) -> FileInfo {
    let kind = if stat.qid.kind & QTDIR != 0 { FileKind::Directory } else { FileKind::Regular };
    plain(kind, stat.length, i64::from(stat.atime), i64::from(stat.mtime))
}

/// A file's kind, size and times, and no Unix fields: they are `undefined` to OTP.
fn plain(kind: FileKind, size: u64, atime: i64, mtime: i64) -> FileInfo {
    FileInfo {
        unix: false,
        size,
        kind,
        readable: false,
        writable: false,
        atime,
        mtime,
        ctime: mtime,
        mode: 0,
        links: 0,
        inode: 0,
        uid: 0,
        gid: 0,
    }
}

impl Redoubt {
    /// Runs the operation `begin` starts as the current asker's: its result if it has finished,
    /// `Later` while it goes on. The VM's own calls wait for it in place.
    fn run(
        &mut self,
        begin: impl FnOnce(&mut Table, &mut Io, u64) -> Result<Option<Answer>, FileError>,
    ) -> Result<Answer, FileError> {
        let asker = self.files.asker.unwrap_or(VM);
        if let Some(result) = self.files.take_result(asker) {
            return result;
        }
        if self.files.ops.contains_key(&asker) {
            return Err(FileError::Later);
        }
        if let Some(answer) = begin(&mut self.files, &mut self.io, asker)? {
            return Ok(answer);
        }
        if asker != VM {
            return self.files.take_result(asker).unwrap_or(Err(FileError::Later));
        }
        loop {
            if let Some(result) = self.files.take_result(VM) {
                return result;
            }
            self.io.wait(FOREVER);
            self.dispatch();
        }
    }

    /// Renames `from` to `to` by `littlefsd`'s typed `rename`, which names the two directories'
    /// fids: both must be on one connection, else `exdev`.
    fn rename_on(&mut self, from: &str, to: &str) -> Result<(), FileError> {
        if below(&self.files.ns, from).is_some() || below(&self.files.ns, to).is_some() {
            return Err(FileError::Eacces);
        }
        let (a, a_rest) = self.files.ns.lookup(from).ok_or(FileError::Enoent)?;
        let (b, b_rest) = self.files.ns.lookup(to).ok_or(FileError::Enoent)?;
        if !a.same(b) {
            // Two servers cannot rename between them (docs/userland/files.md).
            return Err(FileError::Exdev);
        }
        let conn = a.clone();
        let (a_dir, a_name) = split(a_rest)?;
        let (b_dir, b_name) = split(b_rest)?;
        let lend: &mut Lend = &mut self.lend;
        let old = conn.open(lend, a_dir, mode::OREAD).map_err(refusal)?;
        let new = match conn.open(lend, b_dir, mode::OREAD) {
            Ok(new) => new,
            Err(e) => {
                let _ = old.close(lend);
                return Err(refusal(e));
            }
        };
        let renamed = littlefsd::rename(lend, &old, a_name, &new, b_name);
        let _ = old.close(lend);
        let _ = new.close(lend);
        renamed.map_err(refusal)
    }

    /// Copies `from` to the new file `to` by `littlefsd`'s typed `copy_file`, which names the
    /// file's fid and the new one's directory: both must be on one connection, else `exdev`. Made
    /// on the VM's thread, as `rename_on` is, so the VM waits while the server copies.
    fn copy_on(&mut self, from: &str, to: &str) -> Result<u64, FileError> {
        if below(&self.files.ns, from).is_some() || below(&self.files.ns, to).is_some() {
            return Err(FileError::Eacces);
        }
        let (a, a_rest) = self.files.ns.lookup(from).ok_or(FileError::Enoent)?;
        let (b, b_rest) = self.files.ns.lookup(to).ok_or(FileError::Enoent)?;
        if !a.same(b) {
            return Err(FileError::Exdev);
        }
        let conn = a.clone();
        let a_rest = a_rest.to_string();
        let (b_dir, b_name) = split(b_rest)?;
        let lend: &mut Lend = &mut self.lend;
        let src = conn.open(lend, &a_rest, mode::OREAD).map_err(refusal)?;
        let dir = match conn.open(lend, b_dir, mode::OREAD) {
            Ok(dir) => dir,
            Err(e) => {
                let _ = src.close(lend);
                return Err(refusal(e));
            }
        };
        let copied = littlefsd::copy_file(lend, &src, &dir, b_name);
        let _ = src.close(lend);
        let _ = dir.close(lend);
        copied.map_err(refusal)
    }
}

/// `rest`'s directory and last name; the root has none.
fn split(rest: &str) -> Result<(&str, &str), FileError> {
    let rest = rest.trim_end_matches('/');
    match rest.rsplit_once('/') {
        Some((dir, name)) => Ok((dir, name)),
        None if rest.is_empty() => Err(FileError::Einval),
        None => Ok(("", rest)),
    }
}

/// A Redoubt error as the POSIX one OTP's `file` expects: a 9P `Rerror`'s name, or the name a
/// typed error code shares.
fn refusal(e: Error) -> FileError {
    let name = match e {
        Error::Rerror(name) => name,
        Error::Server(code) => match ErrorCode::from_code(code) {
            Some(ErrorCode::NotFound) => Name::NotFound,
            Some(ErrorCode::Exists) => Name::Exists,
            Some(ErrorCode::NotDir) => Name::NotDir,
            Some(ErrorCode::Removed) => Name::Removed,
            Some(ErrorCode::TooLarge) => Name::TooLarge,
            Some(ErrorCode::NoSpace) => Name::NoSpace,
            Some(ErrorCode::Corrupt) => Name::Corrupt,
            Some(ErrorCode::NotPermitted) => Name::NotPermitted,
            Some(ErrorCode::NotSupported) => Name::NotSupported,
            Some(ErrorCode::BadName) => Name::BadName,
            Some(ErrorCode::ReadOnly) => Name::ReadOnly,
            Some(ErrorCode::NoMemory) => Name::NoMemory,
            Some(ErrorCode::IsDir) => Name::IsDir,
            Some(ErrorCode::NotEmpty) => Name::NotEmpty,
            Some(ErrorCode::Malformed) => Name::Protocol,
            None => Name::Other,
        },
        Error::Refused(_) => return FileError::Einval,
        _ => Name::Other,
    };
    posix(name)
}

/// The POSIX error OTP's `file` expects for a Redoubt error's name: the `File` boundary's one
/// mapping, servers/wire.md's last column ("Error names"). Everywhere else an error keeps its name.
pub fn posix(name: Name) -> FileError {
    match name {
        Name::NotFound => FileError::Enoent,
        Name::NotPermitted => FileError::Eacces,
        Name::Exists => FileError::Eexist,
        Name::NotDir => FileError::Enotdir,
        Name::IsDir => FileError::Eisdir,
        Name::NotEmpty => FileError::Enotempty,
        Name::NoSpace => FileError::Enospc,
        Name::ReadOnly => FileError::Erofs,
        Name::Removed => FileError::Estale,
        Name::TooMany => FileError::Emfile,
        Name::NoMemory => FileError::Enomem,
        Name::NotSupported => FileError::Enotsup,
        Name::BadName => FileError::Einval,
        Name::TooLarge => FileError::Efbig,
        Name::Refused => FileError::Econnrefused,
        Name::Timeout => FileError::Etimedout,
        Name::Unreachable => FileError::Ehostunreach,
        Name::InUse => FileError::Eaddrinuse,
        Name::State => FileError::Enotconn,
        Name::Corrupt | Name::Protocol | Name::Busy | Name::Other => FileError::Eio,
    }
}

impl Files for Redoubt {
    fn open(&mut self, path: &str, how: OpenMode) -> Result<u64, FileError> {
        if below(&self.files.ns, path).is_some() {
            return Err(FileError::Eacces);
        }
        let path = path.to_string();
        let answer = self.run(|t, io, asker| {
            let kind = |create| Kind::Open { names: Vec::new(), how, create };
            if how.create {
                let name = path.rsplit('/').next().unwrap_or("").to_string();
                t.begin(io, asker, &path, kind(Some(name)), true)?;
            } else {
                t.begin(io, asker, &path, kind(None), false)?;
            }
            Ok(None)
        })?;
        match answer {
            Answer::Handle(handle) => Ok(handle),
            _ => Err(FileError::Eio),
        }
    }

    fn close(&mut self, handle: u64) { self.files.close_handle(handle, &mut self.io); }

    fn read(&mut self, handle: u64, len: usize) -> Result<Vec<u8>, FileError> {
        let pos = self.files.open.get(&handle).map(|o| o.pos);
        self.read_at(handle, pos, len, true)
    }

    fn write(&mut self, handle: u64, data: &[u8]) -> Result<(), FileError> {
        let open = self.files.open.get(&handle);
        let offset = open.and_then(|o| (!o.append).then_some(o.pos));
        self.write_at(handle, offset, data, true)
    }

    fn pread(&mut self, handle: u64, offset: u64, len: usize) -> Result<Vec<u8>, FileError> {
        self.read_at(handle, Some(offset), len, false)
    }

    fn pwrite(&mut self, handle: u64, offset: u64, data: &[u8]) -> Result<(), FileError> {
        self.write_at(handle, Some(offset), data, false)
    }

    fn seek(&mut self, handle: u64, to: SeekFrom) -> Result<u64, FileError> {
        let pos = self.files.open.get(&handle).ok_or(FileError::Ebadf)?.pos;
        let answer = self.run(|t, io, asker| {
            let at = match to {
                SeekFrom::Start(at) => at,
                SeekFrom::Current(delta) => pos.checked_add_signed(delta).ok_or(FileError::Einval)?,
                SeekFrom::End(delta) => {
                    t.begin_open(io, asker, handle, Kind::SeekEnd { handle, delta }, Ask::Stat)?;
                    return Ok(None);
                }
            };
            t.open.get_mut(&handle).ok_or(FileError::Ebadf)?.pos = at;
            Ok(Some(Answer::Pos(at)))
        })?;
        match answer {
            Answer::Pos(pos) => Ok(pos),
            _ => Err(FileError::Eio),
        }
    }

    /// 9P2000 cuts a file only at its open (`OTRUNC`); the file server refuses `Twstat`.
    fn truncate(&mut self, _handle: u64) -> Result<(), FileError> { Err(FileError::Enotsup) }

    /// Nothing is buffered here: each write was a request the server answered.
    fn sync(&mut self, handle: u64) -> Result<(), FileError> {
        self.files.open.get(&handle).map(|_| ()).ok_or(FileError::Ebadf)
    }

    fn handle_info(&mut self, handle: u64) -> Result<FileInfo, FileError> {
        let answer = self.run(|t, io, asker| {
            t.begin_open(io, asker, handle, Kind::Info, Ask::Stat)?;
            Ok(None)
        })?;
        match answer {
            Answer::Info(info) => Ok(info),
            _ => Err(FileError::Eio),
        }
    }

    fn info(&mut self, path: &str, _follow: bool) -> Result<FileInfo, FileError> {
        match self.path_op(path, Kind::Info, false)? {
            Answer::Info(info) => Ok(info),
            _ => Err(FileError::Eio),
        }
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<Vec<u8>>, FileError> {
        match self.path_op(path, Kind::ListDir { offset: 0, names: Vec::new() }, false)? {
            Answer::Names(names) => Ok(names),
            _ => Err(FileError::Eio),
        }
    }

    fn make_dir(&mut self, path: &str) -> Result<(), FileError> {
        let name = path.rsplit('/').next().unwrap_or("").to_string();
        self.path_op(path, Kind::MakeDir { name }, true).map(|_| ())
    }

    fn delete(&mut self, path: &str) -> Result<(), FileError> {
        self.path_op(path, Kind::Delete { dir: false }, false).map(|_| ())
    }

    fn del_dir(&mut self, path: &str) -> Result<(), FileError> {
        self.path_op(path, Kind::Delete { dir: true }, false).map(|_| ())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FileError> { self.rename_on(from, to) }

    fn copy_file(&mut self, from: &str, to: &str) -> Result<u64, FileError> { self.copy_on(from, to) }

    fn read_file(&mut self, path: &str, max: usize) -> Result<Vec<u8>, FileError> {
        match self.path_op(path, Kind::ReadFile { max, data: Vec::new() }, false)? {
            Answer::Data(data) => Ok(data),
            _ => Err(FileError::Eio),
        }
    }

    fn asker(&mut self, asker: Option<u64>) { self.files.asker = asker; }

    fn finished(&mut self) -> Option<u64> {
        if self.files.finished.is_empty() && self.files.busy() {
            self.take_completed();
        }
        self.files.finished.pop_front()
    }

    fn abandon(&mut self, asker: u64) {
        self.files.finished.retain(|a| *a != asker);
        let Some(op) = self.files.ops.get_mut(&asker) else { return };
        if op.out.is_some() {
            op.abandoned = true;
            return;
        }
        if let Some(Op { result: Some(Ok(Answer::Handle(handle))), .. }) = self.files.ops.remove(&asker) {
            self.files.close_handle(handle, &mut self.io);
        }
    }
}

/// The next name of each binding below `path`, when `path` is above a binding and inside none:
/// a directory the namespace answers itself.
fn below(ns: &Namespace, path: &str) -> Option<Vec<Vec<u8>>> {
    // Cleaned as a walk's rest is: `/home/.` is `/home`.
    let base: String = redoubt_rt::path::clean(path).ok()?.iter().flat_map(|name| ["/", name]).collect();
    if ns.lookup(if base.is_empty() { "/" } else { &base }).is_some() {
        return None;
    }
    let mut names: Vec<Vec<u8>> = ns
        .list()
        .filter_map(|(prefix, _)| {
            let rest = prefix.strip_prefix(base.as_str())?.strip_prefix('/')?;
            rest.split('/').next().filter(|name| !name.is_empty()).map(|name| name.as_bytes().to_vec())
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    (!names.is_empty()).then_some(names)
}

impl Redoubt {
    /// A path above the bindings is answered here: its info and its names; anything that would
    /// change it is `eacces`.
    fn path_op(&mut self, path: &str, kind: Kind, parent: bool) -> Result<Answer, FileError> {
        if let Some(names) = below(&self.files.ns, path) {
            return match kind {
                Kind::Info => Ok(Answer::Info(plain(FileKind::Directory, 0, 0, 0))),
                Kind::ListDir { .. } => Ok(Answer::Names(names)),
                Kind::ReadFile { .. } => Err(FileError::Eisdir),
                _ => Err(FileError::Eacces),
            };
        }
        self.run(|t, io, asker| {
            t.begin(io, asker, path, kind, parent)?;
            Ok(None)
        })
    }

    fn read_at(
        &mut self,
        handle: u64,
        offset: Option<u64>,
        len: usize,
        moves: bool,
    ) -> Result<Vec<u8>, FileError> {
        let offset = offset.ok_or(FileError::Ebadf)?;
        let answer = self.run(|t, io, asker| {
            let kind = Kind::Read { handle, offset, len, data: Vec::new(), moves };
            t.begin_open(io, asker, handle, kind, Ask::Read(offset, len.min(MAX_READ)))?;
            Ok(None)
        })?;
        match answer {
            Answer::Data(data) => Ok(data),
            _ => Err(FileError::Eio),
        }
    }

    /// Writes `data` at `offset`, or at the end for `None` (a file opened to append).
    fn write_at(
        &mut self,
        handle: u64,
        offset: Option<u64>,
        data: &[u8],
        moves: bool,
    ) -> Result<(), FileError> {
        if !self.files.open.contains_key(&handle) {
            return Err(FileError::Ebadf);
        }
        if data.is_empty() {
            return Ok(());
        }
        self.run(|t, io, asker| {
            let kind = Kind::Write { handle, offset, data: data.to_vec(), sent: 0, moves };
            let first = if offset.is_some() { write_piece(&kind) } else { Ask::Stat };
            t.begin_open(io, asker, handle, kind, first)?;
            Ok(None)
        })
        .map(|_| ())
    }
}
