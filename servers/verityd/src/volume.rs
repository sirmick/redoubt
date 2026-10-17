//! The volume as `verityd` checks it (docs/servers/verityd.md, "Reading"): the start check, and
//! each data block checked through the tree before any of it leaves.
//!
//! **A signed volume** ("The root block, and the two modes") gives its own N and root in its root
//! block, the range's last whole block: the block is parsed, its signature verified under the
//! manifest's key with the loader's Ed25519 (`ed25519-compact`), its version held to the floor,
//! and only then are N and the root taken from it, as a pinned volume's come from the manifest.
//!
//! **Memory is fixed at start, whatever the volume's size:** the top tree block, pinned;
//! [`TREE_CACHE`] tree blocks, each kept only once it was checked against the block above it; a
//! cache of checked data blocks sized from the heap the manifest gives the instance ([`Caps`]),
//! least recently used out first, so a block read again (a directory block, the block holding a
//! file's inode) is hashed and fetched once while it is held; and one `blkd` read's worth of
//! read-ahead, a client's run of cold blocks fetched in one read, up to five `blkd` calls at the
//! two-page lend ([`Volume::load`]).
//!
//! **A block is checked from the top down.** The lowest block on its path that is already held
//! (the top always is) gives the digest the next block down must hash to; each block fetched is
//! checked against it before it is kept or used, and the data block against its slot in level 1.
//! So nothing unchecked is ever trusted, and a hit on level 1 costs one hash.

use alloc::vec::Vec;
use core::fmt;

use ed25519_compact::{PublicKey, Signature};
use redoubt_verity::{
    BLOCK, DIGEST, Geometry, Hash, RootBlock, SECTORS_PER_BLOCK, leaf, node, root, root_block_at, slot,
};

use crate::server::MAX_SECTORS;
use crate::{Fault, Mode, Range, SECTOR, Size};

/// The fewest tree blocks kept beside the pinned top, when no budget is given (host tests): 128 KiB.
pub const TREE_CACHE: usize = 32;
/// The fewest checked data blocks kept, when no budget is given (host tests): 16 KiB. One reader's
/// EROFS boot profile held 100, 197 and 289 of 849 blocks asked in 1, 4 and 8 (servers/verityd.md,
/// "A cache of checked data blocks"); under two readers the data cache is sized from the manifest
/// instead ([`Caps::from_heap`]), since two readers' working sets thrash a four-block cache.
pub const DATA_CACHE: usize = 4;

/// The data blocks one `blkd` read carries, `MAX_SECTORS` (64) sectors' worth: the most blocks one
/// read-ahead reads at once (a client's run of consecutive cold blocks, which `erofsd` asks in one
/// call of up to that many sectors), so the round trips a two-reader load pays fall with the cold
/// run, not one read a block (servers/verityd.md, "A cache of checked data blocks"). Derived from
/// the `blkd` limit, not a bound of its own; read-ahead is bounded again by the cache the manifest
/// affords ([`Volume::load`]).
const READAHEAD_BLOCKS: usize = MAX_SECTORS as usize / SECTORS_PER_BLOCK as usize;

/// What [`Caps::from_heap`] keeps of the heap's affordable half ([`Caps::from_heap`]) for
/// everything but the data cache, so the peak stays under half the manifest's cap: the tree cache
/// (32), the read-ahead run and the server's scratch (one `blkd` read, 8 blocks, each), the top
/// tree block (1), the bookkeeping the boot profile measured (about 3), and a few pages of margin.
/// Measured on `init-boot`'s memory dump (docs/testbench.md, "The memory budget").
const HEAP_RESERVE_BLOCKS: usize = 32 + 8 + 8 + 1 + 3 + 4;

/// How many blocks the tree and data caches hold. The tree cache stays at [`TREE_CACHE`] (one
/// reader's tree path fits it and a second reader's is the same few blocks, servers/verityd.md);
/// the data cache takes the rest of the heap the manifest affords.
#[derive(Clone, Copy)]
pub struct Caps {
    pub tree: usize,
    pub data: usize,
}

impl Caps {
    /// The floors, for a host test or a launch with no heap cap in its startup block.
    pub const DEFAULT: Caps = Caps { tree: TREE_CACHE, data: DATA_CACHE };

    /// The caps the heap cap `heap_pages` (the manifest's, through init) affords: the tree cache,
    /// then the data cache fills half the heap less the reserve. The memory budget gate holds a
    /// server's heap cap to twice its peak (docs/testbench.md, "The memory budget"), so the peak
    /// the manifest can hold is half the cap; the data cache takes that half beyond everything else
    /// the heap holds ([`HEAP_RESERVE_BLOCKS`]), never below [`DATA_CACHE`]. A heap too small for
    /// more than the floor keeps the floor.
    pub fn from_heap(heap_pages: Option<usize>) -> Caps {
        let Some(pages) = heap_pages else { return Caps::DEFAULT };
        let data = (pages / 2).saturating_sub(HEAP_RESERVE_BLOCKS).max(DATA_CACHE);
        Caps { tree: TREE_CACHE, data }
    }
}

/// Why the volume is refused at start: every read then fails, and `verityd` stays up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `blkd` would not size the range.
    NoInfo,
    /// The range is shorter than the volume's data blocks and their tree.
    Truncated,
    /// No memory for the tree's blocks.
    NoMemory,
    /// The top tree block could not be read.
    Unread,
    /// The top tree block does not hash to the root.
    Root,
    /// A signed volume's root block could not be read.
    RootBlockUnread,
    /// A signed volume's root block is not one (`RootBlock::parse`).
    Malformed,
    /// A signed volume's root block's signature does not verify under the manifest's key.
    Signature,
    /// A signed volume's root block's version is below the manifest's floor: a rollback.
    Version { version: u64, floor: u64 },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::NoInfo => "blkd would not size its range",
            Refusal::Truncated => "its range is shorter than its blocks and their tree",
            Refusal::NoMemory => "no memory for its tree",
            Refusal::Unread => "its top tree block could not be read",
            Refusal::Root => "its top tree block does not match the root",
            Refusal::RootBlockUnread => "its root block could not be read",
            Refusal::Malformed => "its root block is malformed",
            Refusal::Signature => "its root block's signature does not verify under its key",
            Refusal::Version { version, floor } => {
                return write!(f, "its root block's version {version} is below the floor {floor}");
            }
        })
    }
}

/// A read that did not check, by the block of the range it was about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bad {
    /// `blkd` failed to give the block.
    Unread(u64),
    /// A tree block does not hash to its slot in the block above.
    Tree(u64),
    /// A data block does not hash to its slot in level 1.
    Data(u64),
}

impl fmt::Display for Bad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bad::Unread(b) => write!(f, "block {b} could not be read"),
            Bad::Tree(b) => write!(f, "tree block {b} does not match the tree"),
            Bad::Data(b) => write!(f, "block {b} does not match the tree"),
        }
    }
}

/// What the volume has done: the tree cache's work, which the host tests read and a
/// `boot-stats` build says.
#[cfg(any(test, feature = "boot-stats"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Data blocks checked.
    pub checked: u64,
    /// Of those, the ones whose level-1 tree block was already held.
    pub hits: u64,
    /// Reads at `blkd`: data blocks and tree blocks.
    pub reads: u64,
    /// Data blocks asked for.
    pub requests: u64,
    /// Of those, the ones already held, checked, in the data cache.
    pub held: u64,
}

/// A vector of `len` zero bytes, or `None` if there is no memory for it.
fn zeroed(len: usize) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    v.try_reserve_exact(len).ok()?;
    v.resize(len, 0);
    Some(v)
}

/// Checked blocks, `n` of them, least recently used out first: the tree's and the data's. The
/// count is sized from the manifest at start ([`Caps`]), not compiled, so a server with more of
/// its budget holds a bigger working set; the tags and the block bytes are one heap allocation
/// each, both tried fallibly.
struct Cache {
    tags: Vec<Option<u64>>,
    used: Vec<u64>,
    clock: u64,
    blocks: Vec<u8>,
}

impl Cache {
    /// An empty cache of `n` blocks, its tags and `n * BLOCK` bytes reserved; `None` if either
    /// allocation fails, so the start check refuses for no memory.
    fn new(n: usize) -> Option<Cache> {
        let mut tags = Vec::new();
        tags.try_reserve_exact(n).ok()?;
        tags.resize(n, None);
        let mut used = Vec::new();
        used.try_reserve_exact(n).ok()?;
        used.resize(n, 0);
        let blocks = zeroed(n * BLOCK)?;
        Some(Cache { tags, used, clock: 0, blocks })
    }

    fn cap(&self) -> usize { self.tags.len() }

    fn bytes(&self, i: usize) -> &[u8] { &self.blocks[i * BLOCK..(i + 1) * BLOCK] }

    /// Whether block `n` is held, without marking it used: for read-ahead's run detection.
    fn holds(&self, n: u64) -> bool { self.tags.iter().any(|t| *t == Some(n)) }

    /// The slot holding tree block `n`, marked used.
    fn find(&mut self, n: u64) -> Option<usize> {
        let i = self.tags.iter().position(|t| *t == Some(n))?;
        self.clock += 1;
        self.used[i] = self.clock;
        Some(i)
    }

    /// A slot to read a block into, emptied: a free one, else the least recently used.
    fn victim(&mut self) -> usize {
        let i = (0..self.cap()).min_by_key(|&i| (self.tags[i].is_some(), self.used[i])).unwrap_or(0);
        self.tags[i] = None;
        i
    }

    /// Keeps slot `i` as block `n`, which was checked.
    fn keep(&mut self, i: usize, n: u64) {
        self.clock += 1;
        self.tags[i] = Some(n);
        self.used[i] = self.clock;
    }
}

/// Slot `s` of `block` as a digest; a slot that is not there is no digest anything hashes to.
fn digest_at(block: &[u8], s: usize) -> Hash {
    let mut out = [0u8; DIGEST];
    if let Some(d) = slot(block, s) {
        out.copy_from_slice(d);
    }
    out
}

/// The range, the tree's geometry and what of it is held.
struct Tree<R> {
    range: R,
    geometry: Geometry,
    top: Vec<u8>,
    cache: Cache,
    #[cfg(any(test, feature = "boot-stats"))]
    counts: Counts,
}

impl<R: Range> Tree<R> {
    /// Reads the blocks from `n` into `out` (one block, or a read-ahead run of at most one `blkd`
    /// read's [`READAHEAD_BLOCKS`]) in one `blkd` read, up to five calls at the two-page lend.
    /// Each is checked by the caller before it is trusted or kept.
    fn read(&mut self, n: u64, out: &mut [u8]) -> Result<(), Bad> {
        #[cfg(any(test, feature = "boot-stats"))]
        {
            self.counts.reads += 1;
        }
        // `n` is inside the volume, whose sectors count in `u64` (`Geometry::new`).
        self.range.read(n * SECTORS_PER_BLOCK, out).map_err(|_| Bad::Unread(n))
    }

    /// Checks that data block `b` hashes to `leaf` through the tree.
    fn check(&mut self, b: u64, leaf: &Hash) -> Result<(), Bad> {
        let g = self.geometry;
        let top = g.levels() - 1;
        // The lowest level whose block on `b`'s path is held; the top always is.
        let mut from = top;
        for level in 0..top {
            let (n, _) = g.node(level, b).ok_or(Bad::Data(b))?;
            if self.cache.find(n).is_some() {
                from = level;
                break;
            }
        }
        #[cfg(any(test, feature = "boot-stats"))]
        {
            self.counts.checked += 1;
            self.counts.hits += u64::from(from == 0);
        }
        let (n, s) = g.node(from, b).ok_or(Bad::Data(b))?;
        let mut want = match self.cache.find(n) {
            Some(i) if from < top => digest_at(self.cache.bytes(i), s),
            _ => digest_at(&self.top, s),
        };
        for level in (0..from).rev() {
            let (n, s) = g.node(level, b).ok_or(Bad::Data(b))?;
            let i = self.cache.victim();
            let mut block = core::mem::take(&mut self.cache.blocks);
            let read = self.read(n, &mut block[i * BLOCK..(i + 1) * BLOCK]);
            self.cache.blocks = block;
            read?;
            if node(self.cache.bytes(i)) != want {
                return Err(Bad::Tree(n));
            }
            self.cache.keep(i, n);
            want = digest_at(self.cache.bytes(i), s);
        }
        if *leaf != want {
            return Err(Bad::Data(b));
        }
        Ok(())
    }
}

/// A volume that passed the start check.
pub struct Volume<R> {
    tree: Tree<R>,
    /// The data blocks checked most recently, and their bytes.
    data: Cache,
    /// A read-ahead run's bytes, at most one `blkd` read (several calls at the lend's size),
    /// checked into the cache ([`Volume::load`]).
    ra: Vec<u8>,
}

/// A signed volume's N and root, from the root block in the last whole block of the range
/// `size` gives, and the sector it starts at, which the data and tree must end by. The block is
/// parsed, its signature verified under `key`, and its version held to `floor`, in that order:
/// nothing in it is used before the signature verifies. Refused, the sectors `info` answers: the
/// whole blocks before the root block, the most the volume could be.
fn signed<R: Range>(
    range: &mut R,
    size: Result<Size, Fault>,
    key: &[u8; 32],
    floor: u64,
) -> Result<(Geometry, Hash, u64), (Refusal, u64)> {
    let size = size.map_err(|_| (Refusal::NoInfo, 0))?;
    let at = root_block_at(size.sectors).ok_or((Refusal::Truncated, 0))? * SECTORS_PER_BLOCK;
    let mut block = zeroed(BLOCK).ok_or((Refusal::NoMemory, at))?;
    range.read(at, &mut block).map_err(|_| (Refusal::RootBlockUnread, at))?;
    let rb = RootBlock::parse(&block).map_err(|_| (Refusal::Malformed, at))?;
    PublicKey::new(*key)
        .verify(rb.signed(), &Signature::new(rb.signature))
        .map_err(|_| (Refusal::Signature, at))?;
    if rb.version < floor {
        return Err((Refusal::Version { version: rb.version, floor }, at));
    }
    Ok((rb.geometry, rb.root, at))
}

impl<R: Range> Volume<R> {
    /// Checks `range` at start against `mode`, and gives the volume's sectors, which `info`
    /// answers whether the check passed or not. Pinned, the manifest gives N and the root; signed,
    /// the root block does ([`signed`]). Then `info` must size the range no shorter than the
    /// data and tree (for a signed volume, before its root block), and the top tree block must
    /// hash, with N, to the root.
    pub fn open(mut range: R, mode: &Mode, caps: Caps) -> (Result<Volume<R>, Refusal>, u64) {
        let size = range.info();
        let (geometry, root_, end) = match *mode {
            Mode::Pinned { root, geometry } => (geometry, root, size.map(|s| s.sectors)),
            Mode::Signed { key, floor } => match signed(&mut range, size, &key, floor) {
                Ok((geometry, root, at)) => (geometry, root, Ok(at)),
                Err((why, sectors)) => return (Err(why), sectors),
            },
        };
        (Volume::check(range, end, geometry, &root_, caps), geometry.data_blocks() * SECTORS_PER_BLOCK)
    }

    /// The start check once N and the root are known: the data and tree end by `end`, and the
    /// top tree block hashes to `root_`.
    fn check(
        mut range: R,
        end: Result<u64, Fault>,
        geometry: Geometry,
        root_: &Hash,
        caps: Caps,
    ) -> Result<Volume<R>, Refusal> {
        if end.map_err(|_| Refusal::NoInfo)? < geometry.total_sectors() {
            return Err(Refusal::Truncated);
        }
        let (Some(mut top), Some(tree_cache), Some(data), Some(ra)) = (
            zeroed(BLOCK),
            Cache::new(caps.tree),
            Cache::new(caps.data),
            zeroed(MAX_SECTORS as usize * SECTOR as usize),
        ) else {
            return Err(Refusal::NoMemory);
        };
        range.read(geometry.top() * SECTORS_PER_BLOCK, &mut top).map_err(|_| Refusal::Unread)?;
        if root(geometry.data_blocks(), &top) != *root_ {
            return Err(Refusal::Root);
        }
        let tree = Tree {
            range,
            geometry,
            top,
            cache: tree_cache,
            #[cfg(any(test, feature = "boot-stats"))]
            counts: Counts { reads: 1, ..Counts::default() },
        };
        Ok(Volume { tree, data, ra })
    }

    /// Makes block `b` checked and held, ready for [`Volume::peek`], and returns the last block it
    /// made ready (`>= b`, at most `last`): `b` if it was already held, else the last of the run it
    /// read. A run of consecutive missing blocks (a file's cold data) through `last` is read from
    /// `blkd` in one read of at most one read-ahead run and the cache (up to five `blkd` calls at
    /// the two-page lend), each checked against the tree and kept; the bound on the cache keeps
    /// the run from putting out its own blocks before the caller reads them. A block that does
    /// not check, or is past the volume, is an error: the checked blocks before it stay kept and
    /// the rest is read again next time.
    pub fn load(&mut self, b: u64, last: u64) -> Result<u64, Bad> {
        let blocks = self.tree.geometry.data_blocks();
        if b >= blocks {
            return Err(Bad::Data(b));
        }
        if self.data.find(b).is_some() {
            #[cfg(any(test, feature = "boot-stats"))]
            {
                self.tree.counts.requests += 1;
                self.tree.counts.held += 1;
            }
            return Ok(b);
        }
        let cap = READAHEAD_BLOCKS.min(self.data.cap());
        let mut run = 1u64;
        while b + run <= last && b + run < blocks && (run as usize) < cap && !self.data.holds(b + run) {
            run += 1;
        }
        self.tree.read(b, &mut self.ra[..run as usize * BLOCK])?;
        for k in 0..run {
            #[cfg(any(test, feature = "boot-stats"))]
            {
                self.tree.counts.requests += 1;
            }
            let bytes = &self.ra[k as usize * BLOCK..(k as usize + 1) * BLOCK];
            self.tree.check(b + k, &leaf(bytes))?;
            let i = self.data.victim();
            self.data.blocks[i * BLOCK..(i + 1) * BLOCK].copy_from_slice(bytes);
            self.data.keep(i, b + k);
        }
        Ok(b + run - 1)
    }

    /// Data block `b`, held and checked by a [`Volume::load`] that reached it; `None` if it is not
    /// held (the caller reads the blocks a load made ready before the next load can put them out).
    pub fn peek(&mut self, b: u64) -> Option<&[u8]> { self.data.find(b).map(|i| self.data.bytes(i)) }

    #[cfg(any(test, feature = "boot-stats"))]
    pub fn counts(&self) -> Counts { self.tree.counts }
}
