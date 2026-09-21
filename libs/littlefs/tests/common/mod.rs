//! Shared test helpers: the one RAM block device the tests use (with power failure and
//! tearing), a deterministic RNG, and a dump of a whole volume for comparisons.
#![allow(dead_code)]

use std::collections::BTreeMap;

pub mod ops;

use littlefs::{BlockDevice, Config, Error, FileType, Filesystem, OpenOptions};

/// How a write interrupted by power loss lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tear {
    /// The `BlockDevice` contract: a program persists a prefix of whole units (any number
    /// of them, chosen at random), then possibly part of the next unit; an erase either
    /// happens or not.
    Prefix,
    /// As `Prefix`, and an erase may be torn too: a random prefix of the block is erased.
    PrefixAndErase,
    /// Outside the contract: any subset of a program's units persists (a device that
    /// reorders writes within one program).
    Subset,
}

/// A RAM disk that behaves like flash: erase sets 0xff, programs must hit erased bytes and
/// be aligned. A program over unerased bytes is a bug in the filesystem, so it panics
/// (unless `strict` is off: hostile images can direct a correct filesystem there).
///
/// `budget`: writes (programs and erases) allowed; the last one is torn as `tear` says, and
/// nothing after it persists (every later write and sync fails).
pub struct Ram {
    pub cfg: Config,
    pub data: Vec<u8>,
    pub budget: Option<u64>,
    pub writes: u64,
    pub strict: bool,
    pub tear: Tear,
    pub rng: Rng,
}

impl Ram {
    pub fn new(cfg: Config) -> Ram { Ram::from_image(cfg, vec![0xff; (cfg.block_size * cfg.block_count) as usize]) }

    pub fn from_image(cfg: Config, data: Vec<u8>) -> Ram {
        Ram { cfg, data, budget: None, writes: 0, strict: true, tear: Tear::Prefix, rng: Rng(1) }
    }

    /// Power fails at write number `n` (1-based), torn as `tear` says; `seed` drives the tear.
    pub fn failing_at(mut self, n: u64, tear: Tear, seed: u64) -> Ram {
        self.budget = Some(n);
        self.tear = tear;
        self.rng = Rng(seed | 1);
        self
    }

    fn at(&self, block: u32, off: u32) -> usize { (block * self.cfg.block_size + off) as usize }

    /// Counts a write: `Ok(false)` to perform it, `Ok(true)` to tear it, `Err` once power is
    /// gone.
    fn spend(&mut self) -> Result<bool, Error> {
        self.writes += 1;
        match self.budget {
            Some(b) if self.writes > b => Err(Error::Io),
            Some(b) if self.writes == b => Ok(true),
            _ => Ok(false),
        }
    }
}

impl BlockDevice for Ram {
    fn read(&mut self, block: u32, off: u32, buf: &mut [u8]) -> Result<(), Error> {
        assert!(block < self.cfg.block_count && off as usize + buf.len() <= self.cfg.block_size as usize);
        let at = self.at(block, off);
        buf.copy_from_slice(&self.data[at..at + buf.len()]);
        Ok(())
    }

    fn prog(&mut self, block: u32, off: u32, data: &[u8]) -> Result<(), Error> {
        assert!(block < self.cfg.block_count && off as usize + data.len() <= self.cfg.block_size as usize);
        assert!(off.is_multiple_of(self.cfg.prog_size) && (data.len() as u32).is_multiple_of(self.cfg.prog_size));
        let at = self.at(block, off);
        if self.strict {
            assert!(self.data[at..at + data.len()].iter().all(|b| *b == 0xff), "program over unerased bytes");
        }
        if !self.spend()? {
            self.data[at..at + data.len()].copy_from_slice(data);
            return Ok(());
        }
        let unit = self.cfg.prog_size as usize;
        let units = data.len() / unit;
        if self.tear == Tear::Subset {
            for u in 0..units {
                if self.rng.below(2) == 0 {
                    self.data[at + u * unit..at + (u + 1) * unit].copy_from_slice(&data[u * unit..(u + 1) * unit]);
                }
            }
            return Err(Error::Io);
        }
        // A prefix of whole units lands; the next unit is partly programmed (flash clears
        // bits, so a partial program leaves some of them set).
        let k = self.rng.below(units as u64 + 1) as usize;
        self.data[at..at + k * unit].copy_from_slice(&data[..k * unit]);
        if k < units {
            let mode = self.rng.below(3);
            let partial = k * unit..(k + 1) * unit;
            for (dst, &src) in self.data[at + partial.start..at + partial.end].iter_mut().zip(&data[partial]) {
                let kept = match mode {
                    0 => self.rng.next() as u8,
                    1 if self.rng.below(2) == 0 => 0xff,
                    _ => 0,
                };
                *dst = src | !kept;
            }
        }
        Err(Error::Io)
    }

    fn erase(&mut self, block: u32) -> Result<(), Error> {
        assert!(block < self.cfg.block_count);
        let bs = self.cfg.block_size as usize;
        let at = self.at(block, 0);
        if !self.spend()? {
            self.data[at..at + bs].fill(0xff);
            return Ok(());
        }
        if self.tear == Tear::PrefixAndErase {
            let n = self.rng.below(bs as u64 + 1) as usize;
            self.data[at..at + n].fill(0xff);
        }
        Err(Error::Io)
    }

    fn sync(&mut self) -> Result<(), Error> {
        if matches!(self.budget, Some(b) if self.writes >= b) { Err(Error::Io) } else { Ok(()) }
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
    let h = fs.open(path, OpenOptions { read: true, ..Default::default() })?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 777];
    loop {
        let n = fs.read(h, &mut buf)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    fs.close(h)?;
    Ok(out)
}

pub fn write_file<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str, data: &[u8]) -> Result<(), Error> {
    let h = fs.open(path, OpenOptions { write: true, create: true, truncate: true, ..Default::default() })?;
    let r = fs.write(h, data).map(|_| ());
    let c = fs.close(h);
    r.and(c)
}

fn attrs<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str) -> Result<BTreeMap<u8, Vec<u8>>, Error> {
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
        Some(Node::File { data, attrs }) => format!("file of {} bytes, attrs {:?}", data.len(), attrs.keys().collect::<Vec<_>>()),
    };
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).map(|k| format!("{k}: {} vs {}", brief(a.get(k)), brief(b.get(k)))).collect()
}

/// Reads everything in the volume.
pub fn dump<D: BlockDevice>(fs: &mut Filesystem<D>) -> Result<Tree, Error> {
    let mut tree = Tree::new();
    tree.insert(String::new(), Node::Dir { attrs: attrs(fs, "/")? });
    let mut todo = vec![String::new()];
    while let Some(dir) = todo.pop() {
        let mut entries = Vec::new();
        fs.read_dir(&format!("{dir}/"), |e| {
            entries.push((String::from_utf8_lossy(e.name).into_owned(), e.meta.kind, e.meta.size))
        })?;
        for (name, kind, size) in entries {
            let path = format!("{dir}/{name}");
            let node = match kind {
                FileType::Dir => {
                    todo.push(path.clone());
                    Node::Dir { attrs: attrs(fs, &path)? }
                }
                FileType::File => {
                    let data = read_file(fs, &path)?;
                    assert_eq!(data.len() as u32, size, "stat size of {path}");
                    Node::File { data, attrs: attrs(fs, &path)? }
                }
            };
            tree.insert(path, node);
        }
    }
    Ok(tree)
}
