//! The files behind the 9P skeleton: one littlefs volume, every node carrying the volume's
//! labels.

use alloc::string::String;
use alloc::vec::Vec;

use littlefs::{BlockDevice, DirRef, Error as FsError, FileHandle, FileType, Filesystem, OpenOptions};
use redoubt_rt::ipc::Caller;
use redoubt_rt::path;
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, NineError, QTDIR, Qid, Read, mode};
use redoubt_rt::server::{Cost, Limits};
use redoubt_rt::wire::proto::fsd::ErrorCode;

use crate::volume::{Blocks, Mounted, Range};

/// What admission lets each of `buckets` buckets hold (servers/serving.md R26); the count is the
/// manifest's `buckets=N`. Nothing is parked: every request is answered as it arrives.
pub const fn limits(buckets: u32) -> Limits { Limits { buckets, in_flight: 0, files: 32, state: 8 } }
/// What one of each costs, in bytes: a fid is its table entry and a node per step from its root,
/// each a path; a minted connection its record and its root's path.
pub const COST: Cost = Cost { in_flight: 0, file: 2048, state: 512 };
/// The bytes of this server's budget its clients may use between them.
pub const BUDGET: u64 = 2 * 1024 * 1024;

/// `fsd`'s own attribute types, 0 to 15 (servers/fsd.md, "Attributes"): a client's `set_attr`
/// cannot touch them.
pub const OWN_ATTRS: u8 = 16;
/// A file's or directory's id, a little-endian `u64`: its qid path, and how a fid tells the file
/// it was walked to from one that took its place.
pub(crate) const ATTR_ID: u8 = 0;
/// A file's qid version, a little-endian `u32`, moved by every write and truncation so a client
/// caching the file sees it changed; absent is 0. A directory's stays 0.
const ATTR_VERSION: u8 = 2;
/// On the root only: the next id to give, a little-endian `u64`.
const ATTR_NEXT_ID: u8 = 3;
/// The root's id. Every other id is given from 1 up and never given twice.
const ROOT_ID: u64 = 0;

/// The `Rerror` texts `fsd` adds to the skeleton's fixed set.
pub mod text {
    use redoubt_rt::server::ninep::NineError;

    /// The volume is damaged or its device failed (fsd's `corrupt`).
    pub const CORRUPT: NineError = NineError("corrupt");
    /// The file was removed, or another took its place, since the fid was walked to it.
    pub const REMOVED: NineError = NineError("removed");
    pub const EXISTS: NineError = NineError("file exists");
    pub const NOT_EMPTY: NineError = NineError("directory not empty");
    pub const IS_DIR: NineError = NineError("is a directory");
    pub const NO_SPACE: NineError = NineError("no space");
    pub const TOO_BIG: NineError = NineError("file too large");
    pub const READ_ONLY: NineError = NineError("read-only volume");
}

/// Why `labels=` was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

/// The volume's label set from the arguments other than `buckets=`: `labels=ID[,ID...]` at most
/// once, each ID decimal without leading zeros, at most `MAX_LABELS`; absent, the set is empty.
/// Anything else is refused, so `fsd` never serves a volume under labels it misread.
pub fn parse_labels<'a>(args: impl Iterator<Item = &'a str>) -> Result<Vec<u64>, BadArgs> {
    let mut labels = None;
    for arg in args {
        let list = arg.strip_prefix("labels=").ok_or(BadArgs)?;
        if labels.is_some() {
            return Err(BadArgs);
        }
        let mut set = Vec::new();
        for id in list.split(',') {
            let canonical = !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && (id == "0" || !id.starts_with('0'));
            let id: u64 = id.parse().ok().filter(|_| canonical).ok_or(BadArgs)?;
            if set.contains(&id) || set.len() >= redoubt_rt::abi::MAX_LABELS {
                return Err(BadArgs);
            }
            set.try_reserve(1).map_err(|_| BadArgs)?;
            set.push(id);
        }
        labels = Some(set);
    }
    Ok(labels.unwrap_or_default())
}

/// What a fid rests on: a path from the volume's root, built only from names clients walked or
/// created (never from a name read off the medium), and the id the file had when the fid got
/// there. A node holds no resource; every request finds its file again and checks the id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    path: String,
    id: u64,
    dir: bool,
}

impl Node {
    fn root() -> Node { Node { path: String::new(), id: ROOT_ID, dir: true } }

    /// Where the node is, from the volume's root.
    pub fn path(&self) -> &str { &self.path }

    /// The path of this directory's entry `name` (a valid component).
    pub(crate) fn child(&self, name: &str) -> Result<String, Failure> {
        join(&self.path, name).map_err(|_| Failure::NoMemory)
    }

    /// The last component, or `/` for the root.
    fn name(&self) -> &str { self.path.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("/") }
}

/// `dir/name`, allocated fallibly. `name` is a valid component.
fn join(dir: &str, name: &str) -> Result<String, NineError> {
    let mut path = String::new();
    path.try_reserve(dir.len() + 1 + name.len()).map_err(|_| NineError::NO_MEMORY)?;
    if !dir.is_empty() {
        path.push_str(dir);
        path.push('/');
    }
    path.push_str(name);
    Ok(path)
}

/// One volume's files.
pub struct Fsd<R: Range> {
    /// `None`: the volume did not mount, or its device failed since; every request is corrupt.
    fs: Option<Filesystem<Blocks<R>>>,
    /// The range refuses writes: every change is refused here, before littlefs is asked.
    read_only: bool,
    labels: Vec<u64>,
    /// The last `get_attr`'s value, which its reply borrows.
    attr: Vec<u8>,
}

impl<R: Range> Fsd<R> {
    /// The server for what [`crate::volume::mount`] found, under `labels`. A volume whose ids do
    /// not hold together ([`ids_are_sound`]) is served as corrupt.
    pub fn new(mounted: Mounted<R>, labels: Vec<u64>) -> Fsd<R> {
        let (fs, read_only) = match mounted {
            Mounted::Files { mut fs, read_only, blocks } => match ids_are_sound(&mut fs, blocks) {
                Ok(()) => (Some(fs), read_only),
                Err(_) => (None, read_only),
            },
            Mounted::Corrupt(_) => (None, false),
        };
        Fsd { fs, read_only, labels, attr: Vec::new() }
    }

    /// Refuses a change to a read-only volume, so a refused write never reaches the device and
    /// never poisons the volume for everyone.
    pub(crate) fn writable(&self) -> Result<(), Failure> {
        if self.read_only { Err(Failure::ReadOnly) } else { Ok(()) }
    }

    /// The path of `dir`'s entry `name`, once `dir` is found again.
    pub(crate) fn found_child(&mut self, dir: &Node, name: &str) -> Result<String, Failure> {
        self.find(dir)?;
        dir.child(name)
    }

    pub(crate) fn volume_labels(&self) -> &[u64] { &self.labels }

    /// Keeps `value` for a `get_attr` reply to borrow.
    pub(crate) fn keep_attr(&mut self, value: Vec<u8>) -> &[u8] {
        self.attr = value;
        &self.attr
    }

    /// Runs `op` on the filesystem. An I/O error poisons the volume until it is mounted again
    /// (servers/fsd.md, "Failure and restart"): from then on every request is corrupt.
    pub(crate) fn with<T>(
        &mut self,
        op: impl FnOnce(&mut Filesystem<Blocks<R>>) -> Result<T, FsError>,
    ) -> Result<T, Failure> {
        let fs = self.fs.as_mut().ok_or(Failure::Fs(FsError::Corrupt))?;
        let r = op(fs);
        if r.as_ref().is_err_and(|e| *e == FsError::Io) {
            self.fs = None;
        }
        r.map_err(Failure::Fs)
    }

    /// Finds `node`'s file again: it must still be at its path with its id. Anything else is
    /// `removed`, so a fid never reaches a file that took the place of the one it was on.
    pub(crate) fn find(&mut self, node: &Node) -> Result<(), Failure> {
        if node.id == ROOT_ID {
            return if node.path.is_empty() { self.with(|_| Ok(())) } else { Err(Failure::Removed) };
        }
        match self.with(|fs| fs.get_attr(&node.path, ATTR_ID)) {
            Ok(id) if id.as_slice() == node.id.to_le_bytes() => Ok(()),
            Ok(_) | Err(Failure::Fs(FsError::NoEntry | FsError::NoAttr | FsError::NotDir)) => {
                Err(Failure::Removed)
            }
            Err(e) => Err(e),
        }
    }

    /// The node for the entry at `path`. Every entry has its id: the mount checked, and every
    /// create writes it in the commit that creates the entry.
    pub(crate) fn node_at(&mut self, path: String) -> Result<Node, Failure> {
        let meta = self.with(|fs| fs.stat(&path))?;
        let id = match self.with(|fs| fs.get_attr(&path, ATTR_ID)) {
            Ok(id) => decode_id(&id)?,
            Err(Failure::Fs(FsError::NoAttr)) => return Err(Failure::Fs(FsError::Corrupt)),
            Err(e) => return Err(e),
        };
        Ok(Node { path, id, dir: meta.kind == FileType::Dir })
    }

    /// The id for a create, taken from the root's counter, which moves past it in a commit of its
    /// own before the entry is created with the id in its creating commit: a power cut between
    /// the two skips an id and never leaves an entry without one. Only a create calls this, and a
    /// create needs the volume's labels exactly, so no read moves the counter.
    pub(crate) fn next_id(&mut self) -> Result<u64, Failure> {
        self.writable()?;
        let id = match self.with(|fs| fs.get_attr("", ATTR_NEXT_ID)) {
            Ok(next) => decode_id(&next)?,
            Err(Failure::Fs(FsError::NoAttr)) => ROOT_ID + 1,
            Err(e) => return Err(e),
        };
        let next = id.checked_add(1).ok_or(Failure::Fs(FsError::NoSpace))?;
        self.with(|fs| fs.set_attr("", ATTR_NEXT_ID, &next.to_le_bytes()))?;
        Ok(id)
    }

    /// `node`'s qid: its id, and for a file its version.
    fn qid(&mut self, node: &Node) -> Result<Qid, Failure> {
        if node.dir {
            return Ok(Qid { kind: QTDIR, version: 0, path: node.id });
        }
        let version = match self.with(|fs| fs.get_attr(&node.path, ATTR_VERSION)) {
            Ok(v) => {
                v.as_slice().try_into().map(u32::from_le_bytes).map_err(|_| Failure::Fs(FsError::Corrupt))?
            }
            Err(Failure::Fs(FsError::NoAttr)) => 0,
            Err(e) => return Err(e),
        };
        Ok(Qid { kind: 0, version, path: node.id })
    }

    /// Moves `node`'s qid version on, before the change it announces: a change that then fails
    /// leaves a client re-reading what did not change, never trusting what did.
    fn bump(&mut self, node: &Node) -> Result<(), Failure> {
        let next = self.qid(node)?.version.wrapping_add(1);
        self.with(|fs| fs.set_attr(&node.path, ATTR_VERSION, &next.to_le_bytes()))
    }

    /// Opens `node`'s file, runs `op` on the handle, and closes it whatever `op` did.
    fn on_file<T>(
        &mut self,
        node: &Node,
        options: OpenOptions,
        op: impl FnOnce(&mut Filesystem<Blocks<R>>, FileHandle) -> Result<T, FsError>,
    ) -> Result<T, Failure> {
        self.find(node)?;
        self.with(|fs| {
            let h = fs.open(&node.path, options)?;
            let r = op(fs, h);
            let closed = fs.close(h);
            let value = r?;
            closed.map(|()| value)
        })
    }

    /// The entry `index` of directory `dir` whose name is one a client could walk to; entries
    /// with names no path can name are not listed, so no such name is ever joined into a path.
    fn entry_name(&mut self, dir: &Node, index: u64) -> Result<Option<String>, Failure> {
        self.find(dir)?;
        let mut seen = 0u64;
        let mut found: Result<Option<String>, Failure> = Ok(None);
        self.with(|fs| {
            fs.read_dir(&dir.path, |entry| {
                let Ok(name) = core::str::from_utf8(entry.name) else { return };
                if !path::valid_name(name) || found.as_ref().map_or(true, Option::is_some) {
                    return;
                }
                if seen == index {
                    let mut owned = String::new();
                    found = match owned.try_reserve(name.len()) {
                        Ok(()) => {
                            owned.push_str(name);
                            Ok(Some(owned))
                        }
                        Err(_) => Err(Failure::NoMemory),
                    };
                }
                seen += 1;
            })
        })?;
        found
    }

    fn stat_of(&mut self, node: &Node) -> Result<FileStat, Failure> {
        self.find(node)?;
        let meta = self.with(|fs| fs.stat(&node.path))?;
        let mut name = String::new();
        name.try_reserve(node.name().len()).map_err(|_| Failure::NoMemory)?;
        name.push_str(node.name());
        let mode = if node.dir { DMDIR | 0o755 } else { 0o644 };
        let qid = self.qid(node)?;
        Ok(FileStat { qid, mode, mtime: 0, length: u64::from(meta.size), name })
    }
}

/// Why a request on the files failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// What littlefs said.
    Fs(FsError),
    /// The file a node names is gone: removed, or another in its place.
    Removed,
    /// No memory for a name.
    NoMemory,
    /// A change to a read-only volume.
    ReadOnly,
}

/// A stored id: eight bytes, never the root's.
fn decode_id(bytes: &[u8]) -> Result<u64, Failure> {
    bytes
        .try_into()
        .map(u64::from_le_bytes)
        .ok()
        .filter(|id| *id != ROOT_ID)
        .ok_or(Failure::Fs(FsError::Corrupt))
}

/// Room for one more in `v`, or `NoSpace`.
fn room<T>(v: &mut Vec<T>) -> Result<(), FsError> { v.try_reserve(1).map_err(|_| FsError::NoSpace) }

/// Whether `v`, sorted here, holds a value twice.
fn repeats<T: Ord>(v: &mut [T]) -> bool {
    v.sort_unstable();
    v.windows(2).any(|w| w[0] == w[1])
}

/// Whether the volume is one `fsd` wrote, checked without writing: every file and directory
/// has a name a client could have created and an id, no two ids are the same, no two
/// directories share a block of their first pair, and the root's counter is above every id,
/// so no create gives an id already in use. An image that breaks any of these (another
/// implementation's, or a forged one, which could otherwise let a fid on one file reach
/// another) is corrupt.
///
/// The walk reads each directory by its pair, never by a path resolved again from the root,
/// and the pairs it reads, all directories together, may not outnumber the volume's
/// (`blocks / 2`), each pair belonging to one directory in a sound volume. So the check is
/// linear in the volume whatever the image: a directory pointing back at an ancestor, or two
/// sharing a pair, spends the budget or repeats a block and is refused. Depth is no criterion:
/// renames can honestly build a tree deeper than any walk from one root reaches.
fn ids_are_sound<D: BlockDevice>(fs: &mut Filesystem<D>, blocks: u32) -> Result<(), FsError> {
    let (mut ids, mut heads, mut dirs): (Vec<u64>, Vec<u32>, Vec<DirRef>) =
        (Vec::new(), Vec::new(), Vec::new());
    let root = fs.root_dir();
    room(&mut heads)?;
    heads.extend_from_slice(&root.blocks());
    room(&mut dirs)?;
    dirs.push(root);
    let mut pairs = blocks / 2;
    while let Some(dir) = dirs.pop() {
        let mut failed = None;
        let read = fs.read_dir_at(dir, |entry| {
            if failed.is_some() {
                return;
            }
            let named = core::str::from_utf8(entry.name).is_ok_and(path::valid_name);
            let Some(Ok(id)) = entry.attr(ATTR_ID).map(decode_id).filter(|_| named) else {
                failed = Some(FsError::Corrupt);
                return;
            };
            if room(&mut ids).is_err() {
                failed = Some(FsError::NoSpace);
                return;
            }
            ids.push(id);
            if let Some(child) = entry.dir() {
                if heads.try_reserve(2).is_err() || room(&mut dirs).is_err() {
                    failed = Some(FsError::NoSpace);
                    return;
                }
                heads.extend_from_slice(&child.blocks());
                dirs.push(child);
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        pairs = pairs.checked_sub(read).ok_or(FsError::Corrupt)?;
    }
    if repeats(&mut ids) || repeats(&mut heads) {
        return Err(FsError::Corrupt);
    }
    let highest = ids.last().copied().unwrap_or(ROOT_ID);
    let next = match fs.get_attr("", ATTR_NEXT_ID) {
        Ok(next) => decode_id(&next).map_err(|_| FsError::Corrupt)?,
        Err(FsError::NoAttr) => ROOT_ID + 1,
        Err(e) => return Err(e),
    };
    if next <= highest { Err(FsError::Corrupt) } else { Ok(()) }
}

/// A failure as a 9P error. littlefs's `Corrupt`, an I/O error and the poisoning a failed write
/// leaves are all `corrupt`: the client learns the volume is broken, not a refusal.
pub(crate) fn nine(e: Failure) -> NineError {
    match e {
        Failure::Removed => text::REMOVED,
        Failure::NoMemory => NineError::NO_MEMORY,
        Failure::ReadOnly => text::READ_ONLY,
        Failure::Fs(FsError::Io | FsError::Corrupt | FsError::Poisoned) => text::CORRUPT,
        Failure::Fs(FsError::NoEntry | FsError::NoAttr) => NineError::NOT_FOUND,
        Failure::Fs(FsError::Exists) => text::EXISTS,
        Failure::Fs(FsError::NotDir) => NineError::NOT_DIR,
        Failure::Fs(FsError::IsDir) => text::IS_DIR,
        Failure::Fs(FsError::NotEmpty) => text::NOT_EMPTY,
        Failure::Fs(FsError::NoSpace) => text::NO_SPACE,
        Failure::Fs(FsError::FileTooBig) => text::TOO_BIG,
        Failure::Fs(FsError::NameTooLong) => NineError::BAD_NAME,
        Failure::Fs(FsError::Invalid) => NineError::PERMISSION,
    }
}

/// The same failure as the typed operations' error (libs/wire/tables/fsd.md).
pub(crate) fn code(e: Failure) -> ErrorCode {
    match e {
        Failure::Removed => ErrorCode::Removed,
        Failure::NoMemory | Failure::ReadOnly => ErrorCode::Refused,
        Failure::Fs(FsError::Io | FsError::Corrupt | FsError::Poisoned) => ErrorCode::Corrupt,
        Failure::Fs(FsError::NoEntry | FsError::NoAttr) => ErrorCode::NotFound,
        Failure::Fs(FsError::Exists) => ErrorCode::Exists,
        Failure::Fs(FsError::NotDir) => ErrorCode::NotDir,
        Failure::Fs(FsError::NoSpace | FsError::FileTooBig) => ErrorCode::TooLarge,
        Failure::Fs(FsError::IsDir | FsError::NotEmpty | FsError::Invalid | FsError::NameTooLong) => {
            ErrorCode::Refused
        }
    }
}

impl<R: Range> FileServer for Fsd<R> {
    type Node = Node;

    /// Every badge of the founding kind attaches at the volume's root; `aname` is ignored. A
    /// volume that did not mount answers `corrupt`.
    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Node, Qid), NineError> {
        if self.fs.is_none() {
            return Err(text::CORRUPT);
        }
        let root = Node::root();
        let qid = self.qid(&root).map_err(nine)?;
        Ok((root, qid))
    }

    /// The volume's labels, for every node (servers/fsd.md: labels are per volume).
    fn labels(&self, _: &Node) -> &[u64] { &self.labels }

    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        self.find(dir).map_err(nine)?;
        let node = self.node_at(join(&dir.path, name)?).map_err(nine)?;
        let qid = self.qid(&node).map_err(nine)?;
        Ok((node, qid))
    }

    fn open(&mut self, _: &Caller, node: &Node, m: u8) -> Result<Qid, NineError> {
        if matches!(m & 3, mode::OWRITE | mode::ORDWR) || m & mode::OTRUNC != 0 {
            self.writable().map_err(nine)?;
        }
        self.find(node).map_err(nine)?;
        if m & mode::OTRUNC != 0 && !node.dir {
            self.bump(node).map_err(nine)?;
            let truncate = OpenOptions { write: true, truncate: true, ..OpenOptions::default() };
            self.on_file(node, truncate, |_, _| Ok(())).map_err(nine)?;
        }
        self.qid(node).map_err(nine)
    }

    fn read(&mut self, _: &Caller, node: &Node, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        // Past what littlefs can address there is nothing to read.
        let Ok(offset) = u32::try_from(offset) else { return Ok(Read::Done(0)) };
        let read = OpenOptions { read: true, ..OpenOptions::default() };
        self.on_file(node, read, |fs, h| {
            if offset >= fs.file_size(h)? {
                return Ok(0);
            }
            fs.seek(h, offset)?;
            fs.read(h, out)
        })
        .map(Read::Done)
        .map_err(nine)
    }

    fn write(&mut self, _: &Caller, node: &Node, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let offset = u32::try_from(offset).map_err(|_| text::TOO_BIG)?;
        self.writable().map_err(nine)?;
        self.find(node).map_err(nine)?;
        self.bump(node).map_err(nine)?;
        let write = OpenOptions { write: true, ..OpenOptions::default() };
        self.on_file(node, write, |fs, h| {
            fs.seek(h, offset)?;
            fs.write(h, data)
        })
        .map_err(nine)
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> {
        self.stat_of(node).map_err(nine)
    }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &Node,
        index: u64,
    ) -> Result<Option<(Node, FileStat)>, NineError> {
        let Some(name) = self.entry_name(dir, index).map_err(nine)? else { return Ok(None) };
        let node = self.node_at(join(&dir.path, &name)?).map_err(nine)?;
        let stat = self.stat_of(&node).map_err(nine)?;
        Ok(Some((node, stat)))
    }

    fn create(
        &mut self,
        _: &Caller,
        dir: &Node,
        name: &str,
        perm: u32,
        _: u8,
    ) -> Result<(Node, Qid), NineError> {
        self.writable().map_err(nine)?;
        let path = self.found_child(dir, name).map_err(nine)?;
        let id = self.next_id().map_err(nine)?;
        let attrs: [(u8, &[u8]); 1] = [(ATTR_ID, &id.to_le_bytes())];
        let dir = perm & DMDIR != 0;
        if dir {
            self.with(|fs| fs.mkdir_with_attrs(&path, &attrs)).map_err(nine)?;
        } else {
            let new = OpenOptions { write: true, create_new: true, ..OpenOptions::default() };
            self.with(|fs| {
                let h = fs.open_with_attrs(&path, new, &attrs)?;
                fs.close(h)
            })
            .map_err(nine)?;
        }
        let node = Node { path, id, dir };
        let qid = self.qid(&node).map_err(nine)?;
        Ok((node, qid))
    }

    /// Removes the file or empty directory. Every other fid on it finds it gone (its id is
    /// nowhere on the volume any more) and gets `removed`; littlefs's readable-after-remove
    /// handles are never used, since no handle outlives a request.
    fn remove(&mut self, _: &Caller, node: &Node) -> Result<(), NineError> {
        if node.id == ROOT_ID {
            return Err(NineError::PERMISSION);
        }
        self.writable().map_err(nine)?;
        self.find(node).map_err(nine)?;
        self.with(|fs| fs.remove(&node.path)).map_err(nine)
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
pub(crate) mod tests;
