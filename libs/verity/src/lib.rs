//! A verified volume's hash tree (docs/servers/verityd.md, "The tree"; R76 (verified volumes)):
//! one definition, shared by the packer that writes it and `verityd` that checks it.
//!
//! - A volume of N data blocks of [`BLOCK`] bytes (littlefs's block, eight sectors) is followed in its range
//!   by its tree.
//! - Level 1 holds one digest per data block, `SHA-256(0x00 ‖ block)`. Each level above holds one digest per
//!   block of the level below, `SHA-256(0x01 ‖ block)`. A tree block holds [`FANOUT`] digests, zero-filled
//!   after the last. Levels are stored bottom-up; the top level is one block.
//! - The root is `SHA-256(0x02 ‖ N as u64 LE ‖ top block)`, so it pins the block count too.
//!
//! The three prefixes keep a data block, a tree block and the root apart: no block can be taken
//! for one of another kind. Every bit of data and tree is covered, so any error in either is
//! found; nothing here corrects one.
//!
//! The geometry is overflow-checked arithmetic in one function, [`Geometry::new`]; everything
//! else reads what it computed.

#![no_std]
#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};

/// A block of the volume and of its tree, in bytes: littlefs's block.
pub const BLOCK: usize = 4096;
/// A digest, in bytes.
pub const DIGEST: usize = 32;
/// Digests per tree block.
pub const FANOUT: u64 = (BLOCK / DIGEST) as u64;
/// `blkd`'s sectors per block.
pub const SECTORS_PER_BLOCK: u64 = 8;
/// The most levels a tree has: `FANOUT`⁹ is 2⁶³, so ten levels cover any `u64` block count.
pub const MAX_LEVELS: usize = 10;

/// The prefix of a data block's digest.
const LEAF: u8 = 0x00;
/// The prefix of a tree block's digest.
const NODE: u8 = 0x01;
/// The prefix of the root.
const ROOT: u8 = 0x02;

/// A digest: a block's, or the root.
pub type Hash = [u8; DIGEST];

/// Where a volume of N data blocks keeps its tree: each level's first block, counted from the
/// start of the range, and its length in blocks. Level 0 here is the page's level 1, the data
/// blocks' digests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    data: u64,
    levels: usize,
    start: [u64; MAX_LEVELS],
    count: [u64; MAX_LEVELS],
    total: u64,
}

impl Geometry {
    /// The tree of `data` blocks, or `None` for no blocks, or a volume whose blocks and tree
    /// together do not count in `u64` sectors.
    pub fn new(data: u64) -> Option<Geometry> {
        if data == 0 {
            return None;
        }
        let mut g = Geometry { data, levels: 0, start: [0; MAX_LEVELS], count: [0; MAX_LEVELS], total: 0 };
        let (mut at, mut below) = (data, data);
        loop {
            let count = below.div_ceil(FANOUT);
            // Unreachable for a `u64` count (see `MAX_LEVELS`), and refused rather than assumed.
            let level = g.start.get_mut(g.levels)?;
            *level = at;
            g.count[g.levels] = count;
            at = at.checked_add(count)?;
            g.levels += 1;
            if count == 1 {
                break;
            }
            below = count;
        }
        at.checked_mul(SECTORS_PER_BLOCK)?;
        g.total = at;
        Some(g)
    }

    /// The largest volume whose data and tree fit `blocks` blocks, or `None` if not even one
    /// data block and its tree do.
    pub fn largest(blocks: u64) -> Option<Geometry> {
        // The tree of `blocks` data blocks is at least the tree of any fewer, so this many fit;
        // a few more may, as the tree shrinks with the data.
        let tree = Geometry::new(blocks)?.tree_blocks();
        let mut fit = Geometry::new(blocks.checked_sub(tree)?)?;
        while let Some(more) = fit.data.checked_add(1).and_then(Geometry::new) {
            if more.total > blocks {
                break;
            }
            fit = more;
        }
        Some(fit)
    }

    /// N, the data blocks.
    pub fn data_blocks(&self) -> u64 { self.data }

    /// The tree's blocks, every level.
    pub fn tree_blocks(&self) -> u64 { self.total - self.data }

    /// The data and the tree, in `blkd`'s sectors (checked in [`Geometry::new`]).
    pub fn total_sectors(&self) -> u64 { self.total * SECTORS_PER_BLOCK }

    /// The tree's levels.
    pub fn levels(&self) -> usize { self.levels }

    /// The top block, which the root covers.
    pub fn top(&self) -> u64 { self.start[self.levels - 1] }

    /// The tree block at `level` that covers data block `block`, and the slot in it of the digest
    /// of what lies below on `block`'s path: the data block itself at level 0, the level below's
    /// block above it. `None` for a block past the data or a level past the top.
    pub fn node(&self, level: usize, block: u64) -> Option<(u64, usize)> {
        if block >= self.data || level >= self.levels {
            return None;
        }
        let mut below = block;
        for _ in 0..level {
            below /= FANOUT;
        }
        Some((self.start[level] + below / FANOUT, (below % FANOUT) as usize))
    }
}

fn digest(prefix: u8, block: &[u8]) -> Hash {
    let mut h = Sha256::new();
    h.update([prefix]);
    h.update(block);
    h.finalize().into()
}

/// A data block's digest, as level 1 holds it.
pub fn leaf(block: &[u8]) -> Hash { digest(LEAF, block) }

/// A tree block's digest, as the level above holds it.
pub fn node(block: &[u8]) -> Hash { digest(NODE, block) }

/// The root of a volume of `data` blocks whose top tree block is `top`.
pub fn root(data: u64, top: &[u8]) -> Hash {
    let mut h = Sha256::new();
    h.update([ROOT]);
    h.update(data.to_le_bytes());
    h.update(top);
    h.finalize().into()
}

/// The root, or any digest, from 64 lowercase hex digits, the form a manifest gives it; `None`
/// for anything else.
pub fn from_hex(s: &str) -> Option<Hash> {
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let bytes = s.as_bytes();
    if bytes.len() != 2 * DIGEST {
        return None;
    }
    let mut out = [0u8; DIGEST];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        out[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(out)
}

/// Slot `slot` of a tree block, or `None` past its end.
pub fn slot(block: &[u8], slot: usize) -> Option<&[u8]> {
    block.get(slot.checked_mul(DIGEST)?..slot.checked_mul(DIGEST)?.checked_add(DIGEST)?)
}

/// The packer's sizes do not match the geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrongSize;

/// Writes the tree of `data` (exactly N blocks) into `tree` (exactly the geometry's tree blocks,
/// overwritten whole) and returns the root.
pub fn build(g: &Geometry, data: &[u8], tree: &mut [u8]) -> Result<Hash, WrongSize> {
    let bytes = |blocks: u64| usize::try_from(blocks).ok().and_then(|b| b.checked_mul(BLOCK));
    if bytes(g.data) != Some(data.len()) || bytes(g.tree_blocks()) != Some(tree.len()) {
        return Err(WrongSize);
    }
    tree.fill(0);
    // A tree block's offset in `tree`, from its block number in the range.
    let at = |block: u64| (block - g.data) as usize * BLOCK;
    for (i, block) in data.chunks_exact(BLOCK).enumerate() {
        let (node, slot) = g.node(0, i as u64).ok_or(WrongSize)?;
        let off = at(node) + slot * DIGEST;
        tree[off..off + DIGEST].copy_from_slice(&leaf(block));
    }
    for level in 1..g.levels {
        let (below, count) = (g.start[level - 1], g.count[level - 1]);
        for j in 0..count {
            let child = at(below + j);
            let hash = node(&tree[child..child + BLOCK]);
            let off = at(g.start[level] + j / FANOUT) + (j % FANOUT) as usize * DIGEST;
            tree[off..off + DIGEST].copy_from_slice(&hash);
        }
    }
    let top = at(g.top());
    Ok(root(g.data, &tree[top..top + BLOCK]))
}

#[cfg(test)]
mod tests;
