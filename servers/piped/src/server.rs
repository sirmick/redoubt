//! The pipes behind the 9P skeleton: a directory per pipe, its read end and its write end, a
//! buffer of [`PIPE_BYTES`] between them, and who holds each end.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::PAGE_SIZE;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{
    DMDIR, FIRST_MINTED_BADGE, FileServer, FileStat, NineError, QTDIR, Qid, REQUEST_STATE, Read, Write, mode,
};
use redoubt_rt::server::{Cost, Limits};

/// The badge of the one connection nobody minted: the session's, which the platform gave it when
/// it started this server (docs/userland/beamlet.md, "Natives": `launch` with `serve`). Only it
/// attaches, makes and removes pipes, and mints the connections the stages hold.
pub const ROOT_BADGE: u64 = 1;
/// The bytes one pipe holds between its writer and its reader.
pub const PIPE_BYTES: usize = PAGE_SIZE;
/// The most pipes at once: a pipeline of the most stages the session runs (its `MAX_JOBS`, 16,
/// less this server) needs one more pipe than stages, and one for their standard error.
pub const MAX_PIPES: usize = 32;
/// The longest name a pipe's directory may have.
pub const MAX_NAME: usize = 64;

/// The read end's file name in a pipe's directory.
pub const READ_END: &str = "r";
/// The write end's.
pub const WRITE_END: &str = "w";

/// What `buckets=N` gives each bucket (servers/serving.md R26). A session's stages carry its
/// account and labels (kernel/budgets.md R8), and the session minted their connections through its
/// own, so the session and every stage are one client: one bucket, and one share in it, which may
/// take half the bucket; a session passes `buckets=2`, the fewest admission allows. Half of each cap
/// is what a session of 8 processes needs at most: 6 stages each park one call (a stage reads or
/// writes one stream at a time) and the session its completion call; each stage holds three
/// connections and six fids (a root and an open stream on each), and the session a fid on each
/// pipe it makes, feeds or reads.
pub const fn limits(buckets: u32) -> Limits {
    Limits { buckets, in_flight: 16, files: 128, state: 64, requests: 64, pages: 4 }
}

/// What one of each costs, in bytes: a parked call holds its caller's lend, charged to this server
/// until it replies (kernel/ipc.md R3), `MAX_LEND_PAGES` pages at worst; a fid is its record and
/// its steps (at most three: the root, a pipe, an end); a minted connection its record; a request
/// its record; a page a page.
pub const COST: Cost =
    Cost { in_flight: 64 * 1024, file: 256, state: 256, request: REQUEST_STATE, page: PAGE_SIZE as u64 };
/// The bytes of this server's budget its clients may use between them, over and above the pipes'
/// buffers ([`MAX_PIPES`] of [`PIPE_BYTES`]): a bucket at its caps, 16 parked calls, 128 fids, 64
/// connections, 64 requests and 4 pages, is 1 130 496 bytes, and two 2 260 992.
pub const BUDGET: u64 = 2_260_992;

/// Which end of a pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Read,
    Write,
}

/// What a fid rests on. A pipe is named by an id never reused, so a fid left on a removed pipe
/// reaches no later one. An end walked to by the session carries a serial of its own, so that
/// its clunk can be told from another fid's on the same end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Root,
    Pipe(u64),
    End { pipe: u64, end: End, serial: u64 },
}

/// One pipe.
struct Pipe {
    id: u64,
    name: String,
    buffer: VecDeque<u8>,
    /// Who holds each end now: connections minted at it, and the session's fids open on it.
    writers: u32,
    readers: u32,
    /// Whether each end was ever held: before that, a read waits and a write is taken, so that
    /// the order the session starts its stages in does not matter.
    had_writer: bool,
    had_reader: bool,
    /// The serials of the session's fids open on an end, with the end.
    opened: Vec<(u64, End)>,
}

impl Pipe {
    fn hold(&mut self, end: End) {
        match end {
            End::Read => {
                self.readers += 1;
                self.had_reader = true;
            }
            End::Write => {
                self.writers += 1;
                self.had_writer = true;
            }
        }
    }

    fn let_go(&mut self, end: End) {
        match end {
            End::Read => self.readers = self.readers.saturating_sub(1),
            End::Write => self.writers = self.writers.saturating_sub(1),
        }
        if end == End::Read && self.readers == 0 {
            // Nobody will read what is left.
            self.buffer = VecDeque::new();
        }
    }

    /// Its write end is gone for good: what is buffered is the rest of the stream.
    fn ended(&self) -> bool { self.had_writer && self.writers == 0 }

    /// Its read end is gone for good: a write has nowhere to go.
    fn unread(&self) -> bool { self.had_reader && self.readers == 0 }
}

/// The pipes of one session.
pub struct Pipes {
    pipes: Vec<Pipe>,
    /// The connections minted at an end: (badge, pipe, end).
    held: Vec<(u64, u64, End)>,
    next_pipe: u64,
    next_serial: u64,
    /// The session's label set, read off its first attach; the kernel lets no other set's message
    /// through to a server with no exemption (kernel/ipc.md R1), so every caller has it.
    labels: Vec<u64>,
    /// Something a waiting call may want has changed since [`Pipes::take_moved`] last asked.
    moved: bool,
}

impl Default for Pipes {
    fn default() -> Pipes { Pipes::new() }
}

impl Pipes {
    pub fn new() -> Pipes {
        Pipes {
            pipes: Vec::new(),
            held: Vec::new(),
            next_pipe: 1,
            next_serial: 1,
            labels: Vec::new(),
            moved: false,
        }
    }

    /// Whether a pipe filled, drained, or lost an end since the last ask, so that waiting calls are
    /// worth serving again.
    pub fn take_moved(&mut self) -> bool { core::mem::take(&mut self.moved) }

    /// The pipes there are now.
    pub fn len(&self) -> usize { self.pipes.len() }

    pub fn is_empty(&self) -> bool { self.pipes.is_empty() }

    /// The bytes buffered in the pipe `name`, if there is one.
    pub fn buffered(&self, name: &str) -> Option<usize> {
        self.pipes.iter().find(|p| p.name == name).map(|p| p.buffer.len())
    }

    /// Who holds the ends of the pipe `name`: (writers, readers).
    pub fn holders(&self, name: &str) -> Option<(u32, u32)> {
        self.pipes.iter().find(|p| p.name == name).map(|p| (p.writers, p.readers))
    }

    fn pipe(&mut self, id: u64) -> Result<&mut Pipe, NineError> {
        self.pipes.iter_mut().find(|p| p.id == id).ok_or(REMOVED)
    }

    fn serial(&mut self) -> u64 {
        let serial = self.next_serial;
        self.next_serial += 1;
        serial
    }
}

/// A pipe removed under a fid still held (servers/wire.md, "Error names": `estale`).
const REMOVED: NineError = NineError("removed");
/// A write to a pipe whose reader has gone (`enotconn`): the stream has nowhere to go.
const STATE: NineError = NineError("state");

fn is_root(caller: &Caller) -> bool { caller.badge == ROOT_BADGE }

fn qid(node: &Node) -> Qid {
    match *node {
        Node::Root => Qid { kind: QTDIR, version: 0, path: 0 },
        Node::Pipe(id) => Qid { kind: QTDIR, version: 0, path: id << 2 },
        Node::End { pipe, end: End::Read, .. } => Qid { kind: 0, version: 0, path: pipe << 2 | 1 },
        Node::End { pipe, end: End::Write, .. } => Qid { kind: 0, version: 0, path: pipe << 2 | 2 },
    }
}

impl FileServer for Pipes {
    type Node = Node;

    /// The root, and only through the session's own badge, with no name: nobody else was given a
    /// connection that was not minted.
    fn attach(&mut self, caller: &Caller, aname: &str) -> Result<(Node, Qid), NineError> {
        if !is_root(caller) {
            return Err(NineError::PERMISSION);
        }
        if !aname.is_empty() {
            return Err(NineError::NOT_FOUND);
        }
        if self.labels.is_empty() {
            self.labels.try_reserve(caller.labels.as_slice().len()).map_err(|_| NineError::NO_MEMORY)?;
            self.labels.extend_from_slice(caller.labels.as_slice());
        }
        Ok((Node::Root, qid(&Node::Root)))
    }

    /// A connection rooted at one end of one pipe, and nothing wider: that is all a stage is ever
    /// given. Its holder holds that end until the connection is disconnected.
    fn minted(&mut self, _: &Caller, badge: u64, _: u64, root: &Node, _: u64) -> Result<(), NineError> {
        let Node::End { pipe, end, .. } = *root else { return Err(NineError::PERMISSION) };
        self.held.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
        self.pipe(pipe)?.hold(end);
        self.held.push((badge, pipe, end));
        Ok(())
    }

    fn disconnected(&mut self, badge: u64) {
        let Some(i) = self.held.iter().position(|(b, _, _)| *b == badge) else { return };
        let (_, pipe, end) = self.held.swap_remove(i);
        if let Ok(p) = self.pipe(pipe) {
            p.let_go(end);
        }
        self.moved = true;
    }

    fn labels(&self, _: &Node) -> &[u64] { &self.labels }

    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        let node = match *dir {
            Node::Root => {
                let p = self.pipes.iter().find(|p| p.name == name).ok_or(NineError::NOT_FOUND)?;
                Node::Pipe(p.id)
            }
            Node::Pipe(pipe) => {
                self.pipe(pipe)?;
                let end = match name {
                    READ_END => End::Read,
                    WRITE_END => End::Write,
                    _ => return Err(NineError::NOT_FOUND),
                };
                Node::End { pipe, end, serial: self.serial() }
            }
            Node::End { .. } => return Err(NineError::NOT_DIR),
        };
        Ok((node, qid(&node)))
    }

    /// An end opens only its own way: the read end for reading, the write end for writing. A fid
    /// of the session's open on an end holds it, as a stage's connection does.
    fn open(&mut self, caller: &Caller, node: &Node, how: u8) -> Result<Qid, NineError> {
        if let Node::End { pipe, end, serial } = *node {
            let wanted = match how & 3 {
                mode::OREAD => End::Read,
                mode::OWRITE => End::Write,
                _ => return Err(NineError::PERMISSION),
            };
            if wanted != end || how & mode::OTRUNC != 0 {
                return Err(NineError::PERMISSION);
            }
            if caller.badge < FIRST_MINTED_BADGE {
                let p = self.pipe(pipe)?;
                p.opened.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
                p.hold(end);
                p.opened.push((serial, end));
            }
        }
        Ok(qid(node))
    }

    /// What is buffered, at most `out`'s length; the end of the stream once the write end is gone
    /// and nothing is left; otherwise the call waits. The offset is the client's and means nothing:
    /// a pipe is read in order.
    fn read(&mut self, _: &Caller, node: &Node, _: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let Node::End { pipe, end: End::Read, .. } = *node else { return Err(NineError::NOT_OPEN) };
        let p = self.pipe(pipe)?;
        if p.buffer.is_empty() {
            return Ok(if p.ended() { Read::Done(0) } else { Read::Wait });
        }
        let n = out.len().min(p.buffer.len());
        for (slot, byte) in out.iter_mut().zip(p.buffer.drain(..n)) {
            *slot = byte;
        }
        self.moved = true;
        Ok(Read::Done(n))
    }

    /// Never called: the skeleton writes through [`FileServer::write_or_wait`].
    fn write(&mut self, _: &Caller, _: &Node, _: u64, _: &[u8]) -> Result<usize, NineError> {
        Err(NineError::NOT_SUPPORTED)
    }

    /// As much as there is room for, at least a byte; the call waits while the pipe is full, and
    /// is refused once the read end is gone.
    fn write_or_wait(&mut self, _: &Caller, node: &Node, _: u64, data: &[u8]) -> Result<Write, NineError> {
        let Node::End { pipe, end: End::Write, .. } = *node else { return Err(NineError::NOT_OPEN) };
        let p = self.pipe(pipe)?;
        if p.unread() {
            return Err(STATE);
        }
        let n = data.len().min(PIPE_BYTES - p.buffer.len());
        if n == 0 && !data.is_empty() {
            return Ok(Write::Wait);
        }
        p.buffer.try_reserve(n).map_err(|_| NineError::NO_MEMORY)?;
        p.buffer.extend(&data[..n]);
        self.moved |= n > 0;
        Ok(Write::Done(n))
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> {
        let (name, mode, length) = match *node {
            Node::Root => (String::from("/"), DMDIR | 0o500, 0),
            Node::Pipe(pipe) => {
                let p = self.pipe(pipe)?;
                (p.name.clone(), DMDIR | 0o500, 0)
            }
            Node::End { pipe, end: End::Read, .. } => {
                let p = self.pipe(pipe)?;
                (String::from(READ_END), 0o400, p.buffer.len() as u64)
            }
            Node::End { pipe, end: End::Write, .. } => {
                self.pipe(pipe)?;
                (String::from(WRITE_END), 0o200, 0)
            }
        };
        Ok(FileStat { qid: qid(node), mode, mtime: 0, length, name })
    }

    fn dir_entry(
        &mut self,
        caller: &Caller,
        dir: &Node,
        index: u64,
    ) -> Result<Option<(Node, FileStat)>, NineError> {
        let node = match *dir {
            Node::Root => match self.pipes.get(index as usize) {
                Some(p) => Node::Pipe(p.id),
                None => return Ok(None),
            },
            Node::Pipe(pipe) => {
                let end = match index {
                    0 => End::Read,
                    1 => End::Write,
                    _ => return Ok(None),
                };
                // Serial 0 is no fid's: a listing opens nothing.
                Node::End { pipe, end, serial: 0 }
            }
            Node::End { .. } => return Err(NineError::NOT_DIR),
        };
        let stat = self.stat(caller, &node)?;
        Ok(Some((node, stat)))
    }

    /// A pipe, made by the session in the root and nowhere else, with both its ends.
    fn create(
        &mut self,
        caller: &Caller,
        dir: &Node,
        name: &str,
        perm: u32,
        _: u8,
    ) -> Result<(Node, Qid), NineError> {
        if !is_root(caller) || *dir != Node::Root || perm & DMDIR == 0 {
            return Err(NineError::PERMISSION);
        }
        if name.len() > MAX_NAME {
            return Err(NineError::BAD_NAME);
        }
        if self.pipes.iter().any(|p| p.name == name) {
            return Err(NineError("file exists"));
        }
        if self.pipes.len() >= MAX_PIPES {
            return Err(NineError::TOO_MANY);
        }
        let mut owned = String::new();
        owned.try_reserve(name.len()).map_err(|_| NineError::NO_MEMORY)?;
        owned.push_str(name);
        self.pipes.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
        let id = self.next_pipe;
        self.next_pipe += 1;
        self.pipes.push(Pipe {
            id,
            name: owned,
            buffer: VecDeque::new(),
            writers: 0,
            readers: 0,
            had_writer: false,
            had_reader: false,
            opened: Vec::new(),
        });
        let node = Node::Pipe(id);
        Ok((node, qid(&node)))
    }

    /// A pipe goes when the session removes its directory; whatever waits on it is answered
    /// `removed`, and its ends' holders keep connections that reach nothing.
    fn remove(&mut self, caller: &Caller, node: &Node) -> Result<(), NineError> {
        let Node::Pipe(pipe) = *node else { return Err(NineError::PERMISSION) };
        if !is_root(caller) {
            return Err(NineError::PERMISSION);
        }
        let i = self.pipes.iter().position(|p| p.id == pipe).ok_or(REMOVED)?;
        self.pipes.remove(i);
        self.moved = true;
        Ok(())
    }

    /// A fid of the session's open on an end lets go of it.
    fn clunk(&mut self, node: &Node) {
        let Node::End { pipe, serial, .. } = *node else { return };
        let Ok(p) = self.pipe(pipe) else { return };
        if let Some(i) = p.opened.iter().position(|(s, _)| *s == serial) {
            let (_, end) = p.opened.swap_remove(i);
            p.let_go(end);
            self.moved = true;
        }
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
