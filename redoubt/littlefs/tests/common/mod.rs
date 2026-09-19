//! Shared test helpers: a RAM block device (optionally failing at the Nth write), a
//! deterministic RNG, and a dump of a whole volume for comparisons.
#![allow(dead_code)]

use std::collections::BTreeMap;

use littlefs::{BlockDevice, Config, Error, FileType, Filesystem, OpenOptions};

/// A RAM disk that behaves like flash: erase sets 0xff, programs must hit erased bytes and
/// be aligned. A program over unerased bytes is a bug in the filesystem, so it panics.
///
/// `budget`: writes (programs and erases) allowed before power fails. The failing program
/// persists only its first half (a torn write); after that nothing persists.
pub struct Ram {
    pub cfg: Config,
    pub data: Vec<u8>,
    pub budget: Option<u64>,
    pub writes: u64,
}

impl Ram {
    pub fn new(cfg: Config) -> Ram {
        Ram { cfg, data: vec![0xff; (cfg.block_size * cfg.block_count) as usize], budget: None, writes: 0 }
    }

    pub fn from_image(cfg: Config, data: Vec<u8>) -> Ram { Ram { cfg, data, budget: None, writes: 0 } }

    fn at(&self, block: u32, off: u32) -> usize { (block * self.cfg.block_size + off) as usize }

    /// Counts a write; `Err` once power is gone.
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
        assert!(off % self.cfg.prog_size == 0 && data.len() as u32 % self.cfg.prog_size == 0);
        let at = self.at(block, off);
        assert!(self.data[at..at + data.len()].iter().all(|b| *b == 0xff), "program over unerased bytes");
        let torn = self.spend()?;
        let n = if torn { data.len() / 2 } else { data.len() };
        self.data[at..at + n].copy_from_slice(&data[..n]);
        if torn { Err(Error::Io) } else { Ok(()) }
    }

    fn erase(&mut self, block: u32) -> Result<(), Error> {
        assert!(block < self.cfg.block_count);
        if self.spend()? {
            return Err(Error::Io);
        }
        let at = self.at(block, 0);
        self.data[at..at + self.cfg.block_size as usize].fill(0xff);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> {
        if matches!(self.budget, Some(b) if self.writes >= b) { Err(Error::Io) } else { Ok(()) }
    }
}

/// xorshift64*: deterministic and dependency-free.
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

/// Reads everything in the volume.
pub fn dump<D: BlockDevice>(fs: &mut Filesystem<D>) -> Result<Tree, Error> {
    let mut tree = Tree::new();
    tree.insert(String::new(), Node::Dir { attrs: attrs(fs, "/")? });
    let mut todo = vec![String::new()];
    while let Some(dir) = todo.pop() {
        let mut entries = Vec::new();
        fs.read_dir(&format!("{dir}/"), |e| {
            entries.push((String::from_utf8_lossy(e.name).into_owned(), e.kind, e.size))
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
