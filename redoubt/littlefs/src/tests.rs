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

/// Red team: a pending move naming the superblock entry must not delete it.
#[test]
fn pending_move_on_the_superblock_entry_is_refused() {
    let mut ram = forged("/d", |fs, _| {
        let g = GState { tag: mk(TYPE_DELETE, 0, 0), pair: fs.root };
        vec![(mk(TYPE_MOVESTATE, ID_NONE, 12), g.to_bytes().to_vec())]
    });
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    assert_eq!(fs.mkdir("/x"), Err(Error::Corrupt));
    drop(fs);
    assert!(Filesystem::mount(&mut ram, CFG).is_ok(), "the superblock survived");
}

/// Red team: names no path can name, forged into a pair, are found by the check.
#[test]
fn unnameable_names_fail_the_check() {
    for name in [&b""[..], b".", b"..", b"x/y", b"a\0b"] {
        let mut ram = forged("/f", |_, id| {
            let i = id + 1;
            vec![
                crate::fs::attr_create(i),
                crate::fs::attr_name(TYPE_REG, i, name).unwrap(),
                attr_struct(TYPE_INLINESTRUCT, i, b"hidden").unwrap(),
            ]
        });
        let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
        assert_eq!(fs.check(), Err(Error::Corrupt), "name {name:?}");
        assert_eq!(fs.fsck(), Err(Error::Corrupt), "name {name:?}");
    }
}

/// Red team: many files claiming the whole volume through a self-pointing skip-list would
/// make every allocation walk files x blocks. The walk is bounded, so it fails fast.
#[test]
fn forged_file_sizes_do_not_amplify_allocation() {
    struct Counting(Ram, u64);
    impl BlockDevice for Counting {
        fn read(&mut self, b: u32, o: u32, buf: &mut [u8]) -> Result<(), Error> {
            self.1 += 1;
            self.0.read(b, o, buf)
        }

        fn prog(&mut self, b: u32, o: u32, d: &[u8]) -> Result<(), Error> { self.0.prog(b, o, d) }

        fn erase(&mut self, b: u32) -> Result<(), Error> { self.0.erase(b) }

        fn sync(&mut self) -> Result<(), Error> { Ok(()) }
    }
    const BIG: Config = Config { block_size: 256, block_count: 4096, prog_size: 16 };
    let mut dev = Counting(Ram(vec![0xff; 256 * 4096], 256), 0);
    Filesystem::format(&mut dev, BIG).unwrap();
    let mut fs = Filesystem::mount(&mut dev, BIG).unwrap();
    for i in 0..200 {
        write_all(&mut fs, &std::format!("/f{i:03}"), &[7; 300]);
    }
    let mut heads = Vec::new();
    for i in 0..200 {
        let (crate::ops::Lookup::Found { dir, id }, _) = fs.lookup(&std::format!("/f{i:03}")).unwrap() else { panic!() };
        let crate::fs::Struct::Ctz { head, .. } = fs.decode(&dir.c.entries[id as usize]).unwrap() else { panic!() };
        let ctz = [head.to_le_bytes(), (256u32 * 4096).to_le_bytes()].concat();
        fs.commit(dir.pair, &[attr_struct(TYPE_CTZSTRUCT, id, &ctz).unwrap()]).unwrap();
        heads.push(head);
    }
    drop(fs);
    for h in heads {
        let at = (h * 256) as usize;
        dev.0 .0[at..at + 4].copy_from_slice(&h.to_le_bytes());
        dev.0 .0[at + 4..at + 8].copy_from_slice(&h.to_le_bytes());
    }
    let mut fs = Filesystem::mount(&mut dev, BIG).unwrap();
    assert_eq!(fs.mkdir("/new"), Err(Error::Corrupt));
    drop(fs);
    // Unbounded, one allocation scan read each forged file's whole claimed length (over
    // 800k reads); bounded, a few blocks per pair and at most 3 x 4096 blocks.
    assert!(dev.1 < 100_000, "{} device reads", dev.1);
}

fn write_all<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str, data: &[u8]) {
    let h = fs.open(path, OpenOptions { write: true, create: true, truncate: true, ..Default::default() }).unwrap();
    fs.write(h, data).unwrap();
    fs.close(h).unwrap();
}

fn read_all<D: BlockDevice>(fs: &mut Filesystem<D>, path: &str) -> Vec<u8> {
    let h = fs.open(path, OpenOptions { read: true, ..Default::default() }).unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 1000];
    loop {
        let n = fs.read(h, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    fs.close(h).unwrap();
    out
}

/// The red team's setup for the stale-handle corruption: `/d` split over several pairs with
/// its last pair holding one file (the victim), `/g` with `n` files, the volume full.
/// Returns the victim's path and the pair that will drop when it is removed.
fn drop_scenario(ram: &mut Ram, n: usize) -> (std::string::String, crate::mdir::Pair) {
    const C: Config = Config { block_size: 256, block_count: 96, prog_size: 16 };
    Filesystem::format(&mut *ram, C).unwrap();
    let mut fs = Filesystem::mount(ram, C).unwrap();
    fs.mkdir("/d").unwrap();
    for i in 0..40 {
        write_all(&mut fs, &std::format!("/d/f{i:02}"), b"");
    }
    let (crate::ops::Lookup::Found { dir, id }, _) = fs.lookup("/d").unwrap() else { panic!() };
    let crate::fs::Struct::Dir(mut p) = fs.decode(&dir.c.entries[id as usize]).unwrap() else { panic!() };
    let last = loop {
        let d = fs.fetch(p).unwrap();
        if !d.c.split {
            break d;
        }
        p = d.c.tail;
    };
    let names: Vec<std::string::String> =
        last.c.entries.iter().map(|e| std::string::String::from_utf8(e.name.clone()).unwrap()).collect();
    for nm in &names[1..] {
        fs.remove(&std::format!("/d/{nm}")).unwrap();
    }
    fs.mkdir("/g").unwrap();
    for i in 0..n {
        write_all(&mut fs, &std::format!("/g/h{i:02}"), std::format!("g{i:02}").as_bytes());
    }
    let h = fs.open("/big", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    while fs.write(h, &[1u8; 200]).is_ok() {}
    let _ = fs.close(h);
    (std::format!("/d/{}", names[0]), last.pair)
}

const C96: Config = Config { block_size: 256, block_count: 96, prog_size: 16 };

/// Red team (high): a file alone in a later pair of a split directory is removed while a
/// write handle is open; the pair drops, the next mkdir takes its blocks, and the handle's
/// close used to commit its data into the new directory's first entry.
#[test]
fn stale_handle_after_pair_drop_does_not_touch_another_file() {
    let mut ram = Ram(vec![0xff; 256 * 96], 256);
    let (victim, dropped) = drop_scenario(&mut ram, 0);
    let mut fs = Filesystem::mount(&mut ram, C96).unwrap();
    let h = fs.open(&victim, OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
    fs.remove(&victim).unwrap();
    fs.mkdir("/e").unwrap();
    let (crate::ops::Lookup::Found { dir, id }, _) = fs.lookup("/e").unwrap() else { panic!() };
    let crate::fs::Struct::Dir(p) = fs.decode(&dir.c.entries[id as usize]).unwrap() else { panic!() };
    assert!(crate::mdir::pair_overlaps(&p, &dropped), "the scenario needs the dropped pair reused");
    write_all(&mut fs, "/e/x", b"hello");
    fs.write(h, b"ZZZZZ").unwrap();
    assert_eq!(fs.close(h), Ok(()), "a removed file's handle commits nothing");
    assert_eq!(read_all(&mut fs, "/e/x"), b"hello");
    fs.fsck().unwrap();
}

/// Red team (high), second route: the dropped pair's blocks become another file's data,
/// which the stale handle's sync used to erase.
#[test]
fn stale_handle_after_pair_drop_does_not_erase_another_files_data() {
    for k in 0..10u8 {
        let mut ram = Ram(vec![0xff; 256 * 96], 256);
        let (victim, _) = drop_scenario(&mut ram, 2);
        let mut fs = Filesystem::mount(&mut ram, C96).unwrap();
        for j in 0..k {
            fs.set_attr(&victim, 1, &[j; 8]).unwrap();
        }
        let h = fs.open(&victim, OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
        fs.remove(&victim).unwrap();
        let data: Vec<u8> = (0..200u8).collect();
        write_all(&mut fs, "/other", &data);
        fs.write(h, b"ZZZZZ").unwrap();
        assert_eq!(fs.close(h), Ok(()));
        assert_eq!(read_all(&mut fs, "/other"), data, "k = {k}");
        fs.fsck().unwrap();
    }
}

/// Red team (medium): after a failed write the handle is errored; its close reports the
/// error and commits nothing (the reference does the same).
#[test]
fn a_failed_write_commits_nothing() {
    const C32: Config = Config { block_size: 256, block_count: 32, prog_size: 16 };
    let mut ram = Ram(vec![0xff; 256 * 32], 256);
    Filesystem::format(&mut ram, C32).unwrap();
    let mut fs = Filesystem::mount(&mut ram, C32).unwrap();
    write_all(&mut fs, "/f", &[1u8; 300]);
    let h = fs.open("/f", OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
    fs.seek(h, 100).unwrap();
    assert_eq!(fs.write(h, &[2u8; 20000]), Err(Error::NoSpace));
    assert_eq!(fs.write(h, b"more"), Err(Error::NoSpace), "errored handles stay errored");
    assert_eq!(fs.close(h), Err(Error::NoSpace));
    assert_eq!(read_all(&mut fs, "/f"), [1u8; 300]);
    fs.fsck().unwrap();
}

/// Paths: a trailing slash names a directory; NUL is not a name byte; stale handles fail.
#[test]
fn path_and_handle_rules() {
    let mut ram = Ram(vec![0xff; 256 * 64], 256);
    Filesystem::format(&mut ram, CFG).unwrap();
    let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
    write_all(&mut fs, "/a", b"x");
    fs.mkdir("/d").unwrap();
    assert_eq!(fs.stat("/a/"), Err(Error::NotDir));
    assert!(fs.stat("/d/").is_ok());
    let w = OpenOptions { write: true, create: true, ..Default::default() };
    assert_eq!(fs.open("/new/", w), Err(Error::NotDir));
    assert_eq!(fs.rename("/a", "/b/"), Err(Error::NotDir));
    assert_eq!(fs.open("/a\0b", w), Err(Error::Invalid));
    assert_eq!(fs.mkdir("/a\0b"), Err(Error::Invalid));
    let h = fs.open("/a", OpenOptions { read: true, ..Default::default() }).unwrap();
    fs.close(h).unwrap();
    let h2 = fs.open("/a", OpenOptions { read: true, ..Default::default() }).unwrap();
    assert_eq!(fs.read(h, &mut [0; 4]), Err(Error::Invalid), "a closed handle's slot was reused");
    fs.close(h2).unwrap();
}
