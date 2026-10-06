//! The parser's checks one at a time, on volumes the writer packs and then breaks; and the writer
//! against the parser over random trees (docs/servers/erofsd.md, "The format", "The packer").

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use super::*;

fn sha256(data: &[u8]) -> [u8; 32] { Sha256::digest(data).into() }

fn packed(tree: &[Entry<'_>]) -> Vec<u8> { pack(tree, sha256).unwrap() }

fn superblock(image: &[u8]) -> Result<Superblock, Corrupt> {
    Superblock::parse(image, (image.len() / BLOCK) as u64)
}

fn inode(image: &[u8], nid: u64) -> Result<Inode, Corrupt> {
    let sb = superblock(image)?;
    let at = sb.inode_at(nid)? as usize;
    Inode::parse(&sb, nid, &image[at..image.len().min(at + EXTENDED)])
}

/// The entry `name` in the directory `dir`, block by block.
fn lookup(image: &[u8], dir: &Inode, name: &str) -> Option<u64> {
    (0..dir.dir_blocks()).find_map(|index| {
        let (at, len) = dir.dir_block(index).unwrap();
        Dirents::parse(&image[at as usize..at as usize + len]).unwrap().lookup(name.as_bytes()).map(|e| e.nid)
    })
}

/// The inode at `path`, one lookup per component.
fn find(image: &[u8], path: &str) -> Option<Inode> {
    let mut node = inode(image, superblock(image).ok()?.root).ok()?;
    for name in path.split('/') {
        node = inode(image, lookup(image, &node, name)?).ok()?;
    }
    Some(node)
}

fn data(image: &[u8], inode: &Inode) -> Vec<u8> {
    let (mut out, mut offset) = (Vec::new(), 0);
    while let Some((at, run)) = inode.extent(offset) {
        out.extend_from_slice(&image[at as usize..(at + run) as usize]);
        offset += run;
    }
    out
}

fn put(image: &mut [u8], at: usize, bytes: &[u8]) { image[at..at + bytes.len()].copy_from_slice(bytes) }

/// Where `path`'s inode starts in `image`.
fn inode_at(image: &[u8], path: &str) -> usize {
    let nid = find(image, path).unwrap().nid();
    superblock(image).unwrap().inode_at(nid).unwrap() as usize
}

/// Bytes 0, 1, 2, ... wrapping, `len` of them.
fn counting(len: usize) -> Vec<u8> { (0..len).map(|i| (i % 251) as u8).collect() }

#[test]
fn every_superblock_field_out_of_range_is_corrupt() {
    let image = packed(&[Entry::File("f", b"x")]);
    let blocks = (image.len() / BLOCK) as u64;
    assert!(Superblock::parse(&image, blocks).is_ok());
    let cases: &[(&str, usize, &[u8])] = &[
        ("the magic", 0, &[0xe3]),
        ("a block of 8 KiB", 12, &[13]),
        ("a block of 512 bytes", 12, &[9]),
        ("an incompatible feature", 80, &[1]),
        ("48-bit addresses", 80, &[0x80]),
        ("an unknown incompatible feature", 83, &[0x80]),
        ("an extra device", 86, &[1]),
        ("a directory block of another size", 90, &[1]),
        ("no blocks", 36, &[0, 0, 0, 0]),
        ("the inode area at the end", 40, &(blocks as u32).to_le_bytes()),
        ("the inode area past the end", 40, &[0xff, 0xff, 0xff, 0xff]),
        ("a root past the end", 14, &[0xff, 0xff]),
    ];
    for (what, field, bytes) in cases {
        let mut bad = image.clone();
        put(&mut bad, SUPERBLOCK_AT + field, bytes);
        assert_eq!(Superblock::parse(&bad, blocks), Err(Corrupt), "{what}");
    }
    assert_eq!(Superblock::parse(&image, blocks - 1), Err(Corrupt), "more blocks than the range");
    assert_eq!(Superblock::parse(&image[..SUPERBLOCK_AT + 100], blocks), Err(Corrupt), "a short head");
    let sb = Superblock::parse(&image, blocks).unwrap();
    assert_eq!(sb.inode_at(u64::MAX), Err(Corrupt), "an inode number that overflows");
}

#[test]
fn both_inode_sizes_parse() {
    let bytes = counting(8192);
    let mut image = packed(&[Entry::File("f", &bytes)]);
    let compact = find(&image, "f").unwrap();
    assert_eq!(
        (compact.kind(), compact.layout(), compact.size(), compact.nlink()),
        (Kind::File, Layout::Plain, 8192, 1)
    );
    assert_eq!(data(&image, &compact), bytes);

    // The same file as a 64-byte inode, with no attributes: its size and link count move.
    let at = inode_at(&image, "f");
    let mut extended = [0u8; EXTENDED];
    extended[..2].copy_from_slice(&1u16.to_le_bytes());
    extended[4..6].copy_from_slice(&0o100_644u16.to_le_bytes());
    extended[8..16].copy_from_slice(&8192u64.to_le_bytes());
    extended[16..20].copy_from_slice(&image[at + 16..at + 20]);
    extended[44..48].copy_from_slice(&3u32.to_le_bytes());
    put(&mut image, at, &extended);
    let parsed = find(&image, "f").unwrap();
    assert_eq!((parsed.layout(), parsed.size(), parsed.nlink()), (Layout::Plain, 8192, 3));
    assert_eq!(data(&image, &parsed), bytes);

    // An extended inode cut short by the bytes handed in is corrupt, not read past.
    let sb = superblock(&image).unwrap();
    assert_eq!(Inode::parse(&sb, parsed.nid(), &image[at..at + COMPACT]), Err(Corrupt));
    // 64-bit sizes are checked against the volume like any other.
    put(&mut image, at + 8, &(1u64 << 40).to_le_bytes());
    assert_eq!(inode(&image, parsed.nid()), Err(Corrupt));
}

#[test]
fn both_layouts_read_whole_and_inline_tails_stay_in_their_block() {
    let bytes = counting(10_000);
    let tree = [
        Entry::File("inline", &bytes),
        Entry::File("plain", &bytes[..8192]),
        Entry::File("small", &bytes[..5]),
        Entry::File("empty", b""),
    ];
    let mut image = packed(&tree);
    for (name, layout, size) in
        [("inline", Layout::Inline, 10_000), ("plain", Layout::Plain, 8192), ("small", Layout::Inline, 5)]
    {
        let i = find(&image, name).unwrap();
        assert_eq!((i.layout(), i.size()), (layout, size), "{name}");
        assert_eq!(data(&image, &i), &bytes[..size as usize], "{name}");
    }
    let empty = find(&image, "empty").unwrap();
    assert_eq!((empty.layout(), empty.size(), empty.extent(0)), (Layout::Plain, 0, None));

    // A tail that would cross into the next block.
    let at = inode_at(&image, "inline");
    let mut bad = image.clone();
    put(&mut bad, at + 8, &(2 * 4096 + 4095u32).to_le_bytes());
    assert_eq!(find(&bad, "inline"), None);
    // Whole blocks past the block count, and a first block whose run would overflow 32 bits.
    for start in [superblock(&image).unwrap().blocks - 1, u32::MAX - 1] {
        let mut bad = image.clone();
        put(&mut bad, inode_at(&image, "plain") + 16, &start.to_le_bytes());
        assert_eq!(find(&bad, "plain"), None, "start {start}");
    }
    // Every other layout: the compressed ones, the chunked one, the reserved ones.
    for layout in [1u16, 3, 4, 5, 6, 7] {
        let mut bad = image.clone();
        put(&mut bad, at, &(layout << 1).to_le_bytes());
        assert_eq!(find(&bad, "inline"), None, "layout {layout}");
    }
    // A format bit the subset does not know, and anything but a file or a directory.
    let mut bad = image.clone();
    put(&mut bad, at, &(2u16 << 1 | 1 << 5).to_le_bytes());
    assert_eq!(find(&bad, "inline"), None);
    for mode in [0o120_777u16, 0o020_644, 0o060_644, 0o010_644, 0o140_644, 0] {
        let mut bad = image.clone();
        put(&mut bad, at + 4, &mode.to_le_bytes());
        assert_eq!(find(&bad, "inline"), None, "mode {mode:o}");
    }
    // Bit 4 on a compact file: one link, whatever the field says.
    put(&mut image, at, &(2u16 << 1 | 1 << 4).to_le_bytes());
    put(&mut image, at + 6, &7u16.to_le_bytes());
    assert_eq!(find(&image, "inline").unwrap().nlink(), 1);
}

#[test]
fn extended_attributes_are_sized_and_bounded_never_read() {
    let bytes = counting(5000);
    let mut image = packed(&[Entry::File("f", &bytes)]);
    let at = inode_at(&image, "f");
    // The writer's attribute: `user.sha256`, the file's digest, before the tail.
    assert_eq!(u16::from_le_bytes([image[at + 2], image[at + 3]]), 12);
    let entry = &image[at + COMPACT + 12..at + COMPACT + 56];
    assert_eq!(&entry[..4], &[6, 1, 32, 0]);
    assert_eq!(&entry[4..10], b"sha256");
    assert_eq!(&entry[10..42], &sha256(&bytes));
    // Its content is never read: noise there changes nothing.
    image[at + COMPACT..at + COMPACT + 56].fill(0xa5);
    assert_eq!(data(&image, &find(&image, "f").unwrap()), bytes);
    // A count that moves the tail along inside its block only shifts what is read; one that
    // pushes it across the block, or the area past the volume's end, is corrupt.
    let moved = |count: u16| {
        let mut moved = image.clone();
        put(&mut moved, at + 2, &count.to_le_bytes());
        find(&moved, "f")
    };
    assert_eq!(moved(13).map(|i| i.size()), Some(5000));
    assert!((at + COMPACT + 12 + 4 * 599) % BLOCK + 5000 % BLOCK > BLOCK);
    assert_eq!(moved(600), None);
    assert_eq!(moved(0xffff), None);
}

/// A well-formed directory block of `names` (in the order given), each naming inode 7.
fn dir_block(names: &[&[u8]]) -> Vec<u8> {
    let mut block = vec![0u8; names.len() * DIRENT];
    let mut name_at = block.len();
    for (i, name) in names.iter().enumerate() {
        block[i * DIRENT..i * DIRENT + 8].copy_from_slice(&7u64.to_le_bytes());
        block[i * DIRENT + 8..i * DIRENT + 10].copy_from_slice(&(name_at as u16).to_le_bytes());
        block[i * DIRENT + 10] = 1;
        name_at += name.len();
    }
    for name in names {
        block.extend_from_slice(name);
    }
    block
}

#[test]
fn directory_blocks_are_counted_bounded_and_ordered() {
    let good = dir_block(&[b".", b"..", b"a", b"ab", b"b"]);
    let d = Dirents::parse(&good).unwrap();
    assert_eq!(d.len(), 5);
    let names: Vec<&[u8]> = d.iter().map(|e| e.name).collect();
    assert_eq!(names, [&b"."[..], b"..", b"a", b"ab", b"b"]);
    for name in [&b"."[..], b"..", b"a", b"ab", b"b"] {
        assert_eq!(d.lookup(name).map(|e| (e.name, e.nid, e.file_type)), Some((name, 7, 1)));
    }
    for absent in [&b""[..], b"0", b"aa", b"abc", b"c", b"\xff"] {
        assert_eq!(d.lookup(absent), None);
    }
    // The last name ends at the block's end or its first NUL.
    let mut padded = good.clone();
    padded.extend_from_slice(&[0, 0, 0]);
    assert_eq!(Dirents::parse(&padded).unwrap().get(4).unwrap().name, b"b");

    let bad = |what: &str, block: Vec<u8>| assert!(Dirents::parse(&block).is_err(), "{what}");
    let with = |at: usize, bytes: &[u8]| {
        let mut block = good.clone();
        put(&mut block, at, bytes);
        block
    };
    bad("no entries", with(8, &0u16.to_le_bytes()));
    bad("a count that is not whole entries", with(8, &13u16.to_le_bytes()));
    bad("names starting past the block", with(8, &(good.len() as u16 + 1).to_le_bytes()));
    bad("a last name that is empty", dir_block(&[b"a", b""]));
    bad("a name inside the entries", with(DIRENT + 8, &30u16.to_le_bytes()));
    bad("offsets going back", with(3 * DIRENT + 8, &61u16.to_le_bytes()));
    bad("a name past the block", with(4 * DIRENT + 8, &(good.len() as u16 + 1).to_le_bytes()));
    bad("names out of order", dir_block(&[b"a", b"c", b"b"]));
    bad("a name twice", dir_block(&[b"a", b"b", b"b"]));
    bad("a slash", dir_block(&[b"a", b"b/c"]));
    bad("a NUL inside a name", dir_block(&[b"a\0b", b"c"]));
    bad("a name too long", dir_block(&[&[b'a'; NAME_MAX + 1]]));
    bad("a block too long", {
        let mut long = good.clone();
        long.resize(BLOCK + 1, 0);
        long
    });
    bad("a block too short", good[..9].to_vec());
    assert!(Dirents::parse(&dir_block(&[&[b'a'; NAME_MAX]])).is_ok());
}

/// xorshift64: the same tree for the same seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

/// A random tree: names from an alphabet that sorts around `.` and `..`, some long enough to
/// spread a directory over several blocks; sizes at and around the block's edges.
fn random_tree(rng: &mut Rng) -> Tree {
    const ALPHABET: &[u8] = b"+-.0189AZaz_~";
    fn fill(rng: &mut Rng, dir: &str, depth: u32, out: &mut Tree) {
        let mut names: Vec<String> = Vec::new();
        let count = if depth == 0 { 150 + rng.below(100) } else { rng.below(30) };
        while (names.len() as u64) < count {
            let len = if rng.below(8) == 0 { 200 + rng.below(56) } else { 1 + rng.below(12) };
            let name: String =
                (0..len).map(|_| ALPHABET[rng.below(ALPHABET.len() as u64) as usize] as char).collect();
            if name != "." && name != ".." && !names.contains(&name) {
                names.push(name);
            }
        }
        for name in names {
            let path = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if depth < 3 && rng.below(6) == 0 {
                out.push((path.clone(), None));
                fill(rng, &path, depth + 1, out);
            } else {
                let size = [0, 1, 4095, 4096, 4097, 8192, 4096 - 120, 4096 - 64][rng.below(8) as usize];
                let size = if rng.below(3) == 0 { rng.below(20_000) as usize } else { size };
                let bytes = (0..size).map(|_| rng.next() as u8).collect();
                out.push((path, Some(bytes)));
            }
        }
    }
    let mut out = Vec::new();
    fill(rng, "", 0, &mut out);
    out
}

fn entries(tree: &[(String, Option<Vec<u8>>)]) -> Vec<Entry<'_>> {
    tree.iter()
        .map(|(path, data)| match data {
            Some(data) => Entry::File(path, data),
            None => Entry::Dir(path),
        })
        .collect()
}

#[test]
fn lookup_and_iteration_agree_with_the_packed_tree() {
    for seed in 1..=6 {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ seed);
        let tree = random_tree(&mut rng);
        let image = packed(&entries(&tree));
        let root = inode(&image, superblock(&image).unwrap().root).unwrap();
        assert!(root.dir_blocks() > 1, "seed {seed}: the root spans several blocks");
        for (path, bytes) in &tree {
            let node = find(&image, path).unwrap_or_else(|| panic!("seed {seed}: {path} not found"));
            match bytes {
                Some(bytes) => assert_eq!(&data(&image, &node), bytes, "seed {seed}: {path}"),
                None => assert_eq!(node.kind(), Kind::Dir),
            }
            assert_eq!(find(&image, &format!("{path}.absent")), None);
        }
        // Read back whole, in each directory's order: the same tree.
        let mut expected = tree.clone();
        let mut read = read_tree(&image).unwrap();
        expected.sort();
        read.sort();
        assert_eq!(read, expected, "seed {seed}");
        // Each directory lists its entries in order across its blocks, `.` and `..` among them.
        let mut names = Vec::new();
        for index in 0..root.dir_blocks() {
            let (at, len) = root.dir_block(index).unwrap();
            names.extend(
                Dirents::parse(&image[at as usize..at as usize + len]).unwrap().iter().map(|e| e.name),
            );
        }
        assert!(names.windows(2).all(|w| w[0] < w[1]), "seed {seed}");
        assert!(names.contains(&&b"."[..]) && names.contains(&&b".."[..]));
    }
}

#[test]
fn two_packs_of_one_tree_are_the_same_bytes() {
    let tree = random_tree(&mut Rng(42));
    assert_eq!(packed(&entries(&tree)), packed(&entries(&tree)));
}

#[test]
fn the_writer_refuses_what_it_cannot_name() {
    let long = "x".repeat(NAME_MAX + 1);
    let cases: &[(&[Entry<'_>], &str)] = &[
        (&[Entry::File("", b"")], "not a name"),
        (&[Entry::File(".", b"")], "not a name"),
        (&[Entry::Dir("..")], "not a name"),
        (&[Entry::Dir("a"), Entry::File("a/", b"")], "not a name"),
        (&[Entry::File(&long, b"")], "not a name"),
        (&[Entry::File("a\0b", b"")], "not a name"),
        (&[Entry::File("/a", b"")], "no directory"),
        (&[Entry::File("a/b", b"")], "no directory"),
        (&[Entry::File("a", b""), Entry::File("a/b", b"")], "no directory"),
        (&[Entry::Dir("a"), Entry::File("a//b", b"")], "no directory"),
        (&[Entry::File("a", b""), Entry::Dir("a")], "named twice"),
    ];
    for (tree, why) in cases {
        assert_eq!(pack(tree, sha256).map(|_| ()).map_err(|e| e.why), Err(*why), "{tree:?}");
    }
    assert!(pack(&[Entry::File(&long[..NAME_MAX], b"")], sha256).is_ok());
}
