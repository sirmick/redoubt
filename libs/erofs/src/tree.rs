//! A whole volume read back from its bytes, on the host: what the tests, the fuzz target and the
//! bench's checks compare with the tree that was packed.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{BLOCK, Corrupt, Dirents, EXTENDED, Inode, Kind, Superblock};

/// A volume's files and directories: each path relative to the root, and a file's bytes (`None`
/// for a directory).
pub type Tree = Vec<(String, Option<Vec<u8>>)>;

/// Every file and directory of `image` under its root, parents first and each directory's
/// entries in name order: each path relative to the root, and each file's bytes. A directory
/// reached twice (a cycle, or one directory under two names) is corrupt, so the walk ends.
pub fn read_tree(image: &[u8]) -> Result<Tree, Corrupt> {
    let sb = Superblock::parse(image, (image.len() / BLOCK) as u64)?;
    let (mut out, mut seen) = (Vec::new(), Vec::new());
    let root = inode(image, &sb, sb.root)?;
    if root.kind() != Kind::Dir {
        return Err(Corrupt);
    }
    walk(image, &sb, root, "", &mut seen, &mut out)?;
    Ok(out)
}

fn inode(image: &[u8], sb: &Superblock, nid: u64) -> Result<Inode, Corrupt> {
    let at = sb.inode_at(nid)? as usize;
    Inode::parse(sb, nid, &image[at..image.len().min(at + EXTENDED)])
}

/// `inode`'s data from `offset`, `len` bytes of it, piece by piece as it lies.
fn data(image: &[u8], inode: &Inode, mut offset: u64, len: u64) -> Vec<u8> {
    let (mut out, end) = (Vec::new(), offset + len);
    while let Some((at, run)) = inode.extent(offset).filter(|_| offset < end) {
        let n = run.min(end - offset);
        out.extend_from_slice(&image[at as usize..(at + n) as usize]);
        offset += n;
    }
    out
}

fn walk(
    image: &[u8],
    sb: &Superblock,
    dir: Inode,
    prefix: &str,
    seen: &mut Vec<u64>,
    out: &mut Tree,
) -> Result<(), Corrupt> {
    if seen.contains(&dir.nid()) {
        return Err(Corrupt);
    }
    seen.push(dir.nid());
    for index in 0..dir.dir_blocks() {
        let (_, len) = dir.dir_block(index).ok_or(Corrupt)?;
        let block = data(image, &dir, index * BLOCK as u64, len as u64);
        for entry in Dirents::parse(&block)?.iter() {
            if entry.name == b"." || entry.name == b".." {
                continue;
            }
            let name = core::str::from_utf8(entry.name).map_err(|_| Corrupt)?;
            let mut path = String::from(prefix);
            path.push_str(name);
            let child = inode(image, sb, entry.nid)?;
            match child.kind() {
                Kind::File => out.push((path, Some(data(image, &child, 0, child.size())))),
                Kind::Dir => {
                    out.push((path.clone(), None));
                    path.push('/');
                    walk(image, sb, child, &path, seen, out)?;
                }
            }
        }
    }
    Ok(())
}
