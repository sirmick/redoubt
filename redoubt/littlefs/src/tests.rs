//! Hostile structures forged with valid checksums: the filesystem's own commit path writes
//! metadata a correct writer never would, and every reader must refuse it cleanly and finish.

extern crate std;

use alloc::vec;
use alloc::vec::Vec;

use crate::fs::{attr_struct, attr_tail, pair_bytes, Attr};
use crate::mdir::GState;
use crate::tag::*;
use crate::{BlockDevice, Config, Error, Filesystem, OpenOptions};

struct Ram(Vec<u8>, u32);

impl BlockDevice for Ram {
    fn read(&mut self, block: u32, off: u32, buf: &mut [u8]) -> Result<(), Error> {
        let at = (block * self.1 + off) as usize;
        buf.copy_from_slice(&self.0[at..at + buf.len()]);
        Ok(())
    }

    fn prog(&mut self, block: u32, off: u32, data: &[u8]) -> Result<(), Error> {
        let at = (block * self.1 + off) as usize;
        self.0[at..at + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn erase(&mut self, block: u32) -> Result<(), Error> {
        let at = (block * self.1) as usize;
        self.0[at..at + self.1 as usize].fill(0xff);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), Error> { Ok(()) }
}

const CFG: Config = Config { block_size: 256, block_count: 64, prog_size: 16 };

/// A volume with `/d` (a directory) and `/f` (a three-block file), then `forge` applied to
/// the pair holding `name`'s entry, as a raw commit.
fn forged(name: &str, forge: impl FnOnce(&mut Filesystem<&mut Ram>, u16) -> Vec<Attr>) -> Ram {
    let mut ram = Ram(vec![0xff; 256 * 64], 256);
    Filesystem::format(&mut ram, CFG).unwrap();
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    fs.mkdir("/d").unwrap();
    let h = fs.open("/f", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    fs.write(h, &[7; 600]).unwrap();
    fs.close(h).unwrap();
    let (crate::ops::Lookup::Found { dir, id }, _) = fs.lookup(name).unwrap() else { panic!() };
    let attrs = forge(&mut fs, id);
    fs.commit(dir.pair, &attrs).unwrap();
    drop(fs);
    ram
}

#[test]
fn tail_list_cycle_is_refused() {
    let mut ram = forged("/d", |fs, _| vec![attr_tail(false, fs.root)]);
    assert!(matches!(Filesystem::mount(&mut ram, CFG), Err(Error::Corrupt)));
}

#[test]
fn directory_chain_cycle_is_refused() {
    // The root continues into itself: a hard tail back to its own pair.
    let mut ram = forged("/d", |fs, _| vec![attr_tail(true, fs.root)]);
    assert!(Filesystem::mount(&mut ram, CFG).is_err());
}

#[test]
fn directory_inside_itself_is_found_by_fsck() {
    let mut ram = forged("/d", |fs, id| vec![attr_struct(TYPE_DIRSTRUCT, id, &pair_bytes(fs.root)).unwrap()]);
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    // Lookups stay bounded by the path; walking the tree is the caller's business.
    assert!(fs.stat("/d/d/d/d/d/f").is_ok());
    assert_eq!(fs.fsck(), Err(Error::Corrupt));
}

#[test]
fn file_larger_than_the_volume_is_refused() {
    let mut ram = forged("/f", |_, id| {
        let ctz = [5u32.to_le_bytes(), 0x7fff_0000u32.to_le_bytes()].concat();
        vec![attr_struct(TYPE_CTZSTRUCT, id, &ctz).unwrap()]
    });
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    assert_eq!(fs.stat("/f"), Err(Error::Corrupt));
    assert_eq!(fs.open("/f", OpenOptions { read: true, ..Default::default() }), Err(Error::Corrupt));
    // Allocation walks every file, so writing fails closed too.
    assert!(fs.mkdir("/x").is_err());
}

#[test]
fn file_head_outside_the_volume_is_refused() {
    let mut ram = forged("/f", |_, id| {
        let ctz = [1000u32.to_le_bytes(), 600u32.to_le_bytes()].concat();
        vec![attr_struct(TYPE_CTZSTRUCT, id, &ctz).unwrap()]
    });
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    assert_eq!(fs.stat("/f"), Err(Error::Corrupt));
}

#[test]
fn skip_list_pointing_at_itself_terminates() {
    let mut ram = forged("/f", |_, _| Vec::new());
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    let (crate::ops::Lookup::Found { dir, id }, _) = fs.lookup("/f").unwrap() else { panic!() };
    let crate::fs::Struct::Ctz { head, .. } = fs.decode(&dir.c.entries[id as usize]).unwrap() else { panic!() };
    drop(fs);
    // The head block's first pointer now points at the head itself.
    let at = (head * 256) as usize;
    ram.0[at..at + 4].copy_from_slice(&head.to_le_bytes());
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    let h = fs.open("/f", OpenOptions { read: true, ..Default::default() }).unwrap();
    let mut buf = [0u8; 600];
    let _ = fs.read(h, &mut buf);
    assert_eq!(fs.fsck(), Err(Error::Corrupt));
}

#[test]
fn bogus_pending_move_fails_writes_not_reads() {
    let mut ram = forged("/d", |fs, _| {
        // A pending move naming an id the pair does not have.
        let g = GState { tag: mk(TYPE_DELETE, 77, 0), pair: fs.root };
        vec![(mk(TYPE_MOVESTATE, ID_NONE, 12), g.to_bytes().to_vec())]
    });
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    assert!(fs.stat("/d").is_ok());
    assert_eq!(fs.mkdir("/x"), Err(Error::Corrupt));
}
