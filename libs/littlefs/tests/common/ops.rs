//! Operations on a volume, the in-memory model of what they do, and random workloads.
//! Shared by the model, crash and differential tests.

use std::collections::BTreeMap;

use littlefs::{BlockDevice, Error, Filesystem, OpenOptions};

use super::*;

const NAMES: [&str; 6] = ["a", "b", "cc", "dir", "e0", "a-rather-long-name-that-fills-metadata-quickly-0123456789"];
/// File sizes: inline, one block, several blocks.
const SIZES: [usize; 9] = [0, 1, 17, 64, 200, 511, 1000, 3000, 9000];

/// What random workloads draw from.
#[derive(Clone, Copy)]
pub struct Profile {
    pub names: &'static [&'static str],
    pub sizes: &'static [usize],
    pub max_attr: u64,
    /// How far past a file's end a patch may start, and a truncation may extend it.
    pub grow: u64,
}

impl Profile {
    /// Few names, deep trees, files of every shape. With 128-byte blocks one entry with the
    /// long name and three attributes cannot fit a metadata block at all (the reference fails
    /// the same way), so the long name is left out there.
    pub fn default_for(cfg: littlefs::Config) -> Profile {
        let names = if cfg.block_size < 256 { &NAMES[..5] } else { &NAMES[..] };
        Profile { names, sizes: &SIZES, max_attr: (cfg.block_size / 16) as u64, grow: 500 }
    }

    /// Many names and small files: directories split over several pairs and pairs empty
    /// and drop, while handles stay open; on a small volume the allocator soon reuses the
    /// dropped pairs' blocks.
    pub fn crowded() -> Profile {
        const MANY: [&str; 24] = [
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u",
            "v", "w", "x",
        ];
        Profile { names: &MANY, sizes: &[0, 3, 9, 40, 300], max_attr: 4, grow: 20 }
    }

    pub fn name(&self, rng: &mut Rng) -> &'static str { self.names[rng.below(self.names.len() as u64) as usize] }

    pub fn size(&self, rng: &mut Rng) -> usize { self.sizes[rng.below(self.sizes.len() as u64) as usize] }
}

/// One operation. `Write` creates or replaces a whole file; `Patch` overwrites part of one
/// (possibly past its end), then truncates it if `cut` says so.
#[derive(Clone, Debug)]
pub enum Op {
    Write { path: String, data: Vec<u8> },
    Patch { path: String, at: u32, data: Vec<u8>, cut: Option<u32> },
    Mkdir(String),
    Remove(String),
    Rename(String, String),
    SetAttr(String, u8, Vec<u8>),
    RemoveAttr(String, u8),
}

pub fn parent(path: &str) -> &str { path.rsplit_once('/').map_or("", |(p, _)| p) }

pub fn is_empty_dir(tree: &Tree, path: &str) -> bool {
    matches!(tree.get(path), Some(Node::Dir { .. })) && !tree.keys().any(|k| !k.is_empty() && parent(k) == path)
}

pub fn pick<'a>(rng: &mut Rng, tree: &'a Tree, f: impl Fn(&str, &Node) -> bool) -> Option<&'a String> {
    let c: Vec<&String> = tree.iter().filter(|(k, v)| f(k, v)).map(|(k, _)| k).collect();
    if c.is_empty() { None } else { Some(c[rng.below(c.len() as u64) as usize]) }
}

/// A new path in an existing directory.
pub fn fresh(rng: &mut Rng, tree: &Tree, p: &Profile) -> Option<String> {
    let dir = pick(rng, tree, |_, v| matches!(v, Node::Dir { .. }))?.clone();
    let path = format!("{dir}/{}", p.name(rng));
    (!tree.contains_key(&path)).then_some(path)
}

/// Every directory on the way to `path` exists and is a directory.
fn parent_ok(tree: &Tree, path: &str) -> Result<(), Error> {
    let mut cur = String::new();
    for name in parent(path).split('/').filter(|n| !n.is_empty()) {
        cur = format!("{cur}/{name}");
        match tree.get(&cur) {
            Some(Node::Dir { .. }) => {}
            Some(Node::File { .. }) => return Err(Error::NotDir),
            None => return Err(Error::NoEntry),
        }
    }
    Ok(())
}

/// What the model says an operation's outcome is.
pub fn expect(tree: &Tree, op: &Op) -> Result<(), Error> {
    let is_dir = |p: &str| matches!(tree.get(p), Some(Node::Dir { .. }));
    match op {
        Op::Write { path, .. } => {
            parent_ok(tree, path)?;
            if is_dir(path) { Err(Error::IsDir) } else { Ok(()) }
        }
        Op::Patch { path, .. } => {
            parent_ok(tree, path)?;
            match tree.get(path) {
                Some(Node::Dir { .. }) => Err(Error::IsDir),
                Some(Node::File { .. }) => Ok(()),
                None => Err(Error::NoEntry),
            }
        }
        Op::Mkdir(path) => {
            parent_ok(tree, path)?;
            if tree.contains_key(path) { Err(Error::Exists) } else { Ok(()) }
        }
        Op::Remove(path) => {
            parent_ok(tree, path)?;
            if !tree.contains_key(path) {
                Err(Error::NoEntry)
            } else if is_dir(path) && !is_empty_dir(tree, path) {
                Err(Error::NotEmpty)
            } else {
                Ok(())
            }
        }
        Op::Rename(from, to) => {
            parent_ok(tree, from)?;
            if !tree.contains_key(from) {
                return Err(Error::NoEntry);
            }
            if is_dir(from) && to.starts_with(&format!("{from}/")) {
                return Err(Error::Invalid);
            }
            parent_ok(tree, to)?;
            match (tree.get(to), is_dir(from)) {
                _ if to == from => Ok(()),
                (None, _) => Ok(()),
                (Some(Node::Dir { .. }), false) => Err(Error::IsDir),
                (Some(Node::File { .. }), true) => Err(Error::NotDir),
                (Some(Node::Dir { .. }), true) if !is_empty_dir(tree, to) => Err(Error::NotEmpty),
                _ => Ok(()),
            }
        }
        Op::SetAttr(path, ..) | Op::RemoveAttr(path, _) => {
            parent_ok(tree, path)?;
            if tree.contains_key(path) { Ok(()) } else { Err(Error::NoEntry) }
        }
    }
}

/// A random operation the model says succeeds, so that any failure is a finding.
pub fn generate(rng: &mut Rng, tree: &Tree, p: &Profile) -> Option<Op> {
    let files = |_: &str, v: &Node| matches!(v, Node::File { .. });
    Some(match rng.below(9) {
        0 | 1 => {
            let path = if rng.below(2) == 0 { fresh(rng, tree, p)? } else { pick(rng, tree, files)?.clone() };
            let n = p.size(rng);
            Op::Write { path, data: rng.bytes(n) }
        }
        2 => {
            let path = pick(rng, tree, files)?.clone();
            let Some(Node::File { data, .. }) = tree.get(&path) else { return None };
            let at = rng.below(data.len() as u64 + p.grow) as u32;
            let n = p.size(rng);
            let cut = (rng.below(3) == 0).then(|| rng.below(data.len() as u64 + 4 * p.grow) as u32);
            Op::Patch { path, at, data: rng.bytes(n), cut }
        }
        3 => Op::Mkdir(fresh(rng, tree, p)?),
        4 => {
            let path = pick(rng, tree, |k, v| !k.is_empty() && (matches!(v, Node::File { .. }) || is_empty_dir(tree, k)))?;
            Op::Remove(path.clone())
        }
        5 | 6 => {
            let from = pick(rng, tree, |k, _| !k.is_empty())?.clone();
            let from_dir = matches!(tree.get(&from), Some(Node::Dir { .. }));
            let to = if rng.below(2) == 0 {
                fresh(rng, tree, p)?
            } else if from_dir {
                pick(rng, tree, |k, _| !k.is_empty() && is_empty_dir(tree, k))?.clone()
            } else {
                pick(rng, tree, files)?.clone()
            };
            if to.starts_with(&format!("{from}/")) {
                return None;
            }
            Op::Rename(from, to)
        }
        7 => {
            let path = pick(rng, tree, |_, _| true)?.clone();
            let t = ATTR_TYPES[rng.below(ATTR_TYPES.len() as u64) as usize];
            let n = rng.below(p.max_attr) as usize;
            Op::SetAttr(path, t, rng.bytes(n))
        }
        _ => {
            let path = pick(rng, tree, |_, _| true)?.clone();
            Op::RemoveAttr(path, ATTR_TYPES[rng.below(ATTR_TYPES.len() as u64) as usize])
        }
    })
}

pub fn attrs_of<'a>(tree: &'a mut Tree, p: &str) -> Option<&'a mut BTreeMap<u8, Vec<u8>>> {
    match tree.get_mut(p) {
        Some(Node::Dir { attrs }) | Some(Node::File { attrs, .. }) => Some(attrs),
        None => None,
    }
}

pub fn apply_model(tree: &mut Tree, op: &Op) {
    match op {
        Op::Write { path, data } => {
            let attrs = attrs_of(tree, path).cloned().unwrap_or_default();
            tree.insert(path.clone(), Node::File { data: data.clone(), attrs });
        }
        Op::Patch { path, at, data: patch, cut } => {
            if let Some(Node::File { data, .. }) = tree.get_mut(path) {
                let end = *at as usize + patch.len();
                if data.len() < end {
                    data.resize(end, 0);
                }
                data[*at as usize..end].copy_from_slice(patch);
                if let Some(c) = cut {
                    data.resize(*c as usize, 0);
                }
            }
        }
        Op::Mkdir(path) => {
            tree.insert(path.clone(), Node::Dir { attrs: BTreeMap::new() });
        }
        Op::Remove(path) => {
            tree.remove(path);
        }
        Op::Rename(from, to) => {
            if from != to {
                let under = |k: &str| k == from || k.starts_with(&format!("{from}/"));
                let moved: Vec<(String, Node)> =
                    tree.iter().filter(|(k, _)| under(k)).map(|(k, v)| (format!("{to}{}", &k[from.len()..]), v.clone())).collect();
                tree.retain(|k, _| !under(k));
                tree.remove(to);
                tree.extend(moved);
            }
        }
        Op::SetAttr(path, t, v) => {
            attrs_of(tree, path).unwrap().insert(*t, v.clone());
        }
        Op::RemoveAttr(path, t) => {
            attrs_of(tree, path).unwrap().remove(t);
        }
    }
}

pub fn slash(path: &str) -> &str { if path.is_empty() { "/" } else { path } }

pub fn apply_rust<D: BlockDevice>(fs: &mut Filesystem<D>, op: &Op) -> Result<(), Error> {
    match op {
        Op::Write { path, data } => write_file(fs, path, data),
        Op::Patch { path, at, data, cut } => (|| {
            let h = fs.open(path, OpenOptions { read: true, write: true, ..Default::default() })?;
            fs.seek(h, *at)?;
            fs.write(h, data)?;
            if let Some(c) = cut {
                fs.truncate(h, *c)?;
            }
            fs.close(h)
        })(),
        Op::Mkdir(p) => fs.mkdir(p),
        Op::Remove(p) => fs.remove(p),
        Op::Rename(a, b) => fs.rename(a, b),
        Op::SetAttr(p, t, v) => fs.set_attr(slash(p), *t, v),
        Op::RemoveAttr(p, t) => fs.remove_attr(slash(p), *t),
    }
}
