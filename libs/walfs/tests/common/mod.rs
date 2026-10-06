//! Shared test helpers: the RAM block device the tests use (with a write cache that power loss
//! empties in any order), a deterministic RNG, and a dump of a whole volume for comparisons.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

pub mod ops;

use walfs::{BLOCK, Block, BlockDevice, Error, FileType, Filesystem, Geometry, OpenOptions};

/// A sector: the unit a torn write lands in.
const SECTOR: usize = 512;

/// A RAM disk. Reads see every write; a write is durable only once a `sync` follows it.
///
/// `budget`: writes allowed. The last one is the power cut: it and every write since the last
/// `sync` each land whole, not at all, or as a prefix of sectors, chosen at random, and nothing
/// works after it.
pub struct Ram {
    pub data: Vec<u8>,
    pub budget: Option<u64>,
    pub writes: u64,
    pub syncs: u64,
    /// Blocks written since the last sync, with what they held before.
    pending: Vec<(u32, Vec<u8>)>,
    pub cut: bool,
    pub rng: Rng,
}

impl Ram {
    pub fn new(blocks: u32) -> Ram { Ram::from_image(vec![0; blocks as usize * BLOCK]) }

    pub fn from_image(data: Vec<u8>) -> Ram {
        Ram { data, budget: None, writes: 0, syncs: 0, pending: Vec::new(), cut: false, rng: Rng(1) }
    }

    /// Power fails at write number `n` (1-based); `seed` drives what lands.
    pub fn failing_at(mut self, n: u64, seed: u64) -> Ram {
        self.budget = Some(n);
        self.rng = Rng(seed | 1);
        self
    }

    /// A formatted volume of `blocks` blocks.
    pub fn formatted(blocks: u32) -> Ram {
        let mut ram = Ram::new(blocks);
        Filesystem::format(&mut ram, Geometry::for_blocks(blocks)).unwrap();
        ram
    }

    fn block(&mut self, b: u32) -> &mut [u8] { &mut self.data[b as usize * BLOCK..(b as usize + 1) * BLOCK] }

    /// The power cut: each write since the last sync lands whole, not at all, or torn.
    fn power_off(&mut self) {
        self.cut = true;
        for (b, old) in std::mem::take(&mut self.pending) {
            let new = self.block(b).to_vec();
            let landed = match self.rng.below(3) {
                0 => old,
                1 => new,
                _ => {
                    let k = self.rng.below((BLOCK / SECTOR) as u64 + 1) as usize * SECTOR;
                    [&new[..k], &old[k..]].concat()
                }
            };
            self.block(b).copy_from_slice(&landed);
        }
    }
}

impl BlockDevice for Ram {
    fn block_count(&self) -> u32 { (self.data.len() / BLOCK) as u32 }

    fn read(&mut self, block: u32, buf: &mut Block) -> Result<(), Error> {
        assert!(block < self.block_count(), "read past the device");
        if self.cut {
            return Err(Error::Io);
        }
        buf.copy_from_slice(self.block(block));
        Ok(())
    }

    fn write(&mut self, block: u32, data: &Block) -> Result<(), Error> {
        assert!(block < self.block_count(), "write past the device");
        if self.cut {
            return Err(Error::Io);
        }
        self.writes += 1;
        if !self.pending.iter().any(|p| p.0 == block) {
            let old = self.block(block).to_vec();
            self.pending.push((block, old));
        }
        self.block(block).copy_from_slice(data);
        if self.budget == Some(self.writes) {
            self.power_off();
            return Err(Error::Io);
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> {
        if self.cut {
            return Err(Error::Io);
        }
        self.syncs += 1;
        self.pending.clear();
        Ok(())
    }
}

/// xorshift64*: deterministic and dependency-free.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: u64) -> u64 { self.next() % n }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> { (0..n).map(|_| self.next() as u8).collect() }
}

/// What a volume holds: path -> node. Attribute types checked: `ATTR_TYPES`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Dir { attrs: BTreeMap<u8, Vec<u8>> },
    File { data: Vec<u8>, attrs: BTreeMap<u8, Vec<u8>> },
}

pub type Tree = BTreeMap<String, Node>;

pub const ATTR_TYPES: [u8; 3] = [1, 2, 0x74];

pub fn empty_tree() -> Tree {
    let mut t = Tree::new();
    t.insert(String::new(), Node::Dir { attrs: BTreeMap::new() });
    t
}

pub fn read_file<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str) -> Result<Vec<u8>, Error> {
    read_upto(fs, path, usize::MAX)
}

/// The file at `path`, read up to `cap` bytes.
pub fn read_upto<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str, cap: usize) -> Result<Vec<u8>, Error> {
    let h = fs.open(path, OpenOptions { read: true, ..Default::default() })?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 20_000];
    let read = loop {
        let want = buf.len().min(cap - out.len());
        match fs.read(h, &mut buf[..want]) {
            Ok(0) => break Ok(()),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) => break Err(e),
        }
    };
    fs.close(h)?;
    read.map(|()| out)
}

pub fn write_file<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str, data: &[u8]) -> Result<(), Error> {
    let h = fs.open(path, OpenOptions { write: true, create: true, truncate: true, ..Default::default() })?;
    let r = match fs.write(h, data) {
        Ok(n) if n < data.len() => Err(Error::NoSpace),
        r => r.map(|_| ()),
    };
    let c = fs.close(h);
    r.and(c)
}

/// The attributes of the types the tests use.
pub fn attrs<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str) -> Result<BTreeMap<u8, Vec<u8>>, Error> {
    let mut m = BTreeMap::new();
    for t in ATTR_TYPES {
        match fs.get_attr(path, t) {
            Ok(v) => {
                m.insert(t, v);
            }
            Err(Error::NoAttr) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(m)
}

/// The paths where two trees differ, with a short description of each side.
pub fn tree_diff(a: &Tree, b: &Tree) -> Vec<String> {
    let brief = |n: Option<&Node>| match n {
        None => "absent".to_string(),
        Some(Node::Dir { attrs }) => format!("dir, attrs {:?}", attrs.keys().collect::<Vec<_>>()),
        Some(Node::File { data, attrs }) => {
            format!("file of {} bytes, attrs {:?}", data.len(), attrs.keys().collect::<Vec<_>>())
        }
    };
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| format!("{k}: {} vs {}", brief(a.get(k)), brief(b.get(k))))
        .collect()
}

/// Reads everything in the volume.
pub fn dump<D: BlockDevice>(fs: &mut Filesystem<D>) -> Result<Tree, Error> { walk(fs, usize::MAX) }

/// Reads everything in the volume, each file up to `cap` bytes (a forged size must not make a walk
/// read gigabytes of holes); an inode met twice is a cycle, refused as corrupt.
pub fn walk<D: BlockDevice>(fs: &mut Filesystem<D>, cap: usize) -> Result<Tree, Error> {
    let mut tree = Tree::new();
    tree.insert(String::new(), Node::Dir { attrs: attrs(fs, "/")? });
    let mut seen = BTreeSet::from([walfs::ROOT]);
    let mut todo = vec![String::new()];
    while let Some(dir) = todo.pop() {
        let mut entries = Vec::new();
        fs.read_dir(&format!("{dir}/"), |e| {
            entries.push((
                String::from_utf8_lossy(e.name).into_owned(),
                e.meta.kind,
                e.meta.size,
                e.meta.inode,
            ))
        })?;
        for (name, kind, size, inode) in entries {
            if !seen.insert(inode) {
                return Err(Error::Corrupt);
            }
            let path = format!("{dir}/{name}");
            let attrs = attrs(fs, &path)?;
            let node = match kind {
                FileType::Dir => {
                    todo.push(path.clone());
                    Node::Dir { attrs }
                }
                FileType::File => {
                    let data = read_upto(fs, &path, cap)?;
                    assert_eq!(data.len() as u64, size.min(cap as u64), "stat size of {path}");
                    Node::File { data, attrs }
                }
            };
            tree.insert(path, node);
        }
    }
    Ok(tree)
}

/// A volume of `blocks` blocks holding nested directories, `files` files of up to `largest` bytes,
/// a sparse file with a block at `far`, attributes on a file and on the root, a rename and a
/// removal; checked sound.
pub fn populated(blocks: u32, files: usize, largest: usize, far: u64) -> Vec<u8> {
    let mut ram = Ram::formatted(blocks);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let mut rng = Rng(3);
    fs.mkdir("/d").unwrap();
    fs.mkdir("/d/e").unwrap();
    for i in 0..files {
        write_file(&mut fs, &format!("/d/f{i}"), &rng.bytes(i * largest / files + 1)).unwrap();
    }
    let h = fs.open("/far", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    fs.seek(h, far).unwrap();
    fs.write(h, b"far").unwrap();
    fs.close(h).unwrap();
    fs.set_attr("/d/f1", 1, b"attr").unwrap();
    fs.set_attr("/", 2, b"root").unwrap();
    fs.rename("/d/f2", "/d/e/g").unwrap();
    fs.remove("/d/f3").unwrap();
    sound(&mut fs, "populated");
    drop(fs);
    ram.data
}

/// Mounts, dumps and checks: the volume must be sound.
pub fn sound<D: BlockDevice>(fs: &mut Filesystem<D>, what: &str) -> Tree {
    let problems = fs.check().unwrap_or_else(|e| panic!("{what}: check: {e:?}"));
    assert!(problems.is_empty(), "{what}: {problems:?}");
    dump(fs).unwrap_or_else(|e| panic!("{what}: dump: {e:?}"))
}
