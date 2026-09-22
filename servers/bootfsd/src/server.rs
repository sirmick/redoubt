//! The files behind the 9P skeleton, and the `bootfs` setup protocol in front of it.

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::Handle;
use redoubt_rt::ipc::{Caller, Words};
use redoubt_rt::path;
use redoubt_rt::server::ninep::{
    DMDIR, FIRST_MINTED_BADGE, FileServer, FileStat, NineError, QTDIR, Qid, Read,
};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer};
use redoubt_rt::server::{Cost, Limits};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::bootfs::{self, Add, ErrorCode, Message, Reply, Seal};

/// The most entries the `public` list may name. The manifest is small and read once; a list
/// longer than this is a manifest mistake, not something to serve.
pub const MAX_ENTRIES: usize = 64;
/// The most bytes `/boot` holds in total, across every entry: what bounds this server's own
/// memory, since the bytes are its and not a client's. Its manifest budget must cover this and
/// [`BUDGET`] together.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// What admission lets clients hold, sized so that every bucket at its cap fits [`BUDGET`]
/// (answer 85). Nothing is ever parked here: every request is answered as it arrives.
pub const LIMITS: Limits = Limits { buckets: 16, in_flight: 0, files: 32, state: 8 };
/// What one of each costs, in bytes: a fid is its table entry and its steps from the root (at
/// most two, since `/boot` is flat); a minted connection its record.
pub const COST: Cost = Cost { in_flight: 0, file: 256, state: 256 };
/// The bytes of this server's budget its clients may use between them, over and above
/// [`MAX_BYTES`] of published entries.
pub const BUDGET: u64 = 256 * 1024;

/// Why an argument list was refused. Each one stops the server starting: a `/boot` that is not
/// what the manifest named is worse than no `/boot` at all (TENETS.md 2, fail closed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupError {
    /// More than [`MAX_ENTRIES`] names.
    TooMany,
    /// A name that is not one 9P path component ([`path::valid_name`]): empty, `.`, `..`, or
    /// holding a `/` or a NUL.
    BadName,
    /// The same name twice; `/boot` is flat, so two entries could not be told apart.
    Duplicate,
    /// No memory for the table.
    NoMemory,
}

/// One published entry: its name and the bytes `init` has handed over so far.
struct Entry {
    name: String,
    data: Vec<u8>,
}

/// What a fid rests on. `/boot` is flat: the root, or one entry by its index, which never moves
/// because the table is built from the argument list before anything is served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Root,
    Entry(usize),
}

/// The server: the published entries, and whether setup has ended.
pub struct BootFs {
    entries: Vec<Entry>,
    sealed: bool,
    bytes: usize,
}

impl BootFs {
    /// The server for the `public` list `names`, in the manifest's order, with no bytes yet.
    pub fn new<'a>(names: impl Iterator<Item = &'a str>) -> Result<BootFs, SetupError> {
        let mut entries: Vec<Entry> = Vec::new();
        for name in names {
            if entries.len() >= MAX_ENTRIES {
                return Err(SetupError::TooMany);
            }
            if !path::valid_name(name) {
                return Err(SetupError::BadName);
            }
            if entries.iter().any(|e| e.name == name) {
                return Err(SetupError::Duplicate);
            }
            entries.try_reserve(1).map_err(|_| SetupError::NoMemory)?;
            let mut owned = String::new();
            owned.try_reserve(name.len()).map_err(|_| SetupError::NoMemory)?;
            owned.push_str(name);
            entries.push(Entry { name: owned, data: Vec::new() });
        }
        Ok(BootFs { entries, sealed: false, bytes: 0 })
    }

    /// Whether setup has ended. Until it has, `/boot` is empty.
    pub fn sealed(&self) -> bool { self.sealed }

    /// How many entries the `public` list named.
    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// The published bytes of `name`, for tests.
    pub fn entry(&self, name: &str) -> Option<&[u8]> {
        self.entries.iter().find(|e| e.name == name).map(|e| e.data.as_slice())
    }

    /// `add`: appends `data` to `name` at `offset`, which must be exactly what has been added to
    /// it so far, so a chunk cannot be lost, repeated or reordered.
    fn add(&mut self, name: &str, offset: u64, data: &[u8]) -> Result<(), ErrorCode> {
        if self.sealed {
            return Err(ErrorCode::Refused);
        }
        if self.bytes.saturating_add(data.len()) > MAX_BYTES {
            return Err(ErrorCode::Refused);
        }
        let entry = self.entries.iter_mut().find(|e| e.name == name).ok_or(ErrorCode::Refused)?;
        if offset != entry.data.len() as u64 {
            return Err(ErrorCode::Refused);
        }
        entry.data.try_reserve(data.len()).map_err(|_| ErrorCode::Refused)?;
        entry.data.extend_from_slice(data);
        self.bytes += data.len();
        Ok(())
    }

    /// `seal`: ends setup. After it `/boot` answers walks, and nothing can change an entry.
    fn seal(&mut self) -> Result<(), ErrorCode> {
        if self.sealed {
            return Err(ErrorCode::Refused);
        }
        self.sealed = true;
        Ok(())
    }

    fn qid(&self, node: Node) -> Qid {
        match node {
            Node::Root => Qid { kind: QTDIR, version: 0, path: 0 },
            // Entries never change after `seal`, so every qid version is 0 and stays 0.
            Node::Entry(i) => Qid { kind: 0, version: 0, path: i as u64 + 1 },
        }
    }

    fn stat_of(&self, node: Node) -> Result<FileStat, NineError> {
        let (mode, length, name) = match node {
            Node::Root => (DMDIR | 0o555, 0, "/"),
            Node::Entry(i) => {
                let entry = self.entries.get(i).ok_or(NineError::NOT_FOUND)?;
                (0o444, entry.data.len() as u64, entry.name.as_str())
            }
        };
        let mut owned = String::new();
        owned.try_reserve(name.len()).map_err(|_| NineError::NO_MEMORY)?;
        owned.push_str(name);
        Ok(FileStat { qid: self.qid(node), mode, mtime: 0, length, name: owned })
    }
}

impl FileServer for BootFs {
    type Node = Node;

    /// Every connection attaches at `/boot` itself. `aname` is ignored: the badge decides what a
    /// connection may see (NAMESPACES.md), and here every badge sees the same flat directory.
    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Node, Qid), NineError> {
        Ok((Node::Root, self.qid(Node::Root)))
    }

    /// Unlabelled (the bundle is public to everyone on the box): every caller may read it, and
    /// the write side is refused outright below, not by labels.
    fn labels(&self, _: &Node) -> &[u64] { &[] }

    /// An exact name from the `public` list, and nothing else. Before `seal` there is nothing
    /// at all, so a client that reaches `/boot` early learns nothing from what it finds.
    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        if !self.sealed || *dir != Node::Root {
            return Err(NineError::NOT_FOUND);
        }
        let i = self.entries.iter().position(|e| e.name == name).ok_or(NineError::NOT_FOUND)?;
        Ok((Node::Entry(i), self.qid(Node::Entry(i))))
    }

    /// Reading only: any mode that could change a file is refused, so the read-only promise does
    /// not rest on the label check (an unlabelled caller passes that).
    fn open(&mut self, _: &Caller, node: &Node, mode: u8) -> Result<Qid, NineError> {
        use redoubt_rt::server::ninep::mode as m;
        if mode & m::OTRUNC != 0 || !matches!(mode & 3, m::OREAD | m::OEXEC) {
            return Err(NineError::PERMISSION);
        }
        Ok(self.qid(*node))
    }

    /// Never waits: every byte is already here.
    fn read(&mut self, _: &Caller, node: &Node, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        // Local, not inherited from `walk`: a fid on an entry only exists once sealed, but a read
        // must not depend on that having been checked elsewhere.
        if !self.sealed {
            return Err(NineError::NOT_FOUND);
        }
        let Node::Entry(i) = *node else { return Err(NineError::NOT_FOUND) };
        let data = self.entries.get(i).ok_or(NineError::NOT_FOUND)?.data.as_slice();
        // Any offset is the client's: past the end reads nothing.
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(data.len());
        let n = out.len().min(data.len() - start);
        out[..n].copy_from_slice(&data[start..start + n]);
        Ok(Read::Done(n))
    }

    /// `/boot` is read-only. There is no path here that changes an entry.
    fn write(&mut self, _: &Caller, _: &Node, _: u64, _: &[u8]) -> Result<usize, NineError> {
        Err(NineError::PERMISSION)
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> { self.stat_of(*node) }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &Node,
        index: u64,
    ) -> Result<Option<(Node, FileStat)>, NineError> {
        if !self.sealed || *dir != Node::Root {
            return Ok(None);
        }
        let Ok(i) = usize::try_from(index) else { return Ok(None) };
        if i >= self.entries.len() {
            return Ok(None);
        }
        Ok(Some((Node::Entry(i), self.stat_of(Node::Entry(i))?)))
    }
}

/// The `bootfs` protocol (NAMESPACES.md), named once for [`redoubt_rt::server::typed`].
pub struct Bootfs;

impl Protocol for Bootfs {
    type Error = ErrorCode;
    type Reply<'a> = Reply;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

impl TypedServer<Bootfs> for BootFs {
    /// Setup, and only for the founding connection. A badge at or above [`FIRST_MINTED_BADGE`]
    /// is a connection `new_connection` minted — every client's — and is refused, so filling
    /// `/boot` is the business of whoever was handed this server's endpoint at boot.
    fn handle(
        &mut self,
        caller: &Caller,
        request: Message<'_>,
        _handles: &[Handle],
    ) -> Result<Answer<Reply>, ErrorCode> {
        // Only the founding handle fills `/boot`. A minted badge (`new_connection` mints at or
        // above `FIRST_MINTED_BADGE`) is refused, and so is badge 0 — the receive right, which
        // this server keeps and no `mint` creates — so the gate is an explicit set, not a
        // comparison that happens to exclude them (KERNEL-SPEC.md, Handle).
        if caller.badge == 0 || caller.badge >= FIRST_MINTED_BADGE {
            return Err(ErrorCode::Refused);
        }
        match request {
            Message::Add(Add { name, offset, data }) => {
                self.add(name, offset, data)?;
                Ok(Answer::new(Reply::Add(bootfs::AddReply {})))
            }
            Message::Seal(Seal {}) => {
                self.seal()?;
                Ok(Answer::new(Reply::Seal(bootfs::SealReply {})))
            }
        }
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
