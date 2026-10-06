//! Hostile volumes: noise, a valid volume with any bit of any block flipped, and forged
//! structures whose hashes are made to agree, so they reach the parser. Each must be refused as
//! corrupt or found by the check, never panic, hang, or be read as something else.

mod common;

use common::exercise::{READ_CAP, exercise};
use common::forge::{rehash, rehash_at, seal};
use common::*;
use sha2::Digest;
use walfs::{BLOCK, Error, Filesystem, OpenOptions, Problem};

/// A populated volume of 512 blocks: files of every size up to 60,000 bytes, one sparse file past
/// the single-indirect blocks.
fn populated() -> Vec<u8> { common::populated(512, 10, 60_000, 4_300_000) }

fn u32_at(img: &[u8], at: usize) -> u32 { u32::from_le_bytes(img[at..at + 4].try_into().unwrap()) }

fn put_u32(img: &mut [u8], at: usize, v: u32) { img[at..at + 4].copy_from_slice(&v.to_le_bytes()) }

/// The superblock's fields this test needs: the regions' first blocks.
struct Regions {
    attr: u32,
    hash: u32,
    bitmap: u32,
    data: u32,
}

fn regions(img: &[u8]) -> Regions {
    Regions { attr: u32_at(img, 36), hash: u32_at(img, 40), bitmap: u32_at(img, 44), data: u32_at(img, 48) }
}

fn block(img: &mut [u8], b: u32) -> &mut [u8] { &mut img[b as usize * BLOCK..(b as usize + 1) * BLOCK] }

/// Changes block `b` with `f` and forges its hash.
fn forge(img: &mut [u8], b: u32, f: impl FnOnce(&mut [u8])) {
    f(block(img, b));
    rehash(img, b);
}

/// Inode `i`'s block and offset.
fn inode_at(i: u32) -> (u32, usize) { (34 + i / 32, (i % 32) as usize * 128) }

/// The inode a path names on `img`.
fn ino(img: &[u8], path: &str) -> u32 {
    let mut ram = Ram::from_image(img.to_vec());
    Filesystem::mount(&mut ram).unwrap().stat(path).unwrap().inode
}

/// The first data block of the file at `path`, and the block holding its first entry name.
fn first_block(img: &[u8], path: &str) -> u32 {
    let (b, at) = inode_at(ino(img, path));
    u32_at(img, b as usize * BLOCK + at + 32)
}

/// What the bounded walk reads of `img`.
fn walked(img: &[u8]) -> Tree {
    let mut ram = Ram::from_image(img.to_vec());
    walk(&mut Filesystem::mount(&mut ram).unwrap(), READ_CAP).unwrap()
}

/// Mounts `img`; if it mounts, walks it and changes it. Returns what the mount and the walk gave.
fn drive(img: Vec<u8>) -> Result<Tree, Error> {
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram)?;
    let seen = walk(&mut fs, READ_CAP);
    exercise(&mut fs);
    seen
}

#[test]
fn noise_never_panics() {
    let good = populated();
    for seed in 1..200u64 {
        let mut rng = Rng(seed);
        let blocks = 40 + rng.below(300) as usize;
        assert_eq!(drive(rng.bytes(blocks * BLOCK)).err(), Some(Error::Corrupt), "seed {seed}");
        // The real superblock, and noise behind it.
        let mut img = rng.bytes(512 * BLOCK);
        img[..BLOCK].copy_from_slice(&good[..BLOCK]);
        assert_eq!(drive(img).err(), Some(Error::Corrupt), "seed {seed} behind a superblock");
    }
}

/// Any bit of any block flipped: the volume reads as it was, or what reads the block is refused as
/// corrupt; a block in use is always found, by the mount, the walk or the check.
#[test]
fn every_flipped_bit_is_corrupt_where_it_is_read() {
    let good = populated();
    let tree = walked(&good);
    let r = regions(&good);
    let mut bitmap = vec![0u8; BLOCK];
    bitmap.copy_from_slice(&good[r.bitmap as usize * BLOCK..(r.bitmap as usize + 1) * BLOCK]);
    // The superblock, the tables, the hash region (each block checks itself), the bitmap and data
    // in use must be found; the log and free blocks must not change anything.
    let in_use = |b: u32| {
        b == 0 || (34..r.bitmap).contains(&b) || bitmap[b as usize / 8] & (1 << (b % 8)) != 0 && b >= r.bitmap
    };
    let mut found = 0;
    for b in 0..512u32 {
        for bit in [0usize, 7, 4095 * 8 + 3, (b as usize * 997) % (BLOCK * 8)] {
            let mut img = good.clone();
            img[b as usize * BLOCK + bit / 8] ^= 1 << (bit % 8);
            let what = format!("block {b} bit {bit}");
            let mut ram = Ram::from_image(img.clone());
            let seen = Filesystem::mount(&mut ram).and_then(|mut fs| {
                let seen = walk(&mut fs, READ_CAP)?;
                let problems = fs.check()?;
                Ok((seen, problems))
            });
            match seen {
                Err(Error::Corrupt) => found += 1,
                Ok((seen, problems)) => {
                    assert!(seen == tree, "{what}: read as something else: {:#?}", tree_diff(&seen, &tree));
                    if in_use(b) {
                        assert!(!problems.is_empty(), "{what}: a block in use, flipped, went unseen");
                        found += 1;
                    } else {
                        assert!(problems.is_empty(), "{what}: {problems:?}");
                    }
                }
                Err(e) => panic!("{what}: {e:?}"),
            }
            let _ = drive(img);
        }
    }
    let live = (0..512).filter(|&b| in_use(b)).count();
    assert!(found >= 4 * live && live > 60, "{found} flips found, {live} blocks in use");
}

#[test]
fn every_superblock_field_out_of_range_is_corrupt() {
    let good = populated();
    let hash = regions(&good).hash;
    for at in (8..56).step_by(4) {
        let v = u32_at(&good, at);
        for bad in [0, 1, v.wrapping_sub(1), v + 1, u32::MAX] {
            if bad == v {
                continue;
            }
            let mut img = good.clone();
            put_u32(&mut img, at, bad);
            seal(&mut img, 0);
            rehash_at(&mut img, 0, hash);
            assert_eq!(drive(img).err(), Some(Error::Corrupt), "field at {at} = {bad}");
        }
    }
    let mut img = good.clone();
    block(&mut img, 0)[60] = 1;
    seal(&mut img, 0);
    rehash(&mut img, 0);
    assert_eq!(drive(img).err(), Some(Error::Corrupt), "a reserved byte");
    // The device is not the superblock's size.
    let mut img = good.clone();
    img.extend_from_slice(&[0; BLOCK]);
    assert_eq!(drive(img).err(), Some(Error::Corrupt), "a larger device");
}

/// A committed header that names the superblock, the log, a block twice, or blocks that are not
/// what it hashed, or that holds more than `LOG_BLOCKS`: corrupt, and nothing is copied home.
#[test]
fn a_forged_log_header_is_corrupt() {
    let good = populated();
    let tree = walked(&good);
    let data = regions(&good).data;
    let header = |entries: &[(u32, [u8; 32])], count: u32| {
        let mut img = good.clone();
        let blk = block(&mut img, 1);
        blk.fill(0);
        blk[..8].copy_from_slice(b"walfslog");
        put_u32(blk, 8, 1);
        put_u32(blk, 12, count);
        for (k, (home, hash)) in entries.iter().enumerate() {
            put_u32(blk, 16 + 36 * k, *home);
            blk[20 + 36 * k..52 + 36 * k].copy_from_slice(hash);
        }
        seal(&mut img, 1);
        img
    };
    let zero: [u8; 32] = sha2::Sha256::digest([0u8; BLOCK]).into();
    for (what, img) in [
        ("the superblock", header(&[(0, zero)], 1)),
        ("a log block", header(&[(5, zero)], 1)),
        ("a block twice", header(&[(data, zero), (data, zero)], 2)),
        ("a block past the volume", header(&[(512, zero)], 1)),
        ("more than LOG_BLOCKS", header(&[(data, zero)], 33)),
        ("a logged block that is not what was hashed", header(&[(data, [1; 32])], 1)),
        ("count 0, committed", header(&[], 0)),
    ] {
        assert_eq!(drive(img).err(), Some(Error::Corrupt), "{what}");
    }
    // A header whose own hash fails is a torn one: dropped, and the volume as it was.
    let mut img = header(&[(data, [1; 32])], 1);
    block(&mut img, 1)[100] ^= 1;
    assert_eq!(drive(img), Ok(tree));
}

/// Each inode field out of range, with its hash forged: the inode is refused where it is read.
#[test]
fn every_inode_field_out_of_range_is_corrupt() {
    let good = populated();
    let f = ino(&good, "/d/f5");
    let (b, at) = inode_at(f);
    let edits: Vec<(&str, Box<dyn Fn(&mut [u8])>)> = vec![
        ("kind 3", Box::new(|i| i[0] = 3)),
        ("nlink 2", Box::new(|i| i[2] = 2)),
        ("next past the inodes", Box::new(|i| put_u32(i, 4, 1 << 20))),
        ("size past the largest file", Box::new(|i| i[8..16].copy_from_slice(&u64::MAX.to_le_bytes()))),
        ("a direct block in the inode table", Box::new(|i| put_u32(i, 32, 34))),
        ("a direct block past the volume", Box::new(|i| put_u32(i, 36, 512))),
        ("a single-indirect block that is the superblock's hash block", Box::new(|i| put_u32(i, 80, 40))),
        ("a reserved byte", Box::new(|i| i[100] = 1)),
    ];
    for (what, edit) in edits {
        let mut img = good.clone();
        forge(&mut img, b, |blk| edit(&mut blk[at..at + 128]));
        let mut ram = Ram::from_image(img.clone());
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert_eq!(fs.stat("/d/f5").err(), Some(Error::Corrupt), "{what}");
        assert!(fs.check().unwrap().contains(&Problem::BadInode(f)), "{what}");
        drop(fs);
        let _ = drive(img);
    }
    // A directory whose size is not whole blocks.
    let d = ino(&good, "/d");
    let (b, at) = inode_at(d);
    let mut img = good.clone();
    forge(&mut img, b, |blk| blk[at + 8] = 1);
    let mut ram = Ram::from_image(img);
    assert_eq!(Filesystem::mount(&mut ram).unwrap().stat("/d/f5").err(), Some(Error::Corrupt));
}

/// Directory entries the page does not allow: a name with `/`, `..`, a NUL, bytes past its
/// length, an inode past the table, a slot half free, and a name twice.
#[test]
fn every_bad_directory_entry_is_corrupt() {
    let good = populated();
    let blk = first_block(&good, "/d");
    let name_len = good[blk as usize * BLOCK + 4] as usize;
    let edits: Vec<(&str, Box<dyn Fn(&mut [u8])>)> = vec![
        ("a slash", Box::new(|e| e[5] = b'/')),
        ("a NUL", Box::new(|e| e[5] = 0)),
        (
            "dot dot",
            Box::new(|e| {
                e[4] = 2;
                e[5..7].copy_from_slice(b"..");
                e[7..260].fill(0);
            }),
        ),
        ("a byte past the name", Box::new(move |e| e[5 + name_len] = b'x')),
        ("an inode past the table", Box::new(|e| put_u32(e, 0, 1 << 20))),
        ("a free slot with a length", Box::new(|e| put_u32(e, 0, 0))),
        ("a length of 0", Box::new(|e| e[4] = 0)),
        ("the block's tail", Box::new(|e| e[3900 + 10] = 1)),
    ];
    for (what, edit) in edits {
        let mut img = good.clone();
        forge(&mut img, blk, |b| edit(b));
        let mut ram = Ram::from_image(img.clone());
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert_eq!(fs.read_dir("/d", |_| {}).err(), Some(Error::Corrupt), "{what}");
        assert!(fs.check().unwrap().contains(&Problem::BadBlock(blk)), "{what}");
        drop(fs);
        let _ = drive(img);
    }
    // The second entry given the first's name: a lookup of it is corrupt.
    let mut img = good.clone();
    let first: Vec<u8> = good[blk as usize * BLOCK..blk as usize * BLOCK + 260][4..].to_vec();
    forge(&mut img, blk, |b| b[264..520].copy_from_slice(&first));
    let name = String::from_utf8(first[1..1 + first[0] as usize].to_vec()).unwrap();
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert_eq!(fs.stat(&format!("/d/{name}")).err(), Some(Error::Corrupt));
    assert!(fs.check().unwrap().contains(&Problem::DuplicateName(ino(&good, "/d"))));
}

/// A directory that names the root, or a block two files share: the check names them, and nothing
/// that walks or changes the volume hangs or panics.
#[test]
fn cycles_and_shared_blocks_are_found() {
    let good = populated();
    let blk = first_block(&good, "/d/e");
    let mut img = good.clone();
    forge(&mut img, blk, |b| put_u32(b, 0, walfs::ROOT));
    let mut ram = Ram::from_image(img.clone());
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert_eq!(walk(&mut fs, READ_CAP).err(), Some(Error::Corrupt));
    assert!(fs.check().unwrap().contains(&Problem::NamedTwice(walfs::ROOT)));
    drop(fs);
    let _ = drive(img);

    let b = ino(&good, "/d/f5");
    let shared = first_block(&good, "/d/f4");
    let mut img = good.clone();
    let (ib, at) = inode_at(b);
    forge(&mut img, ib, |blk| put_u32(blk, at + 32, shared));
    let mut ram = Ram::from_image(img.clone());
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert!(fs.check().unwrap().contains(&Problem::SharedBlock(shared)));
    fs.remove("/d/f4").unwrap();
    assert_eq!(fs.remove("/d/f5"), Err(Error::Corrupt), "its block is already free");
    drop(fs);
    let _ = drive(img);
}

/// An orphan list that loops, or runs through a free inode: the mount refuses it or ends, and never
/// hangs.
#[test]
fn a_forged_orphan_list_ends() {
    let good = populated();
    let (x, y) = (ino(&good, "/d/f4"), ino(&good, "/d/f5"));
    for (what, links) in [("a loop", vec![(0, x), (x, y), (y, x)]), ("a free inode", vec![(0, 31)])] {
        let mut img = good.clone();
        for (from, to) in links {
            let (b, at) = inode_at(from);
            forge(&mut img, b, |blk| put_u32(blk, at + 4, to));
        }
        let r = drive(img);
        assert!(matches!(r, Ok(_) | Err(Error::Corrupt)), "{what}: {r:?}");
    }
}

/// A forged size the blocks do not back reads as holes, bounded by what the reader asks for.
#[test]
fn a_forged_size_reads_as_holes_and_allocates_nothing() {
    let good = populated();
    let (b, at) = inode_at(ino(&good, "/d/e/g"));
    let mut img = good.clone();
    forge(&mut img, b, |blk| blk[at + 8..at + 16].copy_from_slice(&walfs::MAX_FILE_SIZE.to_le_bytes()));
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert_eq!(fs.stat("/d/e/g").unwrap().size, walfs::MAX_FILE_SIZE);
    let h = fs.open("/d/e/g", OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
    fs.seek(h, walfs::MAX_FILE_SIZE - 10).unwrap();
    let mut buf = [1u8; 64];
    assert_eq!(fs.read(h, &mut buf).unwrap(), 10);
    assert_eq!(buf[..10], [0; 10]);
    fs.truncate(h, 0).unwrap();
    fs.close(h).unwrap();
    assert!(fs.check().unwrap().is_empty());
}

/// Attribute areas the page does not allow: a record past the area, a type twice, bytes after the
/// end, and attributes on a free inode.
#[test]
fn every_bad_attribute_area_is_corrupt() {
    let good = populated();
    let f = ino(&good, "/d/f1");
    let attr = regions(&good).attr;
    let (b, at) = (attr + f / 16, (f % 16) as usize * 256);
    let edits: Vec<(&str, Box<dyn Fn(&mut [u8])>)> = vec![
        ("a record past the area", Box::new(|a| a[1] = 255)),
        ("a type twice", Box::new(|a| a[6..12].copy_from_slice(&[1, 4, b'a', b't', b't', b'r']))),
        ("a byte after the end", Box::new(|a| a[200] = 1)),
    ];
    for (what, edit) in edits {
        let mut img = good.clone();
        forge(&mut img, b, |blk| edit(&mut blk[at..at + 256]));
        let mut ram = Ram::from_image(img);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert_eq!(fs.get_attr("/d/f1", 1).err(), Some(Error::Corrupt), "{what}");
        assert!(fs.check().unwrap().contains(&Problem::BadAttrs(f)), "{what}");
    }
    let mut img = good.clone();
    // Inode 31, the table's last, is free.
    forge(&mut img, attr + 31 / 16, |blk| blk[(31 % 16) * 256] = 9);
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert!(fs.check().unwrap().contains(&Problem::BadAttrs(31)));
}

/// A damaged hash block fails its own hash: a read through it is refused as corrupt (the first
/// holds the superblock's slot, so the mount is), and the check names it, not the blocks whose
/// slots it holds.
#[test]
fn a_damaged_hash_block_is_refused_as_itself() {
    let good = populated();
    let hash = regions(&good).hash;
    let mut img = good.clone();
    block(&mut img, hash)[17] ^= 4;
    let mut ram = Ram::from_image(img);
    // The first hash block holds the superblock's slot, which the mount reads.
    assert_eq!(Filesystem::mount(&mut ram).err(), Some(Error::Corrupt));
    let last = regions(&good).bitmap - 1;
    let mut img = good.clone();
    block(&mut img, last)[17] ^= 4;
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let problems = fs.check().unwrap();
    assert!(problems.contains(&Problem::BadBlock(last)), "{problems:?}");
    assert!(problems.iter().all(|p| *p == Problem::BadBlock(last)), "{problems:?}");
}

/// An indirect block naming a block outside the data region: the read through it is refused, and
/// the check names the indirect block.
#[test]
fn an_indirect_entry_outside_the_data_region_is_corrupt() {
    let good = populated();
    // f9, of 54,001 bytes, has blocks past the 12 direct ones.
    let (b, at) = inode_at(ino(&good, "/d/f9"));
    let single = u32_at(&good, b as usize * BLOCK + at + 80);
    assert_ne!(single, 0);
    let mut img = good.clone();
    forge(&mut img, single, |blk| put_u32(blk, 0, 5));
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert_eq!(read_file(&mut fs, "/d/f9").err(), Some(Error::Corrupt));
    assert!(fs.check().unwrap().contains(&Problem::BadBlock(single)));
}

/// A bit of the bitmap that must be 1 (a block before the data region) set to 0: the mount refuses
/// the volume.
#[test]
fn a_reserved_bitmap_bit_clear_fails_the_mount() {
    let good = populated();
    let mut img = good.clone();
    let bitmap = regions(&good).bitmap;
    forge(&mut img, bitmap, |blk| blk[0] &= !1);
    let mut ram = Ram::from_image(img);
    assert_eq!(Filesystem::mount(&mut ram).err(), Some(Error::Corrupt));
}

/// A directory whose size is the largest file's, forged: a listing or a lookup in it is refused at
/// its first hole, after reading only the blocks it has.
#[test]
fn a_directory_sized_as_the_largest_file_is_corrupt_at_its_first_hole() {
    let good = populated();
    let (b, at) = inode_at(ino(&good, "/d"));
    let mut img = good.clone();
    forge(&mut img, b, |blk| blk[at + 8..at + 16].copy_from_slice(&walfs::MAX_FILE_SIZE.to_le_bytes()));
    let mut ram = Ram::from_image(img);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let mut listed = 0;
    assert_eq!(fs.read_dir("/d", |_| listed += 1).err(), Some(Error::Corrupt));
    assert!(listed <= 15, "{listed} entries before the first hole");
    assert_eq!(fs.stat("/d/f5").err(), Some(Error::Corrupt));
    assert!(fs.check().unwrap().contains(&Problem::DirectoryHole(ino(&good, "/d"))));
}
