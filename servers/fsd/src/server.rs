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

use crate::quota::{Ledger, Refusal};
use crate::volume::{BLOCK, Blocks, Mounted, Range};

/// What admission lets each of `buckets` buckets hold (servers/serving.md R26); the count is the
/// manifest's `buckets=N`. Nothing is parked: every request is answered as it arrives.
pub const fn limits(buckets: u32) -> Limits {
    Limits { buckets, in_flight: 0, files: 32, state: 8, requests: 0, pages: 0 }
}
/// What one of each costs, in bytes: a fid is its table entry and a node per step from its root,
/// each a path; a minted connection its record and its root's path.
pub const COST: Cost = Cost { in_flight: 0, file: 2048, state: 512, request: 0, page: 0 };
/// The bytes of this server's budget its clients may use between them.
pub const BUDGET: u64 = 2 * 1024 * 1024;

/// Test-only, for the bench's `fsd-restart` (feature `restart-probe`, off in every default build,
/// as netd's is): a walk to this name ends the instance with [`PROBE_EXIT`] while it holds the
/// call, so its caller gets `Dead` and `init` restarts `fsd`. Only the client that walks there
/// triggers it, and it walks there once.
#[cfg(feature = "restart-probe")]
pub const PROBE: &str = "fsd-restart-probe";
#[cfg(feature = "restart-probe")]
pub const PROBE_EXIT: u32 = 9;

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

/// The blocks littlefs holds for itself: the superblock pair, which is also the root
/// directory's first pair.
const SUPERBLOCK: u32 = 2;
/// A metadata pair's bytes.
const PAIR: u64 = 2 * BLOCK as u64;

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
    /// A connection's quota is more than the root above it has room for.
    pub const QUOTA_REFUSED: NineError = NineError("quota refused");
    pub const TOO_BIG: NineError = NineError("file too large");
    pub const READ_ONLY: NineError = NineError("read-only volume");
}

/// Why the arguments were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

/// What `fsd`'s arguments other than `buckets=` say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args<'a> {
    /// The manifest name of the endpoint it receives on (`fsd:data`): its startup block holds
    /// that endpoint under this name.
    pub endpoint: &'a str,
    /// The volume's label set; empty when `labels=` is absent.
    pub labels: Vec<u64>,
}

/// The arguments other than `buckets=`: `endpoint=NAME` exactly once, a name under the
/// manifest's rule, never defaulted; and `labels=ID[,ID...]` at most once, each ID decimal
/// without leading zeros, at most `MAX_LABELS`, absent for an empty set. Anything else is refused,
/// so `fsd` never serves under an endpoint or labels it misread.
pub fn parse_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<Args<'a>, BadArgs> {
    let (mut endpoint, mut labels) = (None, None);
    for arg in args {
        if let Some(name) = arg.strip_prefix("endpoint=") {
            if endpoint.is_some() || !redoubt_rt::startup::valid_name(name) {
                return Err(BadArgs);
            }
            endpoint = Some(name);
            continue;
        }
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
    Ok(Args { endpoint: endpoint.ok_or(BadArgs)?, labels: labels.unwrap_or_default() })
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

/// The most entries a listing window holds (servers/fsd.md, "A directory is listed a window at a
/// time"). One `read` reply holds at most E = 1310 entries: the most a reply carries at the
/// largest msize, `MSIZE - IOHDRSZ` = 65512 bytes, over the shortest stat, 50 bytes (41 fixed,
/// a one-byte name and three empty strings). So one request costs at most
/// ceil(E / `WINDOW`) + 1 = 22 passes over the directory it reads.
const WINDOW: usize = 64;

/// A listing window: up to [`WINDOW`] entries of directory `dir` from index `start`, with all
/// a directory read returns of them, read in one pass at the change generation `changes`.
#[derive(Default)]
struct Window {
    /// `None`: nothing is held.
    dir: Option<Node>,
    changes: u64,
    start: u64,
    /// Fewer than [`WINDOW`]: the directory ends after them.
    entries: Vec<Listed>,
    /// The entries' names, end to end; at most [`WINDOW`] of [`path::MAX_NAME`] bytes.
    names: String,
}

/// One entry of a [`Window`].
struct Listed {
    /// Where its name ends in [`Window::names`]; it starts where the last one's ended.
    end: usize,
    id: u64,
    dir: bool,
    size: u32,
    version: u32,
}

impl Window {
    /// Whether the window answers for entry `index` of `dir` at the generation `changes`: the
    /// entry is in it, or the directory ends within it.
    fn holds(&self, dir: &Node, changes: u64, index: u64) -> bool {
        let inside = index
            .checked_sub(self.start)
            .is_some_and(|i| i < self.entries.len() as u64 || self.entries.len() < WINDOW);
        self.dir.as_ref() == Some(dir) && self.changes == changes && inside
    }

    /// Keeps the entry `name`, with what a directory read returns of it.
    fn keep(&mut self, name: &str, entry: &littlefs::DirEntry) -> Result<(), Failure> {
        let id = entry.attr(ATTR_ID).map_or(Err(Failure::Fs(FsError::Corrupt)), decode_id)?;
        let dir = entry.meta.kind == FileType::Dir;
        let version = match entry.attr(ATTR_VERSION) {
            Some(v) if !dir => {
                v.try_into().map(u32::from_le_bytes).map_err(|_| Failure::Fs(FsError::Corrupt))?
            }
            _ => 0,
        };
        self.names.try_reserve(name.len()).map_err(|_| Failure::NoMemory)?;
        self.names.push_str(name);
        self.entries.push(Listed { end: self.names.len(), id, dir, size: entry.meta.size, version });
        Ok(())
    }

    /// Entry `index`'s node in `dir` and its stat, or `None` past the directory's end. The
    /// window holds it ([`Window::holds`]).
    fn entry(&self, dir: &Node, index: u64) -> Result<Option<(Node, FileStat)>, NineError> {
        let i = usize::try_from(index - self.start).unwrap_or(usize::MAX);
        let Some(e) = self.entries.get(i) else { return Ok(None) };
        let begin = i.checked_sub(1).map_or(0, |b| self.entries[b].end);
        let name = &self.names[begin..e.end];
        let mut owned = String::new();
        owned.try_reserve(name.len()).map_err(|_| NineError::NO_MEMORY)?;
        owned.push_str(name);
        let node = Node { path: join(&dir.path, name)?, id: e.id, dir: e.dir };
        let (mode, kind) = if e.dir { (DMDIR | 0o755, QTDIR) } else { (0o644, 0) };
        let qid = Qid { kind, version: e.version, path: e.id };
        Ok(Some((node, FileStat { qid, mode, mtime: 0, length: u64::from(e.size), name: owned })))
    }
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
    /// The volume's blocks.
    blocks: u32,
    /// What each live root holds (servers/fsd.md, "Quotas").
    pub(crate) ledger: Ledger,
    /// The change generation: moved by every change to the volume, so a window filled before
    /// it is never served after it.
    changes: u64,
    /// The one listing window, whoever lists.
    window: Window,
    /// How many times littlefs itself ran out of room: a quota that kept its promise never
    /// lets it (servers/fsd.md, "Quotas").
    #[cfg(test)]
    pub(crate) out_of_room: u32,
    /// How many passes over a directory listings made.
    #[cfg(test)]
    pub(crate) passes: u32,
}

impl<R: Range> Fsd<R> {
    /// The server for what [`crate::volume::mount`] found, under `labels`. A volume whose ids do
    /// not hold together ([`ids_are_sound`]) is served as corrupt.
    ///
    /// The volume root's quota is the volume's blocks less [`SUPERBLOCK`], and it holds what the
    /// volume holds beyond the superblock pair.
    pub fn new(mounted: Mounted<R>, labels: Vec<u64>) -> Fsd<R> {
        let (fs, read_only, blocks, held) = match mounted {
            Mounted::Files { mut fs, read_only, blocks } => {
                // No pair is made but in the room a change's root has ([`Fsd::recounted`]).
                fs.set_pair_room(0);
                let root = fs.root_dir();
                match ids_are_sound(&mut fs, blocks).and_then(|()| tally(&mut fs, root, blocks, &[])) {
                    Ok((held, _)) => {
                        (Some(fs), read_only, blocks, held.saturating_sub(u64::from(SUPERBLOCK * BLOCK)))
                    }
                    Err(_) => (None, read_only, 0, 0),
                }
            }
            Mounted::Corrupt(_) => (None, false, 0, 0),
        };
        // No reserve beyond the superblock (servers/fsd.md, "Quotas", no promise the disk cannot
        // keep): littlefs takes no block outside a root in one operation. Its only allocations
        // are a file's blocks, which the room check counts (a rewrite's whole new tail, a copy in
        // full) before they are taken, and new metadata pairs, which it makes only within the
        // pair room the charged root has ([`Fsd::recounted`]); every other commit (the id
        // counter's, a rename's source, an orphan repair, a removed directory's list link) gets
        // none, and compaction keeps such a directory in the pairs it has. littlefs never grows
        // the superblock chain or relocates a pair.
        let room = u64::from(blocks.saturating_sub(SUPERBLOCK)) * u64::from(BLOCK);
        Fsd {
            fs,
            read_only,
            labels,
            attr: Vec::new(),
            blocks,
            ledger: Ledger::new(room, held),
            changes: 0,
            window: Window::default(),
            #[cfg(test)]
            out_of_room: 0,
            #[cfg(test)]
            passes: 0,
        }
    }

    /// Whether the volume is served as corrupt: every attach refused with `corrupt`.
    pub fn is_corrupt(&self) -> bool { self.fs.is_none() }

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
        #[cfg(test)]
        if r.as_ref().is_err_and(|e| *e == FsError::NoSpace) {
            self.out_of_room += 1;
        }
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

    /// The root holding the directory `dir`, if it can grow by `need`; otherwise `no space`
    /// (servers/fsd.md, "Quotas").
    pub(crate) fn room(&self, dir: &str, need: u64) -> Result<usize, Failure> {
        let i = self.ledger.holder(dir);
        if self.ledger.fits(i, need) { Ok(i) } else { Err(Failure::Fs(FsError::NoSpace)) }
    }

    /// Runs `change`, which commits to the directories `dirs`, and charges the root holding
    /// each for the metadata pairs the directory gained or lost, whether or not the change
    /// succeeded: littlefs splits a directory when a commit to it compacts, and drops a pair
    /// that empties. It splits one, or makes a new directory's pair, only into the room `root`
    /// has beyond `need`, whole pairs of it (servers/fsd.md, "Quotas"); short of a pair,
    /// compaction keeps a directory in the pairs it has.
    pub(crate) fn recounted<T, const N: usize>(
        &mut self,
        root: usize,
        need: u64,
        dirs: [&str; N],
        change: impl FnOnce(&mut Self) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        // Every change to the volume runs here, whether or not it succeeds.
        self.changes = self.changes.wrapping_add(1);
        let mut before = [0u32; N];
        for (n, dir) in before.iter_mut().zip(dirs) {
            *n = self.with(|fs| fs.read_dir(dir, |_| {}))?;
        }
        let pairs = self.ledger.spare(root).saturating_sub(need) / PAIR;
        if let Some(fs) = self.fs.as_mut() {
            // `u32::MAX` would be no limit at all.
            fs.set_pair_room(u32::try_from(pairs).unwrap_or(u32::MAX).min(u32::MAX - 1));
        }
        let changed = change(self);
        if let Some(fs) = self.fs.as_mut() {
            fs.set_pair_room(0);
        }
        for (i, dir) in dirs.iter().enumerate() {
            if dirs[..i].contains(dir) {
                continue;
            }
            // A volume that failed meanwhile is corrupt from now on; nothing more to count.
            let Ok(after) = self.with(|fs| fs.read_dir(dir, |_| {})) else { break };
            let pairs = |n: u32| u64::from(n) * PAIR;
            let root = self.ledger.holder(dir);
            self.ledger.change(root, pairs(after), pairs(before[i]));
        }
        changed
    }

    /// Counts every live root afresh and checks the ledger agrees: what it holds, its reserve,
    /// and that it is within its quota unless `over` allows it.
    #[cfg(test)]
    pub(crate) fn audit(&mut self, over: bool) {
        let live = self.ledger.charges().unwrap();
        let blocks = self.blocks;
        for (id, path, quota, held, reserve) in self.ledger.roots() {
            let counted = self.with(|fs| {
                let dir = dir_ref(fs, &path)?;
                tally(fs, dir, blocks, &live)
            });
            let (mut counted, counted_reserve) = counted.unwrap();
            if id == ROOT_ID {
                counted -= u64::from(SUPERBLOCK * BLOCK);
            }
            assert_eq!((held, reserve), (counted, counted_reserve), "the root at {path:?}");
            assert!(over || held + reserve <= quota, "the root at {path:?}: {held} + {reserve} > {quota}");
        }
    }

    /// What the entry at `path` holds: a file's bytes, or all that lies under a directory, its
    /// own pairs included. No live root is at or under it.
    pub(crate) fn holds(&mut self, path: &str) -> Result<u64, Failure> {
        let meta = self.with(|fs| fs.stat(path))?;
        if meta.kind == FileType::File {
            return self.with(|fs| Ok(cost(fs, meta.size)));
        }
        let blocks = self.blocks;
        self.with(|fs| {
            let dir = dir_ref(fs, path)?;
            tally(fs, dir, blocks, &[])
        })
        .map(|(held, _)| held)
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

    /// Fills the window with directory `dir`'s entries from `index`, in one pass, once `dir` is
    /// found again. Only entries whose names a client could walk to count; those no path can
    /// name are not listed, so no such name is ever joined into a path.
    fn fill(&mut self, dir: &Node, index: u64) -> Result<(), Failure> {
        self.find(dir)?;
        let mut w = core::mem::take(&mut self.window);
        w.entries.clear();
        w.names.clear();
        w.entries.try_reserve(WINDOW).map_err(|_| Failure::NoMemory)?;
        let path = join("", &dir.path).map_err(|_| Failure::NoMemory)?;
        let (mut seen, mut kept) = (0u64, Ok(()));
        #[cfg(test)]
        {
            self.passes += 1;
        }
        self.with(|fs| {
            fs.read_dir(&dir.path, |entry| {
                let Ok(name) = core::str::from_utf8(entry.name) else { return };
                if !path::valid_name(name) || kept.is_err() || w.entries.len() == WINDOW {
                    return;
                }
                if seen >= index {
                    kept = w.keep(name, entry);
                }
                seen += 1;
            })
        })?;
        kept?;
        (w.dir, w.changes, w.start) = (Some(Node { path, id: dir.id, dir: dir.dir }), self.changes, index);
        self.window = w;
        Ok(())
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

/// The directory `path` lies in.
pub(crate) fn parent(path: &str) -> &str { path.rsplit_once('/').map_or("", |(dir, _)| dir) }

/// The bytes a file of `size` holds: its skip-list's whole blocks, or its length inline.
fn cost<D: BlockDevice>(fs: &Filesystem<D>, size: u32) -> u64 {
    match fs.file_blocks(size) {
        0 => u64::from(size),
        n => u64::from(n) * u64::from(BLOCK),
    }
}

/// The room a write that leaves a file of `old` bytes `new` bytes long, from byte `from`,
/// needs until it commits: littlefs rewrites the file from the block holding `from` to its
/// end before the commit frees the old blocks (servers/fsd.md, "Quotas": a rewrite counts what
/// it writes), and the file may grow.
fn rewrite<D: BlockDevice>(fs: &Filesystem<D>, old: u32, from: u32, new: u32) -> u64 {
    // The blocks before the one holding `from` are kept: as many as a file of `from + 1` bytes
    // has, but its last. An inline file keeps none.
    let kept = match fs.file_blocks(old) {
        0 => 0,
        _ => fs.file_blocks(from.saturating_add(1)).saturating_sub(1),
    };
    let written = u64::from(fs.file_blocks(new).saturating_sub(kept)) * u64::from(BLOCK);
    written.max(cost(fs, new).saturating_sub(cost(fs, old)))
}

/// The directory at `path`, by its first pair.
fn dir_ref<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str) -> Result<DirRef, FsError> {
    if path.is_empty() {
        return Ok(fs.root_dir());
    }
    let name = path.rsplit('/').next().unwrap_or(path).as_bytes();
    let mut found = None;
    fs.read_dir(parent(path), |entry| {
        if entry.name == name {
            found = entry.dir();
        }
    })?;
    found.ok_or(FsError::NotDir)
}

/// What the directory `top` holds, walked by its pairs and bounded by the volume's pairs as
/// the mount walk is (servers/fsd.md, "Quotas"): its pairs and those of every directory under
/// it, whole blocks for a file in blocks and the length of an inline one; for a live root
/// below it (`live`: id and charge), that root's charge in reserve instead of what lies under
/// it. Returns (held, reserve).
fn tally<D: BlockDevice>(
    fs: &mut Filesystem<D>,
    top: DirRef,
    blocks: u32,
    live: &[(u64, u64)],
) -> Result<(u64, u64), FsError> {
    let (mut dirs, mut sizes): (Vec<DirRef>, Vec<u32>) = (Vec::new(), Vec::new());
    room(&mut dirs)?;
    dirs.push(top);
    let (mut pairs, mut held, mut reserve) = (blocks / 2, 0u64, 0u64);
    while let Some(dir) = dirs.pop() {
        let mut failed = None;
        let entry = |entry: &littlefs::DirEntry| {
            let Some(child) = entry.dir() else {
                match room(&mut sizes) {
                    Ok(()) => sizes.push(entry.meta.size),
                    Err(e) => failed = Some(e),
                }
                return;
            };
            let id = entry.attr(ATTR_ID).and_then(|id| decode_id(id).ok());
            match live.iter().find(|(root, _)| Some(*root) == id) {
                Some((_, charge)) => reserve = reserve.saturating_add(*charge),
                None => match room(&mut dirs) {
                    Ok(()) => dirs.push(child),
                    Err(e) => failed = Some(e),
                },
            }
        };
        let read = fs.read_dir_at(dir, entry, |_| Ok(()))?;
        if let Some(e) = failed {
            return Err(e);
        }
        pairs = pairs.checked_sub(read).ok_or(FsError::Corrupt)?;
        held = held.saturating_add(u64::from(read) * 2 * u64::from(BLOCK));
        for size in sizes.drain(..) {
            held = held.saturating_add(cost(fs, size));
        }
    }
    Ok((held, reserve))
}

/// Room for one more in `v`, or `NoSpace`.
fn room<T>(v: &mut Vec<T>) -> Result<(), FsError> { v.try_reserve(1).map_err(|_| FsError::NoSpace) }

/// Whether `v`, sorted here, holds a value twice.
fn repeats<T: Ord>(v: &mut [T]) -> bool {
    v.sort_unstable();
    v.windows(2).any(|w| w[0] == w[1])
}

/// Whether the volume is one `fsd` wrote, checked without writing: every file and directory
/// has a name a client could have created and an id, no two ids are the same, no metadata pair
/// is named twice, within one directory's chain or across two, and the root's counter is above
/// every id, so no create gives an id already in use. An image that breaks any of these
/// (another implementation's, or a forged one, which could otherwise let a fid on one file
/// reach another, or put one root's file under another root) is corrupt.
///
/// The walk reads each directory by its pairs, never by a path resolved again from the root,
/// and marks every block of every pair before it reads it: a block marked already is refused
/// before it is read again. So no pair is read twice and the check is linear in the volume
/// whatever the image: a directory pointing back at an ancestor, two sharing a pair, a chain
/// running into another's or looping back on itself, all repeat a block. Depth is no
/// criterion: renames can honestly build a tree deeper than any walk from one root reaches.
fn ids_are_sound<D: BlockDevice>(fs: &mut Filesystem<D>, blocks: u32) -> Result<(), FsError> {
    let (mut ids, mut dirs): (Vec<u64>, Vec<DirRef>) = (Vec::new(), Vec::new());
    let mut named: Vec<u64> = Vec::new();
    let words = blocks.div_ceil(64) as usize;
    named.try_reserve_exact(words).map_err(|_| FsError::NoSpace)?;
    named.resize(words, 0);
    room(&mut dirs)?;
    dirs.push(fs.root_dir());
    while let Some(dir) = dirs.pop() {
        let mut failed = None;
        let entry = |entry: &littlefs::DirEntry| {
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
                match room(&mut dirs) {
                    Ok(()) => dirs.push(child),
                    Err(e) => failed = Some(e),
                }
            }
        };
        // `read_dir_at` hands over only pairs inside the volume.
        let mark = |pair: DirRef| {
            for b in pair.blocks() {
                let (word, bit) = ((b / 64) as usize, 1u64 << (b % 64));
                if named[word] & bit != 0 {
                    return Err(FsError::Corrupt);
                }
                named[word] |= bit;
            }
            Ok(())
        };
        fs.read_dir_at(dir, entry, mark)?;
        if let Some(e) = failed {
            return Err(e);
        }
    }
    if repeats(&mut ids) {
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
        Failure::Fs(FsError::NoSpace) => ErrorCode::NoSpace,
        Failure::Fs(FsError::FileTooBig) => ErrorCode::TooLarge,
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

    /// Records the connection's quota, carved from the room of the live root above `root`
    /// (servers/fsd.md, "Quotas"). A directory going live is counted here, once.
    fn minted(
        &mut self,
        caller: &Caller,
        badge: u64,
        _: u64,
        root: &Node,
        quota: u64,
    ) -> Result<(), NineError> {
        let live = self.ledger.charges().ok_or(text::QUOTA_REFUSED)?;
        let (blocks, fs) = (self.blocks, self.fs.as_mut().ok_or(text::CORRUPT)?);
        let count = || {
            let dir = dir_ref(fs, &root.path)?;
            tally(fs, dir, blocks, &live)
        };
        match self.ledger.mint(caller.badge, badge, root.id, &root.path, quota, count) {
            Ok(()) => Ok(()),
            Err(Refusal::Refused) => Err(text::QUOTA_REFUSED),
            Err(Refusal::Count(e)) => {
                if e == FsError::Io {
                    self.fs = None;
                }
                Err(nine(Failure::Fs(e)))
            }
        }
    }

    /// Gives the connection's quota back to the root it was carved from.
    fn disconnected(&mut self, badge: u64) { self.ledger.disconnect(badge) }

    /// The volume's labels, for every node (servers/fsd.md: labels are per volume).
    fn labels(&self, _: &Node) -> &[u64] { &self.labels }

    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        #[cfg(feature = "restart-probe")]
        if name == PROBE {
            redoubt_rt::handle::process_exit(PROBE_EXIT);
        }
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
            // A truncation only gives back, and needs no room.
            let held = self.holds(&node.path).map_err(nine)?;
            let i = self.ledger.holder(parent(&node.path));
            self.recounted(i, 0, [parent(&node.path)], |fsd| {
                fsd.bump(node)?;
                let truncate = OpenOptions { write: true, truncate: true, ..OpenOptions::default() };
                fsd.on_file(node, truncate, |_, _| Ok(()))?;
                fsd.ledger.change(i, 0, held);
                Ok(())
            })
            .map_err(nine)?;
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
        let old = self.with(|fs| fs.stat(&node.path)).map_err(nine)?.size;
        let end = u32::try_from(data.len()).ok().and_then(|n| offset.checked_add(n));
        let new = end.ok_or(text::TOO_BIG)?.max(old);
        let costs =
            |fs: &Filesystem<_>| (rewrite(fs, old, offset.min(old), new), cost(fs, new), cost(fs, old));
        let (need, more, less) = self.fs.as_ref().map_or((0, 0, 0), costs);
        let i = self.room(parent(&node.path), need).map_err(nine)?;
        self.recounted(i, need, [parent(&node.path)], |fsd| {
            fsd.bump(node)?;
            let write = OpenOptions { write: true, ..OpenOptions::default() };
            let written = fsd.on_file(node, write, |fs, h| {
                fs.seek(h, offset)?;
                fs.write(h, data)
            })?;
            fsd.ledger.change(i, more, less);
            Ok(written)
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
        if self.window.holds(dir, self.changes, index) {
            // Nothing changed since the fill found `dir`, unless the volume failed meanwhile.
            self.with(|_| Ok(())).map_err(nine)?;
        } else {
            self.fill(dir, index).map_err(nine)?;
        }
        self.window.entry(dir, index)
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
        let mkdir = perm & DMDIR != 0;
        // A directory holds its first pair from the start; a file holds nothing until written.
        let need = if mkdir { PAIR } else { 0 };
        let i = self.room(&dir.path, need).map_err(nine)?;
        // The id counter is the root's: its commit is to the root directory, before the change
        // has any pair room, so it makes no pair.
        let id = self.next_id().map_err(nine)?;
        // A new directory's own pair comes out of the room the check found.
        self.recounted(i, 0, [&dir.path], |fsd| {
            let attrs: [(u8, &[u8]); 1] = [(ATTR_ID, &id.to_le_bytes())];
            if mkdir {
                fsd.with(|fs| fs.mkdir_with_attrs(&path, &attrs))?;
            } else {
                let new = OpenOptions { write: true, create_new: true, ..OpenOptions::default() };
                fsd.with(|fs| {
                    let h = fs.open_with_attrs(&path, new, &attrs)?;
                    fs.close(h)
                })?;
            }
            fsd.ledger.change(i, need, 0);
            Ok(())
        })
        .map_err(nine)?;
        let node = Node { path, id, dir: mkdir };
        let qid = self.qid(&node).map_err(nine)?;
        Ok((node, qid))
    }

    /// Removes the file or empty directory. Every other fid on it finds it gone (its id is
    /// nowhere on the volume any more) and gets `removed`; littlefs's readable-after-remove
    /// handles are never used, since no handle outlives a request. A live root's directory is
    /// not removed: that would end its connections.
    fn remove(&mut self, _: &Caller, node: &Node) -> Result<(), NineError> {
        if node.id == ROOT_ID || self.ledger.holds_live(&node.path) {
            return Err(NineError::PERMISSION);
        }
        self.writable().map_err(nine)?;
        self.find(node).map_err(nine)?;
        let held = self.holds(&node.path).map_err(nine)?;
        let i = self.ledger.holder(parent(&node.path));
        self.recounted(i, 0, [parent(&node.path)], |fsd| {
            fsd.with(|fs| fs.remove(&node.path))?;
            fsd.ledger.change(i, 0, held);
            Ok(())
        })
        .map_err(nine)
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
pub(crate) mod tests;
