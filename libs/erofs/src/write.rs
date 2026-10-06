//! The writer: a tree packed into a volume in the subset the parser reads.
//!
//! Layout: the superblock in block 0; then the inode area from the superblock's end, each inode
//! with its extended attributes and its inline tail kept inside one block, the root first so its
//! number fits the superblock's 16 bits; then every whole data block, file by file, directory by
//! directory, in the tree's order.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::field::{dirent, inode as field_inode, sb as field_sb};
use crate::{
    BLOCK, BLOCK_BITS, COMPACT, DIRENT, EXTENDED, LAYOUT_INLINE, LAYOUT_PLAIN, MAGIC, NAME_MAX, NULL_BLOCK,
    S_IFDIR, S_IFREG, SLOT, SUPERBLOCK_AT, SUPERBLOCK_LEN, XATTR_HEADER,
};

/// One thing in the tree to pack: a path relative to the root, `/`-separated, its directory
/// named before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry<'a> {
    Dir(&'a str),
    File(&'a str, &'a [u8]),
}

/// Why a tree was not packed, and the path it stopped at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackError {
    pub path: String,
    pub why: &'static str,
}

/// `user.`, the attribute's name index, and its name after it.
const XATTR_USER: u8 = 1;
const XATTR_NAME: &[u8] = b"sha256";
/// A file's attribute area: the header, then one entry (its 4 bytes, the name and the 32-byte
/// value), padded to 4 bytes.
const XATTRS: usize = XATTR_HEADER + (4 + XATTR_NAME.len() + 32).next_multiple_of(4);
/// The count an inode records for [`XATTRS`] bytes: the header is one, every 4 bytes after it one.
const XATTR_COUNT: u16 = 1 + ((XATTRS - XATTR_HEADER) / 4) as u16;
const FT_FILE: u8 = 1;
const FT_DIR: u8 = 2;

/// Writes `bytes` into `to` at `at`.
fn put(to: &mut [u8], at: usize, bytes: &[u8]) { to[at..at + bytes.len()].copy_from_slice(bytes) }

struct Node<'a> {
    name: &'a [u8],
    parent: usize,
    /// A file's bytes; `None` for a directory.
    data: Option<&'a [u8]>,
    children: Vec<usize>,
    /// The data's bytes: a file's, or a directory's entry blocks.
    size: u64,
    nlink: u32,
    /// Where the inode starts, in bytes, and its inode number.
    at: usize,
    extended: bool,
    inline: bool,
    start: u32,
}

impl<'a> Node<'a> {
    fn new(name: &'a [u8], parent: usize, data: Option<&'a [u8]>, size: u64, nlink: u32) -> Node<'a> {
        Node {
            name,
            parent,
            data,
            children: Vec::new(),
            size,
            nlink,
            at: 0,
            extended: false,
            inline: false,
            start: 0,
        }
    }

    fn nid(&self) -> u64 { (self.at / SLOT) as u64 }
}

fn refused(path: &str, why: &'static str) -> PackError { PackError { path: path.to_string(), why } }

/// Packs `tree` into a volume's bytes, a whole number of blocks; `sha256` gives each file's
/// digest for its `user.sha256` attribute. Refused: a path whose last component is empty, `.`,
/// `..`, longer than [`NAME_MAX`] or holding a NUL; one whose directory was not named before it;
/// a path named twice; and a volume of more blocks than 32 bits count.
pub fn pack(tree: &[Entry<'_>], sha256: impl Fn(&[u8]) -> [u8; 32]) -> Result<Vec<u8>, PackError> {
    let mut nodes = vec![Node::new(b"", 0, None, 0, 2)];
    let mut paths: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in tree {
        let (path, data) = match *entry {
            Entry::Dir(path) => (path, None),
            Entry::File(path, data) => (path, Some(data)),
        };
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        if name.is_empty() || name == "." || name == ".." || name.len() > NAME_MAX || name.contains('\0') {
            return Err(refused(path, "not a name"));
        }
        let parent = match dir {
            "" if !path.starts_with('/') => 0,
            dir => *paths
                .get(dir)
                .filter(|p| nodes[**p].data.is_none())
                .ok_or_else(|| refused(path, "no directory"))?,
        };
        let index = nodes.len();
        if paths.insert(path, index).is_some() {
            return Err(refused(path, "named twice"));
        }
        nodes[parent].children.push(index);
        if data.is_none() {
            nodes[parent].nlink += 1;
        }
        let size = data.map_or(0, |d| d.len() as u64);
        let nlink = if data.is_some() { 1 } else { 2 };
        nodes.push(Node::new(name.as_bytes(), parent, data, size, nlink));
    }
    for i in 0..nodes.len() {
        let mut children = core::mem::take(&mut nodes[i].children);
        children.sort_by_key(|c| nodes[*c].name);
        if nodes[i].data.is_none() {
            nodes[i].size = dir_size(&names(&nodes, i, &children));
        }
        nodes[i].children = children;
    }

    // The inode area: each inode, its attributes and its inline tail inside one block.
    let mut cursor = (SUPERBLOCK_AT + SUPERBLOCK_LEN).next_multiple_of(SLOT);
    for node in &mut nodes {
        node.extended = node.size > u64::from(u32::MAX) || node.nlink > u32::from(u16::MAX);
        let head = if node.extended { EXTENDED } else { COMPACT } + node.data.map_or(0, |_| XATTRS);
        let tail = (node.size % BLOCK as u64) as usize;
        node.inline = tail > 0 && head + tail <= BLOCK;
        let len = head + if node.inline { tail } else { 0 };
        if cursor % BLOCK + len > BLOCK {
            cursor = cursor.next_multiple_of(BLOCK);
        }
        node.at = cursor;
        cursor += len.next_multiple_of(SLOT);
    }
    // The data: every whole block, in the tree's order.
    let mut next = (cursor.div_ceil(BLOCK)) as u64;
    for node in &mut nodes {
        let blocks = if node.inline { node.size / BLOCK as u64 } else { node.size.div_ceil(BLOCK as u64) };
        node.start = match (blocks, node.inline) {
            (0, true) => NULL_BLOCK,
            (0, false) => 0,
            _ => u32::try_from(next).map_err(|_| refused("", "too many blocks"))?,
        };
        next += blocks;
    }
    let blocks =
        u32::try_from(next).ok().filter(|b| *b < NULL_BLOCK).ok_or_else(|| refused("", "too many blocks"))?;
    let mut image = vec![0u8; blocks as usize * BLOCK];

    superblock(
        &mut image[SUPERBLOCK_AT..SUPERBLOCK_AT + SUPERBLOCK_LEN],
        nodes[0].nid(),
        nodes.len(),
        blocks,
    );
    for (i, node) in nodes.iter().enumerate() {
        let entries;
        let data = match node.data {
            Some(data) => data,
            None => {
                entries = dir_blocks(&nodes, i);
                &entries[..]
            }
        };
        let mut at = inode(&mut image, node, i as u32 + 1);
        if let Some(file) = node.data {
            xattrs(&mut image[at..at + XATTRS], &sha256(file));
            at += XATTRS;
        }
        let whole = if node.inline { data.len() / BLOCK * BLOCK } else { data.len() };
        if whole > 0 {
            let start = node.start as usize * BLOCK;
            image[start..start + whole].copy_from_slice(&data[..whole]);
        }
        if node.inline {
            image[at..at + data.len() - whole].copy_from_slice(&data[whole..]);
        }
    }
    Ok(image)
}

/// Directory `dir`'s entries, `.` and `..` among them, in name order: each name and the node it
/// names.
fn names<'a>(nodes: &[Node<'a>], dir: usize, children: &[usize]) -> Vec<(&'a [u8], usize)> {
    let mut names = vec![(&b"."[..], dir), (&b".."[..], nodes[dir].parent)];
    names.extend(children.iter().map(|c| (nodes[*c].name, *c)));
    names.sort_by_key(|(name, _)| *name);
    names
}

/// The entries each block holds, as ranges of `names`, filling each block in turn.
fn groups(names: &[(&[u8], usize)]) -> Vec<(usize, usize)> {
    let (mut groups, mut first, mut used) = (Vec::new(), 0, 0);
    for (i, (name, _)) in names.iter().enumerate() {
        if used + DIRENT + name.len() > BLOCK {
            groups.push((first, i));
            (first, used) = (i, 0);
        }
        used += DIRENT + name.len();
    }
    groups.push((first, names.len()));
    groups
}

/// A directory's size: its whole blocks, and the bytes the last one uses.
fn dir_size(names: &[(&[u8], usize)]) -> u64 {
    let groups = groups(names);
    let (first, end) = groups[groups.len() - 1];
    let last: usize = names[first..end].iter().map(|(name, _)| DIRENT + name.len()).sum();
    ((groups.len() - 1) * BLOCK + last) as u64
}

/// Directory `dir`'s entry blocks, the last cut to what it uses: in each, the entries, then
/// their names.
fn dir_blocks(nodes: &[Node<'_>], dir: usize) -> Vec<u8> {
    let names = names(nodes, dir, &nodes[dir].children);
    let mut out = Vec::new();
    for (first, end) in groups(&names) {
        out.resize(out.len().next_multiple_of(BLOCK), 0);
        let base = out.len();
        let mut name_at = (end - first) * DIRENT;
        out.resize(base + name_at, 0);
        for (k, (name, node)) in names[first..end].iter().enumerate() {
            let entry = &mut out[base + k * DIRENT..base + (k + 1) * DIRENT];
            put(entry, dirent::NID, &nodes[*node].nid().to_le_bytes());
            put(entry, dirent::NAME, &(name_at as u16).to_le_bytes());
            entry[dirent::TYPE] = if nodes[*node].data.is_some() { FT_FILE } else { FT_DIR };
            name_at += name.len();
        }
        for (name, _) in &names[first..end] {
            out.extend_from_slice(name);
        }
    }
    out
}

/// The superblock: no feature, compatible or not, and a timestamp of 0.
fn superblock(sb: &mut [u8], root: u64, inodes: usize, blocks: u32) {
    put(sb, field_sb::MAGIC, &MAGIC.to_le_bytes());
    sb[field_sb::BLOCK_BITS] = BLOCK_BITS;
    put(sb, field_sb::ROOT, &(root as u16).to_le_bytes());
    put(sb, field_sb::INODES, &(inodes as u64).to_le_bytes());
    put(sb, field_sb::BLOCKS, &blocks.to_le_bytes());
}

/// Writes `node`'s inode, numbered `ino`, and returns where its attributes go.
fn inode(image: &mut [u8], node: &Node<'_>, ino: u32) -> usize {
    let layout = if node.inline { LAYOUT_INLINE } else { LAYOUT_PLAIN };
    let (mode, xattrs) = match node.data {
        Some(_) => (S_IFREG | 0o644, XATTR_COUNT),
        None => (S_IFDIR | 0o755, 0),
    };
    let len = if node.extended { EXTENDED } else { COMPACT };
    let i = &mut image[node.at..node.at + len];
    put(i, field_inode::FORMAT, &(layout << 1 | u16::from(node.extended)).to_le_bytes());
    put(i, field_inode::XATTR_COUNT, &xattrs.to_le_bytes());
    put(i, field_inode::MODE, &mode.to_le_bytes());
    put(i, field_inode::START, &node.start.to_le_bytes());
    put(i, field_inode::INO, &ino.to_le_bytes());
    if node.extended {
        put(i, field_inode::SIZE, &node.size.to_le_bytes());
        put(i, field_inode::NLINK_EXTENDED, &node.nlink.to_le_bytes());
    } else {
        put(i, field_inode::NLINK, &(node.nlink as u16).to_le_bytes());
        put(i, field_inode::SIZE, &(node.size as u32).to_le_bytes());
    }
    node.at + len
}

/// A file's attribute area: a header with no shared attributes, and `user.sha256`.
fn xattrs(area: &mut [u8], digest: &[u8; 32]) {
    let entry = &mut area[XATTR_HEADER..];
    entry[0] = XATTR_NAME.len() as u8;
    entry[1] = XATTR_USER;
    entry[2..4].copy_from_slice(&32u16.to_le_bytes());
    entry[4..4 + XATTR_NAME.len()].copy_from_slice(XATTR_NAME);
    entry[4 + XATTR_NAME.len()..4 + XATTR_NAME.len() + 32].copy_from_slice(digest);
}
