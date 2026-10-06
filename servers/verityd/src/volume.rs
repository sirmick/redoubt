//! The volume as `verityd` checks it (docs/servers/verityd.md, "Reading"): the start check, and
//! each data block checked through the tree before any of it leaves.
//!
//! **Memory is fixed, whatever the volume's size:** the top tree block, pinned at start;
//! [`TREE_CACHE`] tree blocks, each kept only once it was checked against the block above it; and
//! [`DATA_CACHE`] checked data blocks, least recently used out first, so a block read again (a
//! directory block, the block holding a file's inode) is hashed and fetched once while it is held.
//!
//! **A block is checked from the top down.** The lowest block on its path that is already held
//! (the top always is) gives the digest the next block down must hash to; each block fetched is
//! checked against it before it is kept or used, and the data block against its slot in level 1.
//! So nothing unchecked is ever trusted, and a hit on level 1 costs one hash.

use alloc::vec::Vec;
use core::fmt;

use redoubt_verity::{BLOCK, DIGEST, Geometry, Hash, SECTORS_PER_BLOCK, leaf, node, root, slot};

use crate::Range;

/// Tree blocks kept after they were checked, beside the pinned top: 128 KiB.
pub const TREE_CACHE: usize = 32;
/// Checked data blocks kept: 16 KiB. Sized from the EROFS boot profile: by the 512th read, 1, 4 and
/// 8 blocks held 100, 197 and 289 of 849 blocks asked for (servers/verityd.md, "A cache of checked
/// data blocks"); 8 is the next cut.
pub const DATA_CACHE: usize = 4;

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
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::NoInfo => "blkd would not size its range",
            Refusal::Truncated => "its range is shorter than its blocks and their tree",
            Refusal::NoMemory => "no memory for its tree",
            Refusal::Unread => "its top tree block could not be read",
            Refusal::Root => "its top tree block does not match the root",
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

/// Checked blocks, `N` of them, least recently used out first: the tree's and the data's.
struct Cache<const N: usize> {
    tags: [Option<u64>; N],
    used: [u64; N],
    clock: u64,
    blocks: Vec<u8>,
}

impl<const N: usize> Cache<N> {
    /// An empty cache over `blocks`, `N` blocks of bytes.
    fn new(blocks: Vec<u8>) -> Cache<N> { Cache { tags: [None; N], used: [0; N], clock: 0, blocks } }

    fn bytes(&self, i: usize) -> &[u8] { &self.blocks[i * BLOCK..(i + 1) * BLOCK] }

    /// The slot holding tree block `n`, marked used.
    fn find(&mut self, n: u64) -> Option<usize> {
        let i = self.tags.iter().position(|t| *t == Some(n))?;
        self.clock += 1;
        self.used[i] = self.clock;
        Some(i)
    }

    /// A slot to read a block into, emptied: a free one, else the least recently used.
    fn victim(&mut self) -> usize {
        let i = (0..N).min_by_key(|&i| (self.tags[i].is_some(), self.used[i])).unwrap_or(0);
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
    cache: Cache<TREE_CACHE>,
    #[cfg(any(test, feature = "boot-stats"))]
    counts: Counts,
}

impl<R: Range> Tree<R> {
    /// Reads block `n` of the range into `out`, one block.
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
    data: Cache<DATA_CACHE>,
}

impl<R: Range> Volume<R> {
    /// Checks `range` at start: `info` sizes it no shorter than `geometry`'s data and tree, and
    /// its top tree block hashes, with the block count, to `root`.
    pub fn open(mut range: R, geometry: Geometry, root_: &Hash) -> Result<Volume<R>, Refusal> {
        let size = range.info().map_err(|_| Refusal::NoInfo)?;
        if size.sectors < geometry.total_sectors() {
            return Err(Refusal::Truncated);
        }
        let (Some(mut top), Some(blocks), Some(data)) =
            (zeroed(BLOCK), zeroed(TREE_CACHE * BLOCK), zeroed(DATA_CACHE * BLOCK))
        else {
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
            cache: Cache::new(blocks),
            #[cfg(any(test, feature = "boot-stats"))]
            counts: Counts { reads: 1, ..Counts::default() },
        };
        Ok(Volume { tree, data: Cache::new(data) })
    }

    /// Data block `b`, checked; a block that does not check is never returned, and is read again
    /// next time.
    pub fn block(&mut self, b: u64) -> Result<&[u8], Bad> {
        #[cfg(any(test, feature = "boot-stats"))]
        {
            self.tree.counts.requests += 1;
        }
        if let Some(i) = self.data.find(b) {
            #[cfg(any(test, feature = "boot-stats"))]
            {
                self.tree.counts.held += 1;
            }
            return Ok(self.data.bytes(i));
        }
        if b >= self.tree.geometry.data_blocks() {
            return Err(Bad::Data(b));
        }
        let i = self.data.victim();
        let bytes = &mut self.data.blocks[i * BLOCK..(i + 1) * BLOCK];
        self.tree.read(b, bytes)?;
        self.tree.check(b, &leaf(bytes))?;
        self.data.keep(i, b);
        Ok(self.data.bytes(i))
    }

    #[cfg(any(test, feature = "boot-stats"))]
    pub fn counts(&self) -> Counts { self.tree.counts }
}
