extern crate std;

use std::format;
use std::vec;
use std::vec::Vec;

use super::*;

fn hex(h: &Hash) -> std::string::String { h.iter().map(|b| std::format!("{b:02x}")).collect() }

/// `n` data blocks, block i filled with the byte i mod 256, and their tree.
fn volume(n: u64) -> (Geometry, Vec<u8>, Vec<u8>, Hash) {
    let g = Geometry::new(n).unwrap();
    let mut data = vec![0u8; n as usize * BLOCK];
    for (i, block) in data.chunks_exact_mut(BLOCK).enumerate() {
        block.fill(i as u8);
    }
    let mut tree = vec![0xa5u8; g.tree_blocks() as usize * BLOCK];
    let root = build(&g, &data, &mut tree).unwrap();
    (g, data, tree, root)
}

/// Checks data block `block` from the root down, as `verityd` does with nothing cached: the top
/// against the root, then each tree block on the path against its slot in the one above, then
/// the data block against its slot in level 1.
fn verified(g: &Geometry, data: &[u8], tree: &[u8], root_: &Hash, block: u64) -> bool {
    let at = |b: u64| &tree[(b - g.data_blocks()) as usize * BLOCK..][..BLOCK];
    if root(g.data_blocks(), at(g.top())) != *root_ {
        return false;
    }
    for level in (0..g.levels()).rev() {
        let (parent, slot_) = g.node(level, block).unwrap();
        let child = match level {
            0 => leaf(&data[block as usize * BLOCK..][..BLOCK]),
            _ => node(at(g.node(level - 1, block).unwrap().0)),
        };
        if slot(at(parent), slot_) != Some(&child[..]) {
            return false;
        }
    }
    true
}

/// The vectors were computed apart from this crate (Python's `hashlib`) from the page's
/// definition.
#[test]
fn a_known_vector() {
    let g = Geometry::new(1).unwrap();
    let mut tree = vec![0u8; BLOCK];
    let root = build(&g, &[0u8; BLOCK], &mut tree).unwrap();
    assert_eq!(hex(&root), "f3a975b178d5e13d7af8e4435ac1e6f05f553eef23f0f3500f93957dbfd05c77");
    let (_, _, _, root) = volume(129);
    assert_eq!(hex(&root), "f05b156fe41b052e99654eb2ceaf4e03de02c494c1e89e916b9b4ddd46bfff9a");
}

/// Every level boundary: one block, a full level-1 block, one past it, and one past a full
/// two-level tree.
#[test]
fn every_level_boundary() {
    for (n, levels, tree) in
        [(1, 1, 1), (128, 1, 1), (129, 2, 3), (128 * 128, 2, 129), (128 * 128 + 1, 3, 132)]
    {
        let g = Geometry::new(n).unwrap();
        assert_eq!(
            (g.levels(), g.tree_blocks(), g.total_sectors()),
            (levels, tree, (n + tree) * SECTORS_PER_BLOCK),
            "{n}"
        );
        assert_eq!(g.top(), n + tree - 1, "the top is stored last");
        assert_eq!(g.node(0, 0).unwrap().0, n, "level 1 starts after the data");
        assert_eq!(g.node(0, n - 1).map(|(_, s)| s), Some(((n - 1) % FANOUT) as usize));
        assert_eq!(g.node(0, n), None, "past the data");
        assert_eq!(g.node(levels, 0), None, "past the top");
        assert_eq!(g.node(levels - 1, n - 1).unwrap().0, g.top());
    }
}

#[test]
fn the_geometry_refuses_no_blocks_and_overflow() {
    assert_eq!(Geometry::new(0), None);
    assert_eq!(Geometry::new(u64::MAX), None, "the tree does not fit beside it");
    assert_eq!(Geometry::new(u64::MAX / SECTORS_PER_BLOCK), None, "nor its sectors in u64");
    let big = Geometry::new(u64::MAX / 9).unwrap();
    assert!(
        big.levels() <= MAX_LEVELS
            && big.total_sectors() / SECTORS_PER_BLOCK == big.data_blocks() + big.tree_blocks()
    );
}

/// The largest volume fills its blocks, and one more data block would not fit.
#[test]
fn the_largest_volume_fits_its_range() {
    assert_eq!(Geometry::largest(0), None);
    assert_eq!(Geometry::largest(1), None);
    for blocks in [2, 3, 129, 130, 131, 4096, 16_515, 16_516, 16_517, 1 << 20] {
        let g = Geometry::largest(blocks).unwrap();
        assert!(g.total_sectors() / SECTORS_PER_BLOCK <= blocks, "{blocks}");
        let more = Geometry::new(g.data_blocks() + 1).unwrap();
        assert!(more.total_sectors() / SECTORS_PER_BLOCK > blocks, "{blocks}");
    }
}

/// A single flipped bit in a data block, in each level of the tree, and in the root fails the
/// check of every block it covers; the blocks it does not cover still pass.
#[test]
fn a_flipped_bit_at_each_level_is_refused() {
    let (g, data, tree, root) = volume(128 * 128 + 1);
    assert_eq!(g.levels(), 3);
    let last = g.data_blocks() - 1;
    for block in [0, 127, 128, last] {
        assert!(verified(&g, &data, &tree, &root, block), "{block}");
    }
    // A data block.
    let mut bad = data.clone();
    bad[200 * BLOCK + 7] ^= 1;
    assert!(!verified(&g, &bad, &tree, &root, 200));
    assert!(verified(&g, &bad, &tree, &root, 201));
    // Each tree level, in the block on block 0's path, and a slot past the last digest's: the
    // zero fill is covered too.
    for level in 0..g.levels() {
        let (node, _) = g.node(level, 0).unwrap();
        let off = (node - g.data_blocks()) as usize * BLOCK;
        for at in [off + 5, off + BLOCK - 1] {
            let mut bad = tree.clone();
            bad[at] ^= 0x80;
            assert!(!verified(&g, &data, &bad, &root, 0), "level {level} byte {at}");
        }
        if level == 0 {
            let mut bad = tree.clone();
            bad[off + 5] ^= 0x80;
            assert!(verified(&g, &data, &bad, &root, last), "the last block is under another level-1 block");
        }
    }
    let mut wrong = root;
    wrong[31] ^= 1;
    assert!(!verified(&g, &data, &tree, &wrong, last));
}

/// The root pins N: the same top block under another count is another root.
#[test]
fn the_root_pins_the_block_count() {
    let top = [7u8; BLOCK];
    assert_ne!(root(1, &top), root(2, &top));
    assert_ne!(leaf(&top), node(&top), "a data block is never taken for a tree block");
}

#[test]
fn build_takes_exactly_the_geometrys_sizes() {
    let g = Geometry::new(2).unwrap();
    let mut tree = vec![0u8; BLOCK];
    assert_eq!(build(&g, &[0; BLOCK], &mut tree), Err(WrongSize));
    assert_eq!(build(&g, &[0; 2 * BLOCK], &mut [0u8; 2 * BLOCK]), Err(WrongSize));
    assert!(build(&g, &[0; 2 * BLOCK], &mut tree).is_ok());
    assert_eq!(slot(&tree, FANOUT as usize), None);
    assert_eq!(slot(&tree, usize::MAX), None);
}

#[test]
fn a_root_is_64_lowercase_hex_digits() {
    let hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    let root = from_hex(hex).unwrap();
    assert_eq!((root[0], root[1], root[31]), (0x00, 0x11, 0xff));
    for bad in [&hex[1..], &hex.to_uppercase(), &hex.replace('a', "g"), "", &format!("{hex}0")] {
        assert_eq!(from_hex(bad), None, "{bad}");
    }
}

fn root_block() -> RootBlock {
    RootBlock {
        geometry: Geometry::new(4054).unwrap(),
        version: 7,
        root: core::array::from_fn(|i| i as u8),
        signature: core::array::from_fn(|i| 0x80 | i as u8),
    }
}

/// The layout as servers/verityd.md states it, byte by byte, and read back.
#[test]
fn a_root_block_is_its_documented_bytes() {
    let r = root_block();
    let block = r.encode();
    assert_eq!(&block[..8], b"RVOLROOT");
    assert_eq!(&block[8..16], &4054u64.to_le_bytes());
    assert_eq!(&block[16..24], &7u64.to_le_bytes());
    assert_eq!(&block[24..56], &r.root);
    assert_eq!(&block[56..120], &r.signature);
    assert!(block[120..].iter().all(|b| *b == 0));
    assert_eq!(ROOT_BLOCK_USED, 120);
    assert_eq!(RootBlock::parse(&block), Ok(r));
    assert_eq!(r.signed(), redoubt_signing::volume_preimage(4054, 7, &r.root));
}

/// A short or long block, the wrong magic, a block count with no geometry, and any byte of the
/// fill are refused.
#[test]
fn a_malformed_root_block_is_refused() {
    let block = root_block().encode();
    assert_eq!(RootBlock::parse(&block[..BLOCK - 1]), Err(Malformed));
    assert_eq!(RootBlock::parse(&[]), Err(Malformed));
    let mut long = block.to_vec();
    long.push(0);
    assert_eq!(RootBlock::parse(&long), Err(Malformed));
    let changed = |at: usize, to: &[u8]| {
        let mut bad = block;
        bad[at..at + to.len()].copy_from_slice(to);
        RootBlock::parse(&bad)
    };
    assert_eq!(changed(0, b"R"), Ok(root_block()), "the same byte changes nothing");
    assert_eq!(changed(0, b"S"), Err(Malformed));
    assert_eq!(changed(8, &0u64.to_le_bytes()), Err(Malformed), "no blocks");
    assert_eq!(changed(8, &u64::MAX.to_le_bytes()), Err(Malformed), "no tree fits beside them");
    for at in [120, 2000, BLOCK - 1] {
        assert_eq!(changed(at, &[1]), Err(Malformed), "fill byte {at}");
    }
}

/// Arbitrary bytes are a root block that encodes back to themselves, or refused, and never panic:
/// the body of the fuzz target (`fuzz/fuzz_targets/root_block.rs`), over generated blocks.
#[test]
fn arbitrary_bytes_are_one_root_block_or_none() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let good = root_block().encode();
    let mut parsed = 0;
    for _ in 0..2000 {
        let mut block = good;
        // A few bytes changed at random, so some of what is tried is a root block.
        for _ in 0..next() % 3 {
            let at = (next() % ROOT_BLOCK_USED as u64) as usize;
            block[at] = next() as u8;
        }
        let len = if next() % 8 == 0 { (next() % BLOCK as u64) as usize } else { BLOCK };
        if let Ok(r) = RootBlock::parse(&block[..len]) {
            assert_eq!(r.encode(), block);
            parsed += 1;
        }
    }
    assert!(parsed > 100, "{parsed}");
}

/// The root block is the range's last whole block: a partial block at the end is not it.
#[test]
fn the_root_block_is_the_last_whole_block() {
    assert_eq!(root_block_at(0), None);
    assert_eq!(root_block_at(SECTORS_PER_BLOCK - 1), None);
    assert_eq!(root_block_at(SECTORS_PER_BLOCK), Some(0));
    assert_eq!(root_block_at(10 * SECTORS_PER_BLOCK + 7), Some(9));
}
