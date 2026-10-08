//! The files behind the 9P skeleton: one EROFS volume, every node carrying the volume's labels.

use alloc::string::String;
use alloc::vec::Vec;

use erofs::{BLOCK, Corrupt, Dirents, EXTENDED, Inode, Kind, SUPERBLOCK_AT, SUPERBLOCK_LEN, Superblock};
use redoubt_rt::abi::PAGE_SIZE;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{
    DMDIR, FileServer, FileStat, NineError, QTDIR, Qid, REQUEST_STATE, Read, mode,
};
use redoubt_rt::server::{Cost, Limits};

/// What admission lets each of `buckets` buckets hold (servers/serving.md R26); the count is the
/// manifest's `buckets=N`. A call is answered as it arrives; what is held is a multiplexed
/// connection's completion call (`InFlight`, one per session, so two lets a share hold one), its
/// requests (`Requests`), and the pages their transfers brought (`Pages`): nothing is written
/// here, so two pages, the most one batch of requests needs.
pub const fn limits(buckets: u32) -> Limits {
    Limits { buckets, in_flight: 2, files: 32, state: 8, requests: 64, pages: 2 }
}
/// What one of each costs, in bytes: a completion call holds its caller's lend, charged to this
/// server until it replies (kernel/ipc.md R3), `MAX_LEND_PAGES` pages at worst; a fid is its table
/// entry and a node per step from its root, each an inode and a name; a minted connection its
/// record and its root's node; a request its record; a page a page.
pub const COST: Cost =
    Cost { in_flight: 64 * 1024, file: 2048, state: 512, request: REQUEST_STATE, page: PAGE_SIZE as u64 };
/// The bytes of this server's budget its clients may use between them. A bucket at its caps costs
/// 2 completion calls at 64 KiB, 32 fids at 2 KiB, 8 connections at 512 bytes, 64 requests at 256
/// and 2 pages at 4 KiB: 225 280 bytes, so 6 fit here (1 351 680). Every session reads the system
/// volume on a connection the steward makes for it, so each principal's domain is a bucket beside
/// the steward's: the image's three and the steward take 4, a case's extra client 5, and one more
/// principal's domain fits.
pub const BUDGET: u64 = 1536 * 1024;

/// The `Rerror` texts `erofsd` adds to the skeleton's fixed set, `littlefsd`'s words for the same
/// things.
pub mod text {
    use redoubt_rt::server::ninep::NineError;

    /// The volume is not in the subset or not consistent, or its range failed.
    pub const CORRUPT: NineError = NineError("corrupt");
    pub const READ_ONLY: NineError = NineError("read-only volume");
}

/// A request to the range failed: the range refused it, or its disk did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// The range as `erofsd` uses it: its size, and bytes read from anywhere in it
/// (libs/wire/tables/blkd.md's `info` and `read`, in [`crate::blkd`]).
pub trait Range {
    /// The range's length in sectors (`info`).
    fn sectors(&mut self) -> Result<u64, Fault>;
    /// `out.len()` bytes from byte `at`.
    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), Fault>;
}

/// What a fid rests on: the inode it was walked to, read and checked then, and the name it was
/// walked by (`/` for the root). Nothing on the volume changes, so the inode stays true.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    inode: Inode,
    name: String,
}

impl Node {
    pub fn inode(&self) -> &Inode { &self.inode }

    pub fn name(&self) -> &str { &self.name }
}

/// Why a request could not be answered from the volume.
enum Failure {
    /// What the volume holds is not in the subset.
    Corrupt,
    /// The range failed: the volume is corrupt from now on.
    Io,
}

impl From<Corrupt> for Failure {
    fn from(_: Corrupt) -> Failure { Failure::Corrupt }
}

impl From<Fault> for Failure {
    fn from(_: Fault) -> Failure { Failure::Io }
}

/// A volume that parsed: its range, its superblock and its root.
struct Volume<R> {
    range: R,
    sb: Superblock,
    root: Inode,
}

/// Inode `nid` of `sb`'s volume, read and checked: at most an extended inode's bytes, one range
/// read.
fn read_inode<R: Range>(range: &mut R, sb: &Superblock, nid: u64) -> Result<Inode, Failure> {
    let at = sb.inode_at(nid)?;
    let mut bytes = [0u8; EXTENDED];
    let len = (sb.bytes() - at).min(EXTENDED as u64) as usize;
    #[cfg(feature = "boot-stats")]
    crate::stats::read_for(crate::stats::For::Inode);
    range.read(at, &mut bytes[..len])?;
    Ok(Inode::parse(sb, nid, &bytes[..len])?)
}

/// The directory block the scratch block holds: its directory's inode number, its index, its
/// length.
#[derive(Clone, Copy)]
struct Held {
    nid: u64,
    index: u64,
    len: usize,
}

/// Where the last directory read stopped: the directory, the block, and how many of its entries
/// (`.` and `..` left out) the blocks before that one hold. Reading on from there starts there.
#[derive(Clone, Copy)]
struct Cursor {
    nid: u64,
    index: u64,
    first: u64,
}

/// The server: the volume, or `None` when it is corrupt; its labels; one block of scratch.
pub struct Erofsd<R: Range> {
    volume: Option<Volume<R>>,
    labels: Vec<u64>,
    scratch: Vec<u8>,
    held: Option<Held>,
    cursor: Option<Cursor>,
    #[cfg(feature = "boot-stats")]
    say: Option<Say>,
}

/// Says a line on `erofsd`'s console (`boot-stats`).
#[cfg(feature = "boot-stats")]
pub type Say = alloc::boxed::Box<dyn Fn(&str)>;

impl<R: Range> Erofsd<R> {
    /// The server for `range`, with the volume's `labels`: the superblock and the root inode are
    /// read and checked now. A range that cannot be sized, or no memory for the scratch block, is
    /// no volume at all (`Err`: the program exits); anything else that fails is served as corrupt.
    pub fn new(mut range: R, labels: Vec<u64>) -> Result<Erofsd<R>, Fault> {
        let sectors = range.sectors()?;
        let mut scratch = Vec::new();
        scratch.try_reserve_exact(BLOCK).map_err(|_| Fault)?;
        scratch.resize(BLOCK, 0);
        let volume = open(range, sectors, &mut scratch);
        Ok(Erofsd {
            volume,
            labels,
            scratch,
            held: None,
            cursor: None,
            #[cfg(feature = "boot-stats")]
            say: None,
        })
    }

    /// Says the boot's counts through `say` (src/stats.rs).
    #[cfg(feature = "boot-stats")]
    pub fn say_stats(&mut self, say: Say) { self.say = Some(say) }

    /// Counts `op`, and says the counts at each power of two of requests.
    #[cfg(feature = "boot-stats")]
    fn counted(&self, op: crate::stats::Op) {
        if crate::stats::op(op) {
            self.said();
        }
    }

    #[cfg(feature = "boot-stats")]
    fn said(&self) {
        if let Some(say) = &self.say {
            say(&crate::stats::line());
        }
    }

    /// Whether the volume is served as corrupt: every attach refused with `corrupt`.
    pub fn is_corrupt(&self) -> bool { self.volume.is_none() }

    fn volume(&mut self) -> Result<&mut Volume<R>, NineError> { self.volume.as_mut().ok_or(text::CORRUPT) }

    /// What `failure` answers. A range that failed makes the volume corrupt until `erofsd`
    /// starts again (servers/littlefsd.md R49): what it would read next cannot be trusted.
    fn failed(&mut self, failure: Failure) -> NineError {
        if let Failure::Io = failure {
            self.volume = None;
            self.held = None;
        }
        text::CORRUPT
    }

    fn inode(&mut self, nid: u64) -> Result<Inode, Failure> {
        let volume = self.volume.as_mut().ok_or(Failure::Corrupt)?;
        read_inode(&mut volume.range, &volume.sb, nid)
    }

    /// Directory `dir`'s block `index`, into the scratch block unless it is there already: its
    /// length.
    fn dir_block(&mut self, dir: &Inode, index: u64) -> Result<usize, Failure> {
        match self.held {
            Some(h) if h.nid == dir.nid() && h.index == index => return Ok(h.len),
            _ => self.held = None,
        }
        let (at, len) = dir.dir_block(index).ok_or(Failure::Corrupt)?;
        let volume = self.volume.as_mut().ok_or(Failure::Corrupt)?;
        #[cfg(feature = "boot-stats")]
        crate::stats::read_for(crate::stats::For::Directory);
        volume.range.read(at, &mut self.scratch[..len])?;
        self.held = Some(Held { nid: dir.nid(), index, len });
        Ok(len)
    }

    /// The inode number `name` has in `dir`: a binary search over its blocks by their first and
    /// last names, then over the one block that could hold it.
    fn lookup(&mut self, dir: &Inode, name: &[u8]) -> Result<Option<u64>, Failure> {
        let (mut low, mut high) = (0, dir.dir_blocks());
        while low < high {
            let mid = low + (high - low) / 2;
            let len = self.dir_block(dir, mid)?;
            let entries = Dirents::parse(&self.scratch[..len])?;
            let first = entries.get(0).ok_or(Failure::Corrupt)?;
            let last = entries.get(entries.len() - 1).ok_or(Failure::Corrupt)?;
            if name < first.name {
                high = mid;
            } else if name > last.name {
                low = mid + 1;
            } else {
                return Ok(entries.lookup(name).map(|e| e.nid));
            }
        }
        Ok(None)
    }

    /// The `index`th entry of `dir`, `.` and `..` left out: its inode number and name. Reads on
    /// from where the last directory read stopped, so a listing reads each block once.
    fn entry(&mut self, dir: &Inode, index: u64) -> Result<Option<(u64, String)>, Failure> {
        let (mut block, mut first) = match self.cursor {
            Some(c) if c.nid == dir.nid() && c.first <= index => (c.index, c.first),
            _ => (0, 0),
        };
        while block < dir.dir_blocks() {
            let len = self.dir_block(dir, block)?;
            self.cursor = Some(Cursor { nid: dir.nid(), index: block, first });
            let entries = Dirents::parse(&self.scratch[..len])?;
            let children = || entries.iter().filter(|e| e.name != b"." && e.name != b"..");
            let count = children().count() as u64;
            if index < first + count {
                let entry = children().nth((index - first) as usize).ok_or(Failure::Corrupt)?;
                let name = core::str::from_utf8(entry.name).map_err(|_| Failure::Corrupt)?;
                return Ok(Some((entry.nid, owned(name).ok_or(Failure::Corrupt)?)));
            }
            first += count;
            block += 1;
        }
        Ok(None)
    }
}

/// The volume on `range` of `sectors` sectors, its superblock and root read through `scratch`;
/// `None` if any of it fails.
fn open<R: Range>(mut range: R, sectors: u64, scratch: &mut [u8]) -> Option<Volume<R>> {
    let head = &mut scratch[..SUPERBLOCK_AT + SUPERBLOCK_LEN];
    range.read(0, head).ok()?;
    let sb = Superblock::parse(head, sectors / (BLOCK as u64 / 512)).ok()?;
    let root = read_inode(&mut range, &sb, sb.root).ok().filter(|r| r.kind() == Kind::Dir)?;
    Some(Volume { range, sb, root })
}

fn owned(s: &str) -> Option<String> {
    let mut out = String::new();
    out.try_reserve(s.len()).ok()?;
    out.push_str(s);
    Some(out)
}

fn qid(inode: &Inode) -> Qid {
    // Nothing changes on a read-only volume: every version is 0, and the path is the inode number.
    let kind = if inode.kind() == Kind::Dir { QTDIR } else { 0 };
    Qid { kind, version: 0, path: inode.nid() }
}

fn stat_of(inode: &Inode, name: &str) -> Result<FileStat, NineError> {
    let (mode, length) = match inode.kind() {
        Kind::Dir => (DMDIR | 0o555, 0),
        Kind::File => (0o444, inode.size()),
    };
    Ok(FileStat { qid: qid(inode), mode, mtime: 0, length, name: owned(name).ok_or(NineError::NO_MEMORY)? })
}

impl<R: Range> FileServer for Erofsd<R> {
    type Node = Node;

    /// Every badge of the founding kind attaches at the volume's root; `aname` is ignored. A
    /// corrupt volume answers `corrupt`.
    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Node, Qid), NineError> {
        let root = self.volume()?.root;
        Ok((Node { inode: root, name: owned("/").ok_or(NineError::NO_MEMORY)? }, qid(&root)))
    }

    /// The volume's labels, for every node.
    fn labels(&self, _: &Node) -> &[u64] { &self.labels }

    /// One lookup in `dir`, and the inode it names read once, kept on the fid.
    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        #[cfg(feature = "boot-stats")]
        {
            self.counted(crate::stats::Op::Walk);
            if name == crate::stats::SENTINEL {
                self.said();
            }
        }
        self.volume()?;
        if dir.inode.kind() != Kind::Dir {
            return Err(NineError::NOT_DIR);
        }
        let found = match self.lookup(&dir.inode, name.as_bytes()) {
            Ok(found) => found,
            Err(f) => return Err(self.failed(f)),
        };
        let inode = match found.map(|nid| self.inode(nid)) {
            None => return Err(NineError::NOT_FOUND),
            Some(Ok(inode)) => inode,
            Some(Err(f)) => return Err(self.failed(f)),
        };
        Ok((Node { inode, name: owned(name).ok_or(NineError::NO_MEMORY)? }, qid(&inode)))
    }

    /// Reading only: any mode that could change a file is refused before anything else.
    fn open(&mut self, _: &Caller, node: &Node, m: u8) -> Result<Qid, NineError> {
        #[cfg(feature = "boot-stats")]
        self.counted(crate::stats::Op::Open);
        self.volume()?;
        if matches!(m & 3, mode::OWRITE | mode::ORDWR) || m & mode::OTRUNC != 0 {
            return Err(text::READ_ONLY);
        }
        Ok(qid(&node.inode))
    }

    /// The file's bytes from `offset`, cut to its size: one range read of its blocks, and one of
    /// its inline tail where the read reaches it.
    fn read(&mut self, _: &Caller, node: &Node, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        #[cfg(feature = "boot-stats")]
        self.counted(crate::stats::Op::Read);
        self.volume()?;
        if node.inode.kind() != Kind::File {
            return Err(NineError::NOT_SUPPORTED);
        }
        let want = node.inode.size().saturating_sub(offset).min(out.len() as u64) as usize;
        let mut done = 0;
        while done < want {
            let (at, run) = node.inode.extent(offset + done as u64).ok_or(text::CORRUPT)?;
            let n = run.min((want - done) as u64) as usize;
            #[cfg(feature = "boot-stats")]
            crate::stats::read_for(crate::stats::For::Data);
            if let Err(f) = self.volume()?.range.read(at, &mut out[done..done + n]) {
                return Err(self.failed(f.into()));
            }
            done += n;
        }
        #[cfg(feature = "boot-stats")]
        crate::stats::read_bytes(want);
        Ok(Read::Done(want))
    }

    fn write(&mut self, _: &Caller, _: &Node, _: u64, _: &[u8]) -> Result<usize, NineError> {
        Err(text::READ_ONLY)
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> {
        #[cfg(feature = "boot-stats")]
        self.counted(crate::stats::Op::Stat);
        self.volume()?;
        stat_of(&node.inode, &node.name)
    }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &Node,
        index: u64,
    ) -> Result<Option<(Node, FileStat)>, NineError> {
        self.volume()?;
        if dir.inode.kind() != Kind::Dir {
            return Ok(None);
        }
        let entry = match self.entry(&dir.inode, index) {
            Ok(entry) => entry,
            Err(f) => return Err(self.failed(f)),
        };
        let Some((nid, name)) = entry else { return Ok(None) };
        let inode = match self.inode(nid) {
            Ok(inode) => inode,
            Err(f) => return Err(self.failed(f)),
        };
        let stat = stat_of(&inode, &name)?;
        Ok(Some((Node { inode, name }, stat)))
    }

    fn create(&mut self, _: &Caller, _: &Node, _: &str, _: u32, _: u8) -> Result<(Node, Qid), NineError> {
        Err(text::READ_ONLY)
    }

    fn remove(&mut self, _: &Caller, _: &Node) -> Result<(), NineError> { Err(text::READ_ONLY) }

    #[cfg(feature = "boot-stats")]
    fn clunk(&mut self, _: &Node) { self.counted(crate::stats::Op::Clunk) }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
