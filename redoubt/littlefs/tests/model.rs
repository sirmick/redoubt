//! Model-based tests: random operations against the filesystem and against an in-memory
//! model of what it should hold; after every operation the results must agree, and at
//! intervals the volume is remounted, dumped, compared and checked.

mod common;

use std::collections::BTreeMap;

use common::*;
use littlefs::{Config, Error, Filesystem, OpenOptions, SeekFrom};

const NAMES: [&str; 6] = ["a", "b", "cc", "dir", "e0", "a-rather-long-name-that-fills-metadata-quickly-0123456789"];

/// Mostly paths that exist or whose parent exists, so most operations get somewhere; now
/// and then a random one, so errors are exercised too.
fn random_path(rng: &mut Rng, tree: &Tree, names: usize) -> String {
    let name = |rng: &mut Rng| NAMES[rng.below(names as u64) as usize];
    let pick = |rng: &mut Rng, dirs_only: bool| {
        let c: Vec<&String> =
            tree.iter().filter(|(k, v)| !k.is_empty() && (!dirs_only || matches!(v, Node::Dir { .. }))).map(|(k, _)| k).collect();
        if c.is_empty() { String::new() } else { c[rng.below(c.len() as u64) as usize].clone() }
    };
    match rng.below(4) {
        0 => pick(rng, false),
        1 | 2 => format!("{}/{}", pick(rng, true), name(rng)),
        _ => {
            let depth = 1 + rng.below(3);
            (0..depth).map(|_| format!("/{}", name(rng))).collect()
        }
    }
}

fn parent(path: &str) -> String { path.rsplit_once('/').map_or(String::new(), |(p, _)| p.to_string()) }

fn children<'a>(tree: &'a Tree, dir: &'a str) -> impl Iterator<Item = &'a String> + 'a {
    tree.keys().filter(move |k| !k.is_empty() && parent(k) == dir)
}

/// What the model says an operation's outcome is, and the tree after it.
struct Model {
    tree: Tree,
}

impl Model {
    fn get(&self, path: &str) -> Option<&Node> { self.tree.get(path) }

    fn parent_ok(&self, path: &str) -> Result<(), Error> {
        let p = parent(path);
        let mut cur = String::new();
        for name in p.split('/').filter(|n| !n.is_empty()) {
            cur = format!("{cur}/{name}");
            match self.get(&cur) {
                Some(Node::Dir { .. }) => {}
                Some(Node::File { .. }) => return Err(Error::NotDir),
                None => return Err(Error::NoEntry),
            }
        }
        Ok(())
    }
}

fn file_data(tree: &Tree, path: &str) -> Vec<u8> {
    match tree.get(path) {
        Some(Node::File { data, .. }) => data.clone(),
        _ => Vec::new(),
    }
}

fn run(cfg: Config, seed: u64, steps: usize) {
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    let mut m = Model { tree: dump(&mut fs).unwrap() };
    let mut rng = Rng(seed);
    let mut oks = [0u32; 10];
    // With 128-byte blocks one entry with the long name and three attributes cannot fit a
    // metadata block at all; the reference fails the same way.
    let names = if cfg.block_size < 256 { NAMES.len() - 1 } else { NAMES.len() };
    let sizes = [0usize, 1, 17, 64, 200, 511, 1000, 3000, 9000];

    for step in 0..steps {
        let path = random_path(&mut rng, &m.tree, names);
        if path.is_empty() {
            continue;
        }
        let op = rng.below(10);
        let (want, got): (Result<(), Error>, Result<(), Error>) = match op {
            // Write a whole file.
            0 | 1 => {
                let n = sizes[rng.below(sizes.len() as u64) as usize];
                let data = rng.bytes(n);
                let want = m.parent_ok(&path).and(match m.get(&path) {
                    Some(Node::Dir { .. }) => Err(Error::IsDir),
                    _ => Ok(()),
                });
                let got = write_file(&mut fs, &path, &data);
                if want.is_ok() {
                    let attrs = match m.get(&path) {
                        Some(Node::File { attrs, .. }) => attrs.clone(),
                        _ => BTreeMap::new(),
                    };
                    m.tree.insert(path.clone(), Node::File { data, attrs });
                }
                (want, got)
            }
            // Patch an existing file: overwrite at an offset (possibly past the end), then
            // sometimes truncate.
            2 => {
                let want = m.parent_ok(&path).and(match m.get(&path) {
                    Some(Node::Dir { .. }) => Err(Error::IsDir),
                    Some(Node::File { .. }) => Ok(()),
                    None => Err(Error::NoEntry),
                });
                let old = file_data(&m.tree, &path);
                let at = rng.below(old.len() as u64 + 300) as u32;
                let n = sizes[rng.below(sizes.len() as u64) as usize];
                let patch = rng.bytes(n);
                let cut = if rng.below(3) == 0 { Some(rng.below(old.len() as u64 + 2000) as u32) } else { None };
                let got = (|| {
                    let h = fs.open(&path, OpenOptions { read: true, write: true, ..Default::default() })?;
                    fs.seek(h, SeekFrom::Start(at))?;
                    fs.write(h, &patch)?;
                    if let Some(c) = cut {
                        fs.truncate(h, c)?;
                    }
                    fs.close(h)
                })();
                if want.is_ok() {
                    let mut d = old;
                    let end = at as usize + patch.len();
                    if d.len() < end {
                        d.resize(end, 0);
                    }
                    d[at as usize..end].copy_from_slice(&patch);
                    if let Some(c) = cut {
                        d.resize(c as usize, 0);
                    }
                    if let Some(Node::File { data, .. }) = m.tree.get_mut(&path) {
                        *data = d;
                    }
                }
                (want, got)
            }
            3 => {
                let want = m.parent_ok(&path).and(if m.get(&path).is_some() { Err(Error::Exists) } else { Ok(()) });
                let got = fs.mkdir(&path);
                if want.is_ok() {
                    m.tree.insert(path.clone(), Node::Dir { attrs: BTreeMap::new() });
                }
                (want, got)
            }
            4 | 5 => {
                let want = m.parent_ok(&path).and(match m.get(&path) {
                    None => Err(Error::NoEntry),
                    Some(Node::Dir { .. }) if children(&m.tree, &path).next().is_some() => Err(Error::NotEmpty),
                    Some(_) => Ok(()),
                });
                let got = fs.remove(&path);
                if want.is_ok() {
                    m.tree.remove(&path);
                }
                (want, got)
            }
            6 | 7 => {
                let to = random_path(&mut rng, &m.tree, names);
                let to = if to.is_empty() { "/x".to_string() } else { to };
                let want = m.parent_ok(&path).and_then(|()| {
                    let src = m.get(&path).ok_or(Error::NoEntry)?;
                    let is_dir = matches!(src, Node::Dir { .. });
                    if is_dir && to.starts_with(&format!("{path}/")) {
                        return Err(Error::Invalid);
                    }
                    m.parent_ok(&to)?;
                    match (m.get(&to), is_dir) {
                        _ if to == path => Ok(()),
                        (None, _) => Ok(()),
                        (Some(Node::Dir { .. }), false) => Err(Error::IsDir),
                        (Some(Node::File { .. }), true) => Err(Error::NotDir),
                        (Some(Node::Dir { .. }), true) if children(&m.tree, &to).next().is_some() => {
                            Err(Error::NotEmpty)
                        }
                        _ => Ok(()),
                    }
                });
                let got = fs.rename(&path, &to);
                if want.is_ok() && to != path {
                    let moved: Vec<(String, Node)> = m
                        .tree
                        .iter()
                        .filter(|(k, _)| **k == path || k.starts_with(&format!("{path}/")))
                        .map(|(k, v)| (format!("{to}{}", &k[path.len()..]), v.clone()))
                        .collect();
                    m.tree.retain(|k, _| !(*k == path || k.starts_with(&format!("{path}/"))));
                    m.tree.remove(&to);
                    m.tree.extend(moved);
                }
                (want, got)
            }
            _ => {
                let t = ATTR_TYPES[rng.below(ATTR_TYPES.len() as u64) as usize];
                let target = if rng.below(8) == 0 { String::new() } else { path.clone() };
                let want = m.parent_ok(&target).and(if m.get(&target).is_some() { Ok(()) } else { Err(Error::NoEntry) });
                let remove = rng.below(3) == 0;
                // A long name, inline data and three attributes must fit one metadata block.
                let n = rng.below(cfg.block_size as u64 / 16) as usize;
                let value = rng.bytes(n);
                let p = if target.is_empty() { "/".to_string() } else { target.clone() };
                let got = if remove { fs.remove_attr(&p, t) } else { fs.set_attr(&p, t, &value) };
                if want.is_ok() {
                    if let Some(Node::Dir { attrs } | Node::File { attrs, .. }) = m.tree.get_mut(&target) {
                        if remove {
                            attrs.remove(&t);
                        } else {
                            attrs.insert(t, value);
                        }
                    }
                }
                (want, got)
            }
        };
        if want.is_ok() { oks[op as usize] += 1; }
        match (&want, &got) {
            (Ok(()), Err(Error::NoSpace)) => panic!("seed {seed} step {step}: volume full; shrink the test"),
            _ => assert_eq!(want, got, "seed {seed} step {step} op {op} path {path}"),
        }

        if step % 50 == 49 {
            drop(fs);
            fs = Filesystem::mount(&mut ram, cfg).unwrap();
            assert_eq!(dump(&mut fs).unwrap(), m.tree, "seed {seed} after step {step}");
            fs.fsck().unwrap();
        }
    }
    let t = dump(&mut fs).unwrap();
    assert_eq!(t, m.tree);
    fs.fsck().unwrap();
    eprintln!("seed {seed}: successful ops by kind {oks:?}, {} paths at the end", t.len());
}

#[test]
fn random_operations_small_blocks() {
    for seed in 1..=6 {
        run(Config { block_size: 256, block_count: 4096, prog_size: 16 }, seed, 1500);
    }
}

#[test]
fn random_operations_large_blocks() {
    for seed in 10..=13 {
        run(Config { block_size: 4096, block_count: 128, prog_size: 512 }, seed, 1000);
    }
}

#[test]
fn random_operations_tiny_blocks() {
    for seed in 20..=23 {
        run(Config { block_size: 128, block_count: 16384, prog_size: 1 }, seed, 1000);
    }
}

/// Many entries in one directory: the metadata splits over several pairs, and removing them
/// all drops the extra pairs again.
#[test]
fn directory_split_and_drop() {
    let cfg = Config { block_size: 256, block_count: 256, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    fs.mkdir("/d").unwrap();
    for i in 0..120 {
        write_file(&mut fs, &format!("/d/file{i:03}"), format!("contents {i}").as_bytes()).unwrap();
    }
    let mut n = 0;
    fs.read_dir("/d", |_| n += 1).unwrap();
    assert_eq!(n, 120);
    fs.fsck().unwrap();
    for i in 0..120 {
        assert_eq!(read_file(&mut fs, &format!("/d/file{i:03}")).unwrap(), format!("contents {i}").as_bytes());
    }
    for i in 0..120 {
        fs.remove(&format!("/d/file{i:03}")).unwrap();
    }
    fs.remove("/d").unwrap();
    fs.fsck().unwrap();
    drop(fs);
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    let mut n = 0;
    fs.read_dir("/", |_| n += 1).unwrap();
    assert_eq!(n, 0);
    fs.fsck().unwrap();
}

/// Filling the volume reports NoSpace and leaves it usable.
#[test]
fn full_volume() {
    let cfg = Config { block_size: 256, block_count: 64, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    write_file(&mut fs, "/keep", b"survives").unwrap();
    let h = fs.open("/big", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    let chunk = vec![7u8; 1000];
    let err = loop {
        if let Err(e) = fs.write(h, &chunk) {
            break e;
        }
    };
    assert_eq!(err, Error::NoSpace);
    fs.close(h).unwrap_or(());
    assert_eq!(read_file(&mut fs, "/keep").unwrap(), b"survives");
    fs.remove("/big").unwrap();
    write_file(&mut fs, "/after", &vec![1u8; 3000]).unwrap();
    fs.fsck().unwrap();
}

/// Open handles follow renames and survive removal.
#[test]
fn handles_follow_renames() {
    let cfg = Config { block_size: 256, block_count: 128, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    fs.mkdir("/x").unwrap();
    let h = fs.open("/f", OpenOptions { read: true, write: true, create: true, ..Default::default() }).unwrap();
    fs.write(h, b"hello").unwrap();
    fs.rename("/f", "/x/g").unwrap();
    fs.write(h, b" world").unwrap();
    fs.close(h).unwrap();
    assert_eq!(read_file(&mut fs, "/x/g").unwrap(), b"hello world");
    assert_eq!(fs.stat("/f"), Err(Error::NoEntry));

    let h = fs.open("/x/g", OpenOptions { read: true, ..Default::default() }).unwrap();
    fs.remove("/x/g").unwrap();
    let mut buf = [0u8; 32];
    let n = fs.read(h, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello world");
    fs.close(h).unwrap();
    fs.fsck().unwrap();
}

#[test]
fn bad_arguments() {
    let cfg = Config { block_size: 256, block_count: 32, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    assert_eq!(fs.mkdir("/a/.."), Err(Error::Invalid));
    assert_eq!(fs.mkdir("/"), Err(Error::Exists));
    assert_eq!(fs.remove("/"), Err(Error::Invalid));
    assert_eq!(fs.mkdir(&"n".repeat(256)), Err(Error::NameTooLong));
    fs.mkdir("/d").unwrap();
    assert_eq!(fs.rename("/d", "/d/e"), Err(Error::Invalid));
    assert_eq!(fs.open("/d", OpenOptions { read: true, ..Default::default() }), Err(Error::IsDir));
    assert_eq!(fs.set_attr("/d", 1, &[0; 1023]), Err(Error::NoSpace));
    assert_eq!(fs.get_attr("/d", 1), Err(Error::NoAttr));
    assert_eq!(fs.open("/nope", OpenOptions { read: true, create: true, ..Default::default() }), Err(Error::Invalid));
}
