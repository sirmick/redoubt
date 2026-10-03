//! Packing a volume on the host: a tree of files written through `fsd`'s own code into the bytes
//! of a range, for a disk image (image/disk.toml).
//!
//! **One writer.** Every directory and file is created and written through [`Fsd`]'s own
//! `create` and `write`, the code that serves the volume, so every entry carries its id and the
//! id counter is above the highest, and the packed volume is one `fsd` mounts as its own
//! (servers/fsd.md, "Volumes, connections and labels"). A second writer would have to keep that
//! rule in step by hand.

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{DMDIR, FileServer};

use crate::server::{Fsd, Node};
use crate::volume::{Fault, Geometry, Mounted, Range, SECTOR, mount};

/// One entry of the tree, by its path from the volume's root (`a/b`, no leading `/`). A
/// directory comes before anything in it.
#[derive(Clone, Copy, Debug)]
pub enum Entry<'a> {
    Dir(&'a str),
    File(&'a str, &'a [u8]),
}

/// Why a tree was not packed: the path, and what `fsd` answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackError {
    pub path: String,
    pub why: String,
}

/// A range held in memory.
#[derive(Clone)]
struct Memory(Rc<RefCell<Vec<u8>>>);

impl Memory {
    fn span(&self, sector: u64, len: usize) -> Result<core::ops::Range<usize>, Fault> {
        let start = usize::try_from(sector).ok().and_then(|s| s.checked_mul(SECTOR as usize)).ok_or(Fault)?;
        let end = start.checked_add(len).ok_or(Fault)?;
        if end > self.0.borrow().len() || !len.is_multiple_of(SECTOR as usize) {
            return Err(Fault);
        }
        Ok(start..end)
    }
}

impl Range for Memory {
    fn info(&mut self) -> Result<Geometry, Fault> {
        Ok(Geometry { sectors: (self.0.borrow().len() / SECTOR as usize) as u64, read_only: false })
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        let span = self.span(sector, out.len())?;
        out.copy_from_slice(&self.0.borrow()[span]);
        Ok(())
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), Fault> {
        let span = self.span(sector, data.len())?;
        self.0.borrow_mut()[span].copy_from_slice(data);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Fault> { Ok(()) }
}

/// A volume of `sectors` sectors holding `tree`, formatted and written by `fsd`'s own code.
pub fn pack(sectors: u64, tree: &[Entry]) -> Result<Vec<u8>, PackError> {
    let fail = |path: &str, why: &str| PackError { path: path.into(), why: why.into() };
    let len = usize::try_from(sectors).ok().and_then(|s| s.checked_mul(SECTOR as usize));
    let memory = Memory(Rc::new(RefCell::new(vec![0; len.ok_or_else(|| fail("", "too large"))?])));
    let mounted = mount(memory.clone()).map_err(|e| fail("", &alloc::format!("{e:?}")))?;
    if let Mounted::Corrupt(e) = mounted {
        return Err(fail("", &alloc::format!("{e:?}")));
    }
    let mut fsd = Fsd::new(mounted, Vec::new());
    let packer = Caller { badge: 1, account: 0, labels: Labels::new() };
    let (root, _) = fsd.attach(&packer, "").map_err(|e| fail("", e.0))?;
    for entry in tree {
        let (path, perm, data) = match *entry {
            Entry::Dir(path) => (path, DMDIR | 0o755, None),
            Entry::File(path, data) => (path, 0o644, Some(data)),
        };
        let at = |e: redoubt_rt::server::ninep::NineError| fail(path, e.0);
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut parent: Node = root.clone();
        for step in dir.split('/').filter(|s| !s.is_empty()) {
            parent = fsd.walk(&packer, &parent, step).map_err(at)?.0;
        }
        let (node, _) = fsd.create(&packer, &parent, name, perm, 0).map_err(at)?;
        if let Some(data) = data.filter(|d| !d.is_empty()) {
            let written = fsd.write(&packer, &node, 0, data).map_err(at)?;
            if written != data.len() {
                return Err(fail(path, "short write"));
            }
        }
    }
    drop(fsd);
    Ok(memory.0.take())
}

#[cfg(test)]
mod tests {
    use redoubt_rt::server::ninep::mode;

    use super::*;
    use crate::server::tests::{Memory as Disk, T, caller};

    /// A packed tree mounts in `fsd` as a volume it wrote (the mount's id check passes), reads
    /// back whole, and takes a new file whose id is above every packed one.
    #[test]
    fn a_packed_tree_mounts_and_reads_back_in_fsd() {
        let big: Vec<u8> = (0..20_000u32).map(|i| i as u8).collect();
        let tree = [
            Entry::Dir("etc"),
            Entry::File("etc/motd", b"hello\n"),
            Entry::Dir("etc/empty"),
            Entry::File("big", &big),
            Entry::File("nothing", b""),
        ];
        let bytes = pack(64 * 8, &tree).expect("packed");
        let mut t = T::on(&Disk::holding(bytes), &[]);
        let who = caller(1, &[]);
        t.attach(&who, 0).unwrap();
        t.walk(&who, 0, 5, &[]).unwrap();
        t.open(&who, 5, mode::OREAD).unwrap();
        let mut list = t.list(&who, 5).unwrap();
        list.sort();
        assert_eq!(list, ["big", "etc", "nothing"]);
        let qids = t.walk(&who, 0, 1, &["etc", "motd"]).unwrap();
        t.open(&who, 1, mode::OREAD).unwrap();
        assert_eq!(t.read(&who, 1, 0, 100).unwrap(), b"hello\n");
        t.walk(&who, 0, 2, &["big"]).unwrap();
        t.open(&who, 2, mode::OREAD).unwrap();
        let mut back = Vec::new();
        while back.len() < big.len() {
            let chunk = t.read(&who, 2, back.len() as u64, 4096).unwrap();
            assert!(!chunk.is_empty());
            back.extend(chunk);
        }
        assert_eq!(back, big);
        t.walk(&who, 0, 3, &["etc"]).unwrap();
        t.create(&who, 3, "new", 0o644, mode::OWRITE).unwrap();
        let new = t.walk(&who, 0, 4, &["etc", "new"]).unwrap();
        assert!(new[1] > qids[1], "the counter is above every packed id");
    }

    #[test]
    fn a_tree_whose_parent_is_missing_is_refused() {
        assert_eq!(pack(64 * 8, &[Entry::File("no/such", b"x")]).unwrap_err().path, "no/such");
        assert!(pack(64 * 8, &[Entry::Dir("a"), Entry::Dir("a")]).is_err());
    }
}
