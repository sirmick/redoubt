//! Operations on a volume, the in-memory model of what they do, and random workloads. Shared by
//! the model and crash tests.

use std::collections::BTreeMap;

use walfs::{ATTR_MAX, ATTRS, BlockDevice, Error, Filesystem, OpenOptions};

use super::*;

const NAMES: [&str; 6] = ["a", "b", "cc", "dir", "e0", "a-rather-long-name-that-takes-room-0123456789"];
/// File sizes: nothing, a little, a block and either side of it, several blocks.
const SIZES: [usize; 9] = [0, 1, 17, 300, 4095, 4096, 4097, 9000, 20_000];

/// What random workloads draw from.
#[derive(Clone, Copy)]
pub struct Profile {
    pub names: &'static [&'static str],
    pub sizes: &'static [usize],
    pub max_attr: u64,
    /// How far past a file's end a patch may start, and a truncation may extend it.
    pub grow: u64,
    /// One patch in this many starts far out, past the direct blocks or the single-indirect ones
    /// (0: never).
    pub far: u64,
}

impl Profile {
    /// Few names, deep trees, files of every shape, now and then far past their ends.
    pub fn default() -> Profile {
        Profile { names: &NAMES, sizes: &SIZES, max_attr: 120, grow: 5000, far: 25 }
    }

    /// Many names and small files: directories grow past one block, on a small volume that is
    /// soon full.
    pub fn crowded() -> Profile {
        const MANY: [&str; 24] = [
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
            "t", "u", "v", "w", "x",
        ];
        Profile { names: &MANY, sizes: &[0, 3, 9, 40, 300, 5000], max_attr: 40, grow: 20, far: 0 }
    }

    pub fn name(&self, rng: &mut Rng) -> &'static str {
        self.names[rng.below(self.names.len() as u64) as usize]
    }

    pub fn size(&self, rng: &mut Rng) -> usize { self.sizes[rng.below(self.sizes.len() as u64) as usize] }

    /// Where a patch of a file of `len` bytes starts.
    pub fn at(&self, rng: &mut Rng, len: u64) -> u64 {
        if self.far > 0 && rng.below(self.far) == 0 {
            // Past the direct blocks (48 KiB) or past the single-indirect ones (about 4.2 MB).
            if rng.below(2) == 0 { 49_152 + rng.below(100_000) } else { 4_243_456 + rng.below(100_000) }
        } else {
            rng.below(len + self.grow)
        }
    }
}

/// One operation. `Write` creates or replaces a whole file; `Patch` overwrites part of one
/// (possibly past its end), then truncates it if `cut` says so.
#[derive(Clone, Debug)]
pub enum Op {
    Write { path: String, data: Vec<u8> },
    Patch { path: String, at: u64, data: Vec<u8>, cut: Option<u64> },
    Mkdir(String),
    Remove(String),
    Rename(String, String),
    SetAttr(String, u8, Vec<u8>),
    RemoveAttr(String, u8),
}

/// How a write-carrying operation ended: whole, or with only its first `n` bytes written before
/// the volume filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    All,
    Short(usize),
}

pub fn parent(path: &str) -> &str { path.rsplit_once('/').map_or("", |(p, _)| p) }

pub fn is_empty_dir(tree: &Tree, path: &str) -> bool {
    matches!(tree.get(path), Some(Node::Dir { .. }))
        && !tree.keys().any(|k| !k.is_empty() && parent(k) == path)
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

pub fn attrs_of<'a>(tree: &'a mut Tree, p: &str) -> Option<&'a mut BTreeMap<u8, Vec<u8>>> {
    match tree.get_mut(p) {
        Some(Node::Dir { attrs }) | Some(Node::File { attrs, .. }) => Some(attrs),
        None => None,
    }
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
        Op::SetAttr(path, t, v) => {
            parent_ok(tree, path)?;
            let Some(mut attrs) = tree.get(path).map(|n| match n {
                Node::Dir { attrs } | Node::File { attrs, .. } => attrs.clone(),
            }) else {
                return Err(Error::NoEntry);
            };
            attrs.insert(*t, v.clone());
            let room: usize = attrs.values().map(|v| 2 + v.len()).sum();
            if v.len() > ATTR_MAX || room > ATTRS { Err(Error::NoSpace) } else { Ok(()) }
        }
        Op::RemoveAttr(path, _) => {
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
            let at = p.at(rng, data.len() as u64);
            let n = p.size(rng);
            let cut = (rng.below(3) == 0).then(|| rng.below(data.len() as u64 + 4 * p.grow));
            Op::Patch { path, at, data: rng.bytes(n), cut }
        }
        3 => Op::Mkdir(fresh(rng, tree, p)?),
        4 => {
            let path = pick(rng, tree, |k, v| {
                !k.is_empty() && (matches!(v, Node::File { .. }) || is_empty_dir(tree, k))
            })?;
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

/// Patches `data` with `patch` at `at`, zero-filling any gap.
fn overlay(data: &mut Vec<u8>, at: u64, patch: &[u8]) {
    if patch.is_empty() {
        return;
    }
    let end = at as usize + patch.len();
    if data.len() < end {
        data.resize(end, 0);
    }
    data[at as usize..end].copy_from_slice(patch);
}

/// What `op` does to the model, whole or, for a write cut short by a full volume, as far as it
/// went.
pub fn apply_model(tree: &mut Tree, op: &Op, done: Done) {
    match op {
        Op::Write { path, data } => {
            let attrs = attrs_of(tree, path).cloned().unwrap_or_default();
            let n = match done {
                Done::All => data.len(),
                Done::Short(n) => n,
            };
            tree.insert(path.clone(), Node::File { data: data[..n].to_vec(), attrs });
        }
        Op::Patch { path, at, data: patch, cut } => {
            if let Some(Node::File { data, .. }) = tree.get_mut(path) {
                match done {
                    Done::All => {
                        overlay(data, *at, patch);
                        if let Some(c) = cut {
                            data.resize(*c as usize, 0);
                        }
                    }
                    Done::Short(n) => overlay(data, *at, &patch[..n]),
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
                let moved: Vec<(String, Node)> = tree
                    .iter()
                    .filter(|(k, _)| under(k))
                    .map(|(k, v)| (format!("{to}{}", &k[from.len()..]), v.clone()))
                    .collect();
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

/// A write through `h` of `data`: whole, or as far as a full volume let it go.
fn write_all<D: BlockDevice>(
    fs: &mut Filesystem<D>,
    h: walfs::FileHandle,
    data: &[u8],
) -> Result<Done, Error> {
    match fs.write(h, data) {
        Ok(n) if n == data.len() => Ok(Done::All),
        Ok(n) => Ok(Done::Short(n)),
        Err(Error::NoSpace) => Ok(Done::Short(0)),
        Err(e) => Err(e),
    }
}

pub fn apply<D: BlockDevice>(fs: &mut Filesystem<D>, op: &Op) -> Result<Done, Error> {
    match op {
        Op::Write { path, data } => {
            let h = fs.open(
                path,
                OpenOptions { write: true, create: true, truncate: true, ..Default::default() },
            )?;
            let done = write_all(fs, h, data);
            let closed = fs.close(h);
            let done = done?;
            closed.map(|()| done)
        }
        Op::Patch { path, at, data, cut } => {
            let h = fs.open(path, OpenOptions { read: true, write: true, ..Default::default() })?;
            fs.seek(h, *at)?;
            let mut done = write_all(fs, h, data);
            if let (Ok(Done::All), Some(c)) = (&done, cut) {
                done = fs.truncate(h, *c).map(|()| Done::All);
            }
            let closed = fs.close(h);
            let done = done?;
            closed.map(|()| done)
        }
        Op::Mkdir(p) => fs.mkdir(p).map(|()| Done::All),
        Op::Remove(p) => fs.remove(p).map(|()| Done::All),
        Op::Rename(a, b) => fs.rename(a, b).map(|()| Done::All),
        Op::SetAttr(p, t, v) => fs.set_attr(slash(p), *t, v).map(|()| Done::All),
        Op::RemoveAttr(p, t) => fs.remove_attr(slash(p), *t).map(|()| Done::All),
    }
}
