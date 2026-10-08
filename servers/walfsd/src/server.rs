//! The files behind the 9P skeleton: one walfs volume, every node carrying the volume's labels.

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_fileserver::quota::{Ledger, Refusal};
use redoubt_rt::abi::PAGE_SIZE;
use redoubt_rt::ipc::Caller;
use redoubt_rt::path;
use redoubt_rt::server::ninep::{
    DMDIR, FileServer, FileStat, NineError, QTDIR, Qid, REQUEST_STATE, Read, mode,
};
use redoubt_rt::server::{Cost, Limits};
use redoubt_rt::wire::proto::littlefsd::ErrorCode;
use walfs::{Error as FsError, FileHandle, FileType, Filesystem, Metadata, OpenOptions};

use crate::volume::{BLOCK, Blocks, Mounted, Range};

/// What admission lets each of `buckets` buckets hold (servers/serving.md R26); the count is the
/// manifest's `buckets=N`. A call is answered as it arrives; what is held is a multiplexed
/// connection's completion call (`InFlight`, one per session, so two lets a share hold one), its
/// requests (`Requests`) and the pages their transfers brought (`Pages`).
pub const fn limits(buckets: u32) -> Limits {
    Limits { buckets, in_flight: 2, files: 32, state: 8, requests: 128, pages: 32 }
}
/// What one of each costs, in bytes: a completion call holds its caller's lend, charged to this
/// server until it replies (kernel/ipc.md R3), `MAX_LEND_PAGES` pages at worst; a fid is its
/// table entry and a node per step from its root, each a path; a minted connection its record and
/// its root's path; a request its record; a page a page.
pub const COST: Cost =
    Cost { in_flight: 64 * 1024, file: 2048, state: 512, request: REQUEST_STATE, page: PAGE_SIZE as u64 };
/// The bytes of this server's budget its clients may use between them. A bucket at its caps costs
/// 2 completion calls at 64 KiB, 32 fids at 2 KiB, 8 connections at 512 bytes, 128 requests at 256
/// and 32 pages at 4 KiB: 364 544 bytes, so the manifests' 4 buckets take 1 458 176, and 5 fit.
pub const BUDGET: u64 = 2 * 1024 * 1024;

/// The attribute types a client's `set_attr` may not touch, 0 to 15, as on `littlefsd`, so a
/// client sees one contract from both writable servers; walfs keeps nothing of `walfsd`'s in
/// them, and type 0 ends a walfs attribute area.
pub const OWN_ATTRS: u8 = 16;

/// The `Rerror` texts `walfsd` adds to the skeleton's fixed set: `littlefsd`'s.
pub mod text {
    use redoubt_rt::server::ninep::NineError;

    /// The volume is damaged where the request read it, or its device failed (`corrupt`).
    pub const CORRUPT: NineError = NineError("corrupt");
    /// The file was removed, or another took its inode, since the fid was walked to it.
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

/// What a fid rests on: a path from the volume's root, built only from names clients walked or
/// created (never from a name read off the medium), and the inode and generation the file had
/// when the fid got there, which name it for the volume's life (servers/walfsd.md, "Inodes"). A
/// node holds no resource; every request finds its file again and checks the pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    path: String,
    ino: u32,
    generation: u64,
    dir: bool,
}

impl Node {
    /// Where the node is, from the volume's root.
    pub fn path(&self) -> &str { &self.path }

    /// The last component, or `/` for the root.
    fn name(&self) -> &str { self.path.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("/") }

    /// Its qid: the inode as the path, the generation's low 32 bits as the version; never read
    /// from the medium again.
    fn qid(&self) -> Qid {
        let kind = if self.dir { QTDIR } else { 0 };
        Qid { kind, version: self.generation as u32, path: u64::from(self.ino) }
    }

    fn at(path: String, m: &Metadata) -> Node {
        Node { path, ino: m.inode, generation: m.generation, dir: m.kind == FileType::Dir }
    }
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

/// The most entries a listing window holds, as `littlefsd`'s (servers/littlefsd.md, "A directory
/// is listed a window at a time"). One `read` reply holds at most 1310 entries, so one request
/// costs at most ceil(1310 / `WINDOW`) + 1 = 22 passes over the directory it reads.
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
    meta: Metadata,
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

    /// Entry `index`'s node in `dir` and its stat, or `None` past the directory's end. The
    /// window holds it ([`Window::holds`]).
    fn entry(&self, dir: &Node, index: u64) -> Result<Option<(Node, FileStat)>, NineError> {
        let i = usize::try_from(index - self.start).unwrap_or(usize::MAX);
        let Some(e) = self.entries.get(i) else { return Ok(None) };
        let begin = i.checked_sub(1).map_or(0, |b| self.entries[b].end);
        let node = Node::at(join(&dir.path, &self.names[begin..e.end])?, &e.meta);
        let stat = stat(&node, &e.meta)?;
        Ok(Some((node, stat)))
    }
}

/// What `stat` says of `node`, whose metadata is `m`. A directory's length is 0, as 9P has it.
fn stat(node: &Node, m: &Metadata) -> Result<FileStat, NineError> {
    let mut name = String::new();
    name.try_reserve(node.name().len()).map_err(|_| NineError::NO_MEMORY)?;
    name.push_str(node.name());
    let (mode, length) = if node.dir { (DMDIR | 0o755, 0) } else { (0o644, m.size) };
    let mtime = u32::try_from(m.mtime / 1_000_000).unwrap_or(u32::MAX);
    Ok(FileStat { qid: node.qid(), mode, mtime, length, name })
}

/// One volume's files.
pub struct Walfsd<R: Range> {
    /// `None`: the volume did not mount, or its device failed since; every request is corrupt.
    fs: Option<Filesystem<Blocks<R>>>,
    /// The range refuses writes: every change is refused here, before walfs is asked.
    read_only: bool,
    labels: Vec<u64>,
    /// The last `get_attr`'s value, which its reply borrows.
    attr: Vec<u8>,
    /// What each entry holds beside its blocks: the volume's room over its inodes
    /// (servers/walfsd.md, "Quotas").
    share: u64,
    /// What each live root holds.
    pub(crate) ledger: Ledger,
    /// The change generation: moved by every change to the volume, so a window filled before
    /// it is never served after it.
    changes: u64,
    /// The one listing window, whoever lists.
    window: Window,
    /// How many passes over a directory listings made.
    #[cfg(test)]
    pub(crate) passes: u32,
}

impl<R: Range> Walfsd<R> {
    /// The server for what [`crate::volume::mount`] found, under `labels`. The volume root's
    /// quota is the data region's bytes, and it holds what is on the volume, counted now; a
    /// volume that does not count is served as corrupt.
    pub fn new(mounted: Mounted<R>, labels: Vec<u64>) -> Walfsd<R> {
        let (fs, read_only, room, share, held) = match mounted {
            Mounted::Files { mut fs, read_only } => {
                let room = u64::from(fs.data_blocks()) * u64::from(BLOCK);
                // Inode 0 and the root are never an entry's.
                let share = room.div_ceil(u64::from(fs.inode_count().saturating_sub(2).max(1)));
                match tally(&mut fs, "", share, &[]) {
                    Ok((held, _)) => (Some(fs), read_only, room, share, held),
                    Err(_) => (None, read_only, 0, 0, 0),
                }
            }
            Mounted::Corrupt(_) => (None, false, 0, 0, 0),
        };
        Walfsd {
            fs,
            read_only,
            labels,
            attr: Vec::new(),
            share,
            ledger: Ledger::new(room, held, walfs::ROOT as u64),
            changes: 0,
            window: Window::default(),
            #[cfg(test)]
            passes: 0,
        }
    }

    /// Whether the volume is served as corrupt: every attach refused with `corrupt`.
    pub fn is_corrupt(&self) -> bool { self.fs.is_none() }

    /// The volume check's line, for a start under the power-loss feature (src/cut.rs).
    #[cfg(feature = "cut-after-write")]
    pub fn checked(&mut self) -> Option<String> { self.fs.as_mut().map(crate::cut::said) }

    /// What each entry holds beside its blocks.
    pub fn share(&self) -> u64 { self.share }

    /// Refuses a change to a read-only volume, so a refused write never reaches the device.
    pub(crate) fn writable(&self) -> Result<(), Failure> {
        if self.read_only { Err(Failure::ReadOnly) } else { Ok(()) }
    }

    pub(crate) fn volume_labels(&self) -> &[u64] { &self.labels }

    /// Keeps `value` for a `get_attr` reply to borrow.
    pub(crate) fn keep_attr(&mut self, value: Vec<u8>) -> &[u8] {
        self.attr = value;
        &self.attr
    }

    /// Runs `op` on the filesystem. A damaged block is `Corrupt` for this request alone
    /// (servers/walfsd.md, "What is corrupt"); an I/O error poisons the volume until it is
    /// mounted again (servers/walfsd.md, "Failure and restart"), and from then on every request
    /// is corrupt.
    pub(crate) fn with<T>(
        &mut self,
        op: impl FnOnce(&mut Filesystem<Blocks<R>>) -> Result<T, FsError>,
    ) -> Result<T, Failure> {
        let fs = self.fs.as_mut().ok_or(Failure::Fs(FsError::Corrupt))?;
        let r = op(fs);
        if r.as_ref().is_err_and(|e| matches!(e, FsError::Io | FsError::Poisoned)) {
            self.fs = None;
        }
        r.map_err(Failure::Fs)
    }

    /// Finds `node`'s file again: it must still be at its path with its inode and generation.
    /// Anything else is `removed`, so a fid never reaches a file that took the place of the one
    /// it was on. Returns what `stat` says of it now.
    pub(crate) fn find(&mut self, node: &Node) -> Result<Metadata, Failure> {
        match self.with(|fs| fs.stat(&node.path)) {
            Ok(m) if m.inode == node.ino && m.generation == node.generation => Ok(m),
            Ok(_) | Err(Failure::Fs(FsError::NoEntry | FsError::NotDir)) => Err(Failure::Removed),
            Err(e) => Err(e),
        }
    }

    /// The node for the entry at `path`.
    pub(crate) fn node_at(&mut self, path: String) -> Result<Node, Failure> {
        let m = self.with(|fs| fs.stat(&path))?;
        Ok(Node::at(path, &m))
    }

    /// The path of `dir`'s entry `name`, once `dir` is found again.
    pub(crate) fn found_child(&mut self, dir: &Node, name: &str) -> Result<String, Failure> {
        self.find(dir)?;
        join(&dir.path, name).map_err(|_| Failure::NoMemory)
    }

    /// The root holding the directory `dir`, if it can grow by `need`; otherwise `no space`
    /// (servers/walfsd.md, "Quotas").
    pub(crate) fn room(&self, dir: &str, need: u64) -> Result<usize, Failure> {
        let i = self.ledger.holder(dir);
        if self.ledger.fits(i, need) { Ok(i) } else { Err(Failure::Fs(FsError::NoSpace)) }
    }

    /// The bytes the directory at `dir` holds in blocks.
    pub(crate) fn dir_size(&mut self, dir: &str) -> Result<u64, Failure> {
        self.with(|fs| fs.stat(dir)).map(|m| m.size)
    }

    /// Moves the change generation on: every change to the volume calls this first, so no
    /// listing window filled before it is served after it.
    pub(crate) fn changed(&mut self) { self.changes = self.changes.wrapping_add(1) }

    /// Runs `change`, which may add an entry to the directory `dir`, charging root `i` for the
    /// blocks the directory gains, whether or not the change succeeded. A directory never shrinks
    /// (servers/walfsd.md, "Directories"), so a change that adds no entry needs only
    /// [`Self::changed`].
    pub(crate) fn changing<T>(
        &mut self,
        i: usize,
        dir: &str,
        change: impl FnOnce(&mut Self) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        self.changed();
        let before = self.dir_size(dir)?;
        let changed = change(self);
        // A volume that failed meanwhile is corrupt from now on; nothing more to count.
        if let Ok(after) = self.dir_size(dir) {
            self.ledger.change(i, after, before);
        }
        changed
    }

    /// What the entry at `path` holds: its share, and a file's blocks or all that lies under a
    /// directory, its own blocks included. No live root is at or under it.
    pub(crate) fn holds(&mut self, path: &str) -> Result<u64, Failure> {
        let m = self.with(|fs| fs.stat(path))?;
        let under = match m.kind {
            FileType::File => file_bytes(m.size),
            FileType::Dir => {
                let share = self.share;
                self.with(|fs| tally(fs, path, share, &[]))?.0
            }
        };
        Ok(self.share.saturating_add(under))
    }

    /// Counts every live root afresh and checks the ledger agrees: what it holds, its reserve,
    /// and that it is within its quota unless `over` allows it.
    #[cfg(test)]
    pub(crate) fn audit(&mut self, over: bool) {
        let live = self.ledger.charges().unwrap();
        let share = self.share;
        for (_, path, quota, held, reserve) in self.ledger.roots() {
            let counted = self.with(|fs| tally(fs, &path, share, &live)).unwrap();
            assert_eq!((held, reserve), counted, "the root at {path:?}");
            assert!(over || held + reserve <= quota, "the root at {path:?}: {held} + {reserve} > {quota}");
        }
    }

    /// Opens `node`'s file, runs `op` on the handle, and closes it whatever `op` did.
    pub(crate) fn on_file<T>(
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

    /// Creates the file `path` in `dir`, which must not hold it: a create that finds the name
    /// taken is `exists`, never an open of what is there.
    pub(crate) fn create_file(&mut self, path: &str) -> Result<(), Failure> {
        match self.with(|fs| fs.stat(path)) {
            Ok(_) => return Err(Failure::Fs(FsError::Exists)),
            Err(Failure::Fs(FsError::NoEntry)) => {}
            Err(e) => return Err(e),
        }
        let new = OpenOptions { write: true, create: true, ..OpenOptions::default() };
        self.with(|fs| {
            let h = fs.open(path, new)?;
            fs.close(h)
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
        // The node is kept by a fallible copy of its path.
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
                    kept = w.names.try_reserve(name.len()).map_err(|_| Failure::NoMemory);
                    if kept.is_ok() {
                        w.names.push_str(name);
                        w.entries.push(Listed { end: w.names.len(), meta: entry.meta });
                    }
                }
                seen += 1;
            })
        })?;
        kept?;
        (w.dir, w.changes, w.start) = (Some(Node { path, ..*dir }), self.changes, index);
        self.window = w;
        Ok(())
    }
}

/// Why a request on the files failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// What walfs said.
    Fs(FsError),
    /// The file a node names is gone: removed, or another in its inode.
    Removed,
    /// No memory for a name.
    NoMemory,
    /// A change to a read-only volume.
    ReadOnly,
}

pub(crate) fn parent(path: &str) -> &str { path.rsplit_once('/').map_or("", |(dir, _)| dir) }

/// The bytes a file of `size` holds in blocks (servers/walfsd.md, "Quotas"): every data block its
/// size reaches, holes or not, and the indirect blocks that would map them.
pub(crate) fn file_bytes(size: u64) -> u64 {
    const DIRECT: u64 = 12;
    const PER: u64 = 1024;
    let data = size.div_ceil(u64::from(BLOCK));
    let single = u64::from(data > DIRECT);
    let double = match data.saturating_sub(DIRECT + PER) {
        0 => 0,
        past => 1 + past.div_ceil(PER),
    };
    (data + single + double).saturating_mul(u64::from(BLOCK))
}

/// What the directory `top` holds (servers/walfsd.md, "Quotas"): its own blocks, and for every
/// entry under it its share and its blocks, a file's by its size and a directory's own; for a
/// live root below it (`live`: inode and charge), its entry's share, and that root's charge in
/// reserve instead of what lies under it. Returns (held, reserve). The walk reads at most as many
/// directories with entries as the volume has data blocks, so a directory named inside itself on
/// a hostile medium is corrupt, not a loop.
fn tally<R: Range>(
    fs: &mut Filesystem<Blocks<R>>,
    top: &str,
    share: u64,
    live: &[(u64, u64)],
) -> Result<(u64, u64), FsError> {
    let m = fs.stat(top)?;
    if m.kind != FileType::Dir {
        return Err(FsError::NotDir);
    }
    let mut dirs: Vec<String> = Vec::new();
    let mut owned = String::new();
    owned.try_reserve(top.len()).map_err(|_| FsError::NoSpace)?;
    owned.push_str(top);
    dirs.try_reserve(1).map_err(|_| FsError::NoSpace)?;
    dirs.push(owned);
    let (mut held, mut reserve, mut left) = (m.size, 0u64, fs.data_blocks());
    while let Some(dir) = dirs.pop() {
        let mut failed = None;
        let mut below = Vec::new();
        let read = fs.read_dir(&dir, |e| {
            held = held.saturating_add(share);
            if e.meta.kind == FileType::File {
                held = held.saturating_add(file_bytes(e.meta.size));
                return;
            }
            if let Some((_, charge)) = live.iter().find(|(root, _)| *root == u64::from(e.meta.inode)) {
                reserve = reserve.saturating_add(*charge);
                return;
            }
            held = held.saturating_add(e.meta.size);
            // A directory no client could name was not written by `walfsd` or its packer.
            let child = core::str::from_utf8(e.name).ok().filter(|n| path::valid_name(n));
            match child.map(|name| join(&dir, name)) {
                Some(Ok(path)) if below.try_reserve(1).is_ok() => below.push(path),
                Some(_) => failed = Some(FsError::NoSpace),
                None => failed = Some(FsError::Corrupt),
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        if read > 0 {
            left = left.checked_sub(1).ok_or(FsError::Corrupt)?;
        }
        dirs.try_reserve(below.len()).map_err(|_| FsError::NoSpace)?;
        dirs.append(&mut below);
    }
    Ok((held, reserve))
}

/// A failure as a 9P error. walfs's `Corrupt`, an I/O error and the poisoning it leaves are all
/// `corrupt`: the client learns the volume is broken, not a refusal.
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

/// The same failure as the typed operations' error (libs/wire/tables/littlefsd.md), by the name
/// [`nine`] gives it (servers/wire.md, "Error names").
pub(crate) fn code(e: Failure) -> ErrorCode {
    match e {
        Failure::Removed => ErrorCode::Removed,
        Failure::NoMemory => ErrorCode::NoMemory,
        Failure::ReadOnly => ErrorCode::ReadOnly,
        Failure::Fs(FsError::Io | FsError::Corrupt | FsError::Poisoned) => ErrorCode::Corrupt,
        Failure::Fs(FsError::NoEntry | FsError::NoAttr) => ErrorCode::NotFound,
        Failure::Fs(FsError::Exists) => ErrorCode::Exists,
        Failure::Fs(FsError::NotDir) => ErrorCode::NotDir,
        Failure::Fs(FsError::IsDir) => ErrorCode::IsDir,
        Failure::Fs(FsError::NotEmpty) => ErrorCode::NotEmpty,
        Failure::Fs(FsError::NoSpace) => ErrorCode::NoSpace,
        Failure::Fs(FsError::FileTooBig) => ErrorCode::TooLarge,
        Failure::Fs(FsError::NameTooLong) => ErrorCode::BadName,
        Failure::Fs(FsError::Invalid) => ErrorCode::NotPermitted,
    }
}

impl<R: Range> FileServer for Walfsd<R> {
    type Node = Node;

    /// Every badge of the founding kind attaches at the volume's root; `aname` is ignored. A
    /// volume that did not mount answers `corrupt`.
    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Node, Qid), NineError> {
        let root = self.node_at(String::new()).map_err(nine)?;
        let qid = root.qid();
        Ok((root, qid))
    }

    /// Records the connection's quota, carved from the room of the live root above `root`
    /// (servers/walfsd.md, "Quotas"), once `root` is found again: a stale node at a directory
    /// removed and made again is `removed`, never a second root at that path. A directory going
    /// live is counted here, once.
    fn minted(
        &mut self,
        caller: &Caller,
        badge: u64,
        _: u64,
        root: &Node,
        quota: u64,
    ) -> Result<(), NineError> {
        self.find(root).map_err(nine)?;
        let live = self.ledger.charges().ok_or(text::QUOTA_REFUSED)?;
        let share = self.share;
        let fs = self.fs.as_mut().ok_or(text::CORRUPT)?;
        let count = || tally(fs, &root.path, share, &live);
        match self.ledger.mint(caller.badge, badge, u64::from(root.ino), &root.path, quota, count) {
            Ok(()) => Ok(()),
            Err(Refusal::Refused) => Err(text::QUOTA_REFUSED),
            Err(Refusal::Count(e)) => {
                if matches!(e, FsError::Io | FsError::Poisoned) {
                    self.fs = None;
                }
                Err(nine(Failure::Fs(e)))
            }
        }
    }

    /// Gives the connection's quota back to the root it was carved from.
    fn disconnected(&mut self, badge: u64) { self.ledger.disconnect(badge) }

    /// The volume's labels, for every node (servers/walfsd.md: labels are per volume).
    fn labels(&self, _: &Node) -> &[u64] { &self.labels }

    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        #[cfg(feature = "cut-after-write")]
        if crate::cut::arm(name) {
            return Err(NineError::NOT_FOUND);
        }
        self.find(dir).map_err(nine)?;
        let node = self.node_at(join(&dir.path, name)?).map_err(nine)?;
        let qid = node.qid();
        Ok((node, qid))
    }

    fn open(&mut self, _: &Caller, node: &Node, m: u8) -> Result<Qid, NineError> {
        if matches!(m & 3, mode::OWRITE | mode::ORDWR) || m & mode::OTRUNC != 0 {
            self.writable().map_err(nine)?;
        }
        let meta = self.find(node).map_err(nine)?;
        if m & mode::OTRUNC != 0 && !node.dir {
            // A truncation only gives back, and needs no room.
            let dir = parent(&node.path);
            let i = self.ledger.holder(dir);
            self.changed();
            let truncate = OpenOptions { write: true, truncate: true, ..OpenOptions::default() };
            self.on_file(node, truncate, |_, _| Ok(())).map_err(nine)?;
            self.ledger.change(i, 0, file_bytes(meta.size));
        }
        Ok(node.qid())
    }

    fn read(&mut self, _: &Caller, node: &Node, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let read = OpenOptions { read: true, ..OpenOptions::default() };
        let n = self.on_file(node, read, |fs, h| {
            if offset >= fs.file_size(h)? {
                return Ok(0);
            }
            fs.seek(h, offset)?;
            fs.read(h, out)
        });
        n.map(Read::Done).map_err(nine)
    }

    /// One transaction while the write fits one, several if not (servers/walfsd.md,
    /// "Atomicity"); refused before any begins if it would take the root past its quota.
    fn write(&mut self, _: &Caller, node: &Node, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        self.writable().map_err(nine)?;
        let old = self.find(node).map_err(nine)?.size;
        let end = u64::try_from(data.len()).ok().and_then(|n| offset.checked_add(n));
        let new = end.filter(|e| *e <= walfs::MAX_FILE_SIZE).ok_or(text::TOO_BIG)?.max(old);
        let dir = parent(&node.path);
        let i = self.room(dir, file_bytes(new) - file_bytes(old)).map_err(nine)?;
        self.changed();
        let write = OpenOptions { write: true, ..OpenOptions::default() };
        let written = self.on_file(node, write, |fs, h| {
            fs.seek(h, offset)?;
            fs.write(h, data)
        });
        // A write that filled the volume part way, or failed after some transactions, left the
        // file as long as it now is.
        if let Ok(now) = self.with(|fs| fs.stat(&node.path)) {
            self.ledger.change(i, file_bytes(now.size), file_bytes(old));
        }
        written.map_err(nine)
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> {
        let m = self.find(node).map_err(nine)?;
        stat(node, &m)
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

    /// A new entry holds its share from the start, and its directory may gain a block.
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
        let i = self.room(&dir.path, self.share + u64::from(BLOCK)).map_err(nine)?;
        self.changing(i, &dir.path, |walfsd| {
            if perm & DMDIR != 0 {
                walfsd.with(|fs| fs.mkdir(&path))?;
            } else {
                walfsd.create_file(&path)?;
            }
            walfsd.ledger.change(i, walfsd.share, 0);
            Ok(())
        })
        .map_err(nine)?;
        let node = self.node_at(path).map_err(nine)?;
        let qid = node.qid();
        Ok((node, qid))
    }

    /// Removes the file or empty directory. Every other fid on it finds it gone (its inode is
    /// free, or another file's at a later generation) and gets `removed`; no walfs handle outlives
    /// a request, so its blocks are freed in the same call. A live root's directory is not
    /// removed: that would end its connections.
    fn remove(&mut self, _: &Caller, node: &Node) -> Result<(), NineError> {
        if node.path.is_empty() || self.ledger.holds_live(&node.path) {
            return Err(NineError::PERMISSION);
        }
        self.writable().map_err(nine)?;
        self.find(node).map_err(nine)?;
        let held = self.holds(&node.path).map_err(nine)?;
        let dir = parent(&node.path);
        let i = self.ledger.holder(dir);
        self.changed();
        self.with(|fs| fs.remove(&node.path)).map_err(nine)?;
        self.ledger.change(i, 0, held);
        Ok(())
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "quota_tests.rs"]
mod quota_tests;
