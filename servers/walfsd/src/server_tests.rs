//! `walfsd`'s files through the 9P skeleton, as a client sends them, on a range in memory standing
//! in for `blkd`'s. The whole program against a fake kernel and a fake `blkd`, and the client
//! library against it, are in `tests/`.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use redoubt_rt::abi::Labels;
use redoubt_rt::server::ninep::{Answer, NineServer, mode};
use redoubt_rt::wire::MSIZE;
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names};

use super::*;
use crate::volume::{Fault, Geometry, MIN_BLOCKS, NoVolume, SECTOR, SECTORS_PER_BLOCK, mount};

extern crate std;
use std::string::String as StdString;

/// A range in memory, sparse: a sector never written reads as zero. Clones share the sectors,
/// so a test can look at or break the disk under a running server.
#[derive(Clone)]
pub(crate) struct Memory(pub Rc<RefCell<Disk>>);

pub(crate) struct Disk {
    /// The range's length in sectors.
    pub len: u64,
    /// The sectors that hold anything but zeros.
    pub sectors: BTreeMap<u64, Vec<u8>>,
    /// Every request fails from now on.
    pub failing: bool,
    /// Writes are refused, as `blkd` refuses them on a read-only range.
    pub read_only: bool,
    /// Writes that reached the disk.
    pub writes: usize,
    /// Power fails at this write: it and every later one are lost.
    pub fail_at: Option<usize>,
    /// Reads that reached the disk: one per block.
    pub reads: usize,
    /// Flushes that reached the disk.
    pub flushes: usize,
}

const S: usize = SECTOR as usize;

impl Memory {
    pub fn blank(sectors: usize) -> Memory {
        Memory(Rc::new(RefCell::new(Disk {
            len: sectors as u64,
            sectors: BTreeMap::new(),
            failing: false,
            read_only: false,
            writes: 0,
            fail_at: None,
            reads: 0,
            flushes: 0,
        })))
    }

    pub fn holding(bytes: Vec<u8>) -> Memory {
        let disk = Memory::blank(bytes.len() / S);
        disk.put(0, &bytes);
        disk
    }

    pub fn fail(&self) { self.0.borrow_mut().failing = true }

    /// The same bytes, now a read-only range.
    pub fn read_only(self) -> Memory {
        self.0.borrow_mut().read_only = true;
        self
    }

    /// The whole range, for a small one.
    pub fn bytes(&self) -> Vec<u8> { self.get(0, self.0.borrow().len as usize * S) }

    /// A copy of the disk as it is now, sharing nothing.
    pub fn copy(&self) -> Memory { Memory::holding(self.bytes()) }

    fn get(&self, sector: u64, len: usize) -> Vec<u8> {
        let disk = self.0.borrow();
        let mut out = vec![0; len];
        for (i, chunk) in out.chunks_mut(S).enumerate() {
            if let Some(data) = disk.sectors.get(&(sector + i as u64)) {
                chunk.copy_from_slice(data);
            }
        }
        out
    }

    pub fn put(&self, sector: u64, data: &[u8]) {
        let mut disk = self.0.borrow_mut();
        for (i, chunk) in data.chunks(S).enumerate() {
            if chunk.iter().all(|b| *b == 0) {
                disk.sectors.remove(&(sector + i as u64));
            } else {
                disk.sectors.insert(sector + i as u64, chunk.to_vec());
            }
        }
    }

    fn check(&self, sector: u64, len: usize) -> Result<(), Fault> {
        let disk = self.0.borrow();
        let end = sector.checked_add((len / S) as u64).ok_or(Fault)?;
        if disk.failing || !len.is_multiple_of(S) || end > disk.len { Err(Fault) } else { Ok(()) }
    }
}

impl Range for Memory {
    fn info(&mut self) -> Result<Geometry, Fault> {
        let disk = self.0.borrow();
        if disk.failing {
            return Err(Fault);
        }
        Ok(Geometry { sectors: disk.len, read_only: disk.read_only })
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.0.borrow_mut().reads += 1;
        self.check(sector, out.len())?;
        out.copy_from_slice(&self.get(sector, out.len()));
        Ok(())
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), Fault> {
        self.check(sector, data.len())?;
        let mut disk = self.0.borrow_mut();
        if disk.read_only || disk.fail_at.is_some_and(|at| disk.writes + 1 >= at) {
            return Err(Fault);
        }
        disk.writes += 1;
        drop(disk);
        self.put(sector, data);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Fault> {
        self.check(0, 0)?;
        self.0.borrow_mut().flushes += 1;
        Ok(())
    }
}

/// A volume of 1024 blocks (4 MiB), the bench cases' disk.
pub(crate) const SECTORS: usize = 1024 * 8;

pub(crate) fn caller(badge: u64, labels: &[u64]) -> Caller {
    Caller { badge, account: 1001, labels: Labels::from_slice(labels).unwrap() }
}

/// A server on `disk` under `labels`, and the 9P a client would send it.
pub(crate) struct T {
    pub server: NineServer<Walfsd<Memory>>,
    pub buf: Vec<u8>,
}

impl T {
    pub fn on(disk: &Memory, labels: &[u64]) -> T {
        let mounted = mount(disk.clone()).expect("a volume");
        let server = NineServer::new(Walfsd::new(mounted, labels.to_vec()), limits(4), 0).unwrap();
        T { server, buf: vec![0; MSIZE] }
    }

    pub fn rpc(&mut self, who: &Caller, body: Body<'_>) -> Body<'_> {
        self.buf = vec![0; MSIZE];
        Message { tag: 9, body }.encode(&mut self.buf).unwrap();
        assert_eq!(self.server.answer_in_place(who, &mut self.buf), Answer::Replied);
        let reply = Message::decode(&self.buf).unwrap();
        assert_eq!(reply.tag, 9);
        reply.body
    }

    /// `Ok` for any R-message but `Rerror`, whose text is the error.
    pub fn try_rpc(&mut self, who: &Caller, body: Body<'_>) -> Result<(), StdString> {
        match self.rpc(who, body) {
            Body::Rerror { ename } => Err(ename.into()),
            _ => Ok(()),
        }
    }

    pub fn attach(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tattach { fid, afid: NOFID, uname: "", aname: "" })
    }

    pub fn walk(
        &mut self,
        who: &Caller,
        fid: u32,
        newfid: u32,
        names: &[&str],
    ) -> Result<Vec<u64>, StdString> {
        match self.rpc(who, Body::Twalk { fid, newfid, wnames: Names::new(names).unwrap() }) {
            Body::Rwalk { qids } if qids.as_slice().len() == names.len() => {
                Ok(qids.as_slice().iter().map(|q| q.path).collect())
            }
            Body::Rerror { ename } => Err(ename.into()),
            // Walked part of the way: the fid was not made (intro(5), walk).
            Body::Rwalk { .. } => Err("partial walk".into()),
            other => panic!("walk {names:?}: {other:?}"),
        }
    }

    pub fn open(&mut self, who: &Caller, fid: u32, m: u8) -> Result<(), StdString> {
        self.try_rpc(who, Body::Topen { fid, mode: m })
    }

    pub fn create(&mut self, who: &Caller, fid: u32, name: &str, perm: u32, m: u8) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tcreate { fid, name, perm, mode: m })
    }

    pub fn read(&mut self, who: &Caller, fid: u32, offset: u64, count: u32) -> Result<Vec<u8>, StdString> {
        match self.rpc(who, Body::Tread { fid, offset, count }) {
            Body::Rread { data } => Ok(data.to_vec()),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    pub fn write(&mut self, who: &Caller, fid: u32, offset: u64, data: &[u8]) -> Result<u32, StdString> {
        match self.rpc(who, Body::Twrite { fid, offset, data }) {
            Body::Rwrite { count } => Ok(count),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    /// The name, length and qid (path, version) `Tstat` reports.
    pub fn stat(&mut self, who: &Caller, fid: u32) -> Result<(StdString, u64, u64, u32), StdString> {
        match self.rpc(who, Body::Tstat { fid }) {
            Body::Rstat { stat } => Ok((stat.name.into(), stat.length, stat.qid.path, stat.qid.version)),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    pub fn clunk(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tclunk { fid })
    }

    pub fn remove(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tremove { fid })
    }

    /// The names a directory read of `fid` (opened) lists, `count` bytes a read.
    pub fn list_by(&mut self, who: &Caller, fid: u32, count: u32) -> Result<Vec<StdString>, StdString> {
        let mut names = Vec::new();
        let mut offset = 0;
        loop {
            let chunk = self.read(who, fid, offset, count)?;
            if chunk.is_empty() {
                return Ok(names);
            }
            offset += chunk.len() as u64;
            names.extend(redoubt_rt::wire::ninep::stats(&chunk).map(|s| StdString::from(s.unwrap().name)));
        }
    }

    pub fn list(&mut self, who: &Caller, fid: u32) -> Result<Vec<StdString>, StdString> {
        self.list_by(who, fid, 4096)
    }

    /// Creates the file `name` at the root (fid 0) holding `data`, written over 9P.
    pub fn put(&mut self, who: &Caller, name: &str, data: &[u8]) {
        self.walk(who, 0, 90, &[]).unwrap();
        self.create(who, 90, name, 0o644, mode::OWRITE).unwrap();
        for (i, chunk) in data.chunks(4096).enumerate() {
            assert_eq!(self.write(who, 90, (i * 4096) as u64, chunk), Ok(chunk.len() as u32));
        }
        self.clunk(who, 90).unwrap();
    }

    /// What the file at `names` holds, read through fid 91.
    pub fn get(&mut self, who: &Caller, names: &[&str]) -> Result<Vec<u8>, StdString> {
        self.walk(who, 0, 91, names)?;
        let got = self.open(who, 91, mode::OREAD).and_then(|()| self.read(who, 91, 0, 64 * 1024));
        self.clunk(who, 91).unwrap();
        got
    }
}

/// A fresh volume with `notes` holding `data`, written over 9P; fid 0 is the root.
pub(crate) fn with_notes(t: &mut T, who: &Caller, data: &[u8]) {
    t.attach(who, 0).unwrap();
    t.put(who, "notes", data);
}

/// The volume on `disk` checked whole: walfs's check finds nothing.
fn sound(disk: &Memory) {
    let Ok(Mounted::Files { mut fs, .. }) = mount(disk.clone()) else { panic!("no volume") };
    assert_eq!(fs.check().unwrap(), []);
}

#[test]
fn attach_walk_open_read_write() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let alice = caller(1, &[]);
    with_notes(&mut t, &alice, b"hello, world");
    let qids = t.walk(&alice, 0, 1, &["notes"]).unwrap();
    assert_ne!(qids[0], u64::from(walfs::ROOT), "a file's qid path is its inode, never the root's");
    t.open(&alice, 1, mode::ORDWR).unwrap();
    assert_eq!(t.read(&alice, 1, 0, 100).unwrap(), b"hello, world");
    assert_eq!(t.read(&alice, 1, 7, 3).unwrap(), b"wor");
    assert_eq!(t.write(&alice, 1, 0, b"HELLO"), Ok(5));
    assert_eq!(t.read(&alice, 1, 0, 100).unwrap(), b"HELLO, world");
    // Past the end reads nothing, whatever the offset; past the largest file writes nothing.
    assert_eq!(t.read(&alice, 1, 12, 10).unwrap(), b"");
    assert_eq!(t.read(&alice, 1, u64::MAX - 5, 5).unwrap(), b"");
    assert_eq!(t.write(&alice, 1, walfs::MAX_FILE_SIZE, b"x").unwrap_err(), "file too large");
    assert_eq!(t.stat(&alice, 1).unwrap().0, "notes");
    assert_eq!(t.stat(&alice, 1).unwrap().1, 12);
    assert_eq!(t.walk(&alice, 0, 2, &["missing"]).unwrap_err(), "file does not exist");
    // A create finds a taken name taken, and opens nothing.
    t.walk(&alice, 0, 2, &[]).unwrap();
    assert_eq!(t.create(&alice, 2, "notes", 0o644, mode::OWRITE).unwrap_err(), "file exists");
    t.create(&alice, 2, "d", DMDIR | 0o755, mode::OREAD).unwrap();
    t.walk(&alice, 0, 3, &[]).unwrap();
    t.open(&alice, 3, mode::OREAD).unwrap();
    assert_eq!(t.list(&alice, 3).unwrap(), ["notes", "d"]);
    t.walk(&alice, 0, 4, &["d"]).unwrap();
    assert_eq!(t.remove(&alice, 4), Ok(()));
    sound(&disk);
}

#[test]
fn files_and_directories_survive_a_remount() {
    let disk = Memory::blank(SECTORS);
    let alice = caller(1, &[]);
    let (file, dir) = {
        let mut t = T::on(&disk, &[]);
        with_notes(&mut t, &alice, b"kept");
        t.walk(&alice, 0, 1, &[]).unwrap();
        t.create(&alice, 1, "dir", DMDIR | 0o755, mode::OREAD).unwrap();
        (t.walk(&alice, 0, 2, &["notes"]).unwrap()[0], t.walk(&alice, 0, 3, &["dir"]).unwrap()[0])
    };
    let mut t = T::on(&disk, &[]);
    t.attach(&alice, 0).unwrap();
    assert_eq!(t.walk(&alice, 0, 1, &["notes"]).unwrap(), [file], "the qid path is the inode, on the medium");
    assert_eq!(t.walk(&alice, 0, 2, &["dir"]).unwrap(), [dir]);
    assert_eq!(t.get(&alice, &["notes"]).unwrap(), b"kept");
}

/// The page's attack test, kept by the generation: after a remove, the file's other fids get
/// `removed` on read, write and stat, and only a clunk succeeds; a file made in its place, in the
/// same inode, is not theirs.
#[test]
fn a_removed_files_other_fids_get_removed() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let (alice, bob) = (caller(1, &[]), caller(2, &[]));
    with_notes(&mut t, &alice, b"secret");
    t.attach(&bob, 0).unwrap();
    let (ino, generation) = {
        t.walk(&bob, 0, 1, &["notes"]).unwrap();
        let (_, _, path, version) = t.stat(&bob, 1).unwrap();
        (path, version)
    };
    t.open(&bob, 1, mode::ORDWR).unwrap();
    t.walk(&alice, 0, 1, &["notes"]).unwrap();
    t.remove(&alice, 1).unwrap();
    let gone = |t: &mut T| {
        assert_eq!(t.read(&bob, 1, 0, 10).unwrap_err(), "removed");
        assert_eq!(t.write(&bob, 1, 0, b"x").unwrap_err(), "removed");
        assert_eq!(t.stat(&bob, 1).unwrap_err(), "removed");
    };
    gone(&mut t);
    assert_eq!(t.walk(&bob, 0, 2, &["notes"]).unwrap_err(), "file does not exist");
    // A new file under the old name, in the freed inode at its next generation: inodes are
    // given round the table, so files are made and removed until one takes it.
    for i in 0.. {
        t.put(&alice, "notes", b"new");
        t.walk(&alice, 0, 2, &["notes"]).unwrap();
        let (_, _, path, version) = t.stat(&alice, 2).unwrap();
        t.clunk(&alice, 2).unwrap();
        if path == ino {
            assert_eq!(version, generation + 1);
            break;
        }
        assert!(i < 64, "the inode came round");
        t.walk(&alice, 0, 2, &["notes"]).unwrap();
        t.remove(&alice, 2).unwrap();
    }
    gone(&mut t);
    t.clunk(&bob, 1).unwrap();
    assert_eq!(t.get(&bob, &["notes"]).unwrap(), b"new");
}

/// Labels are the volume's: with `labels=7`, {7} reads and writes, {7, 9} reads but cannot
/// write, and {} reaches nothing, not even the root or a listing.
#[test]
fn the_volumes_labels_are_checked_on_every_request() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[7]);
    let (owner, above, nobody) = (caller(1, &[7]), caller(2, &[7, 9]), caller(3, &[]));
    with_notes(&mut t, &owner, b"labelled");
    t.walk(&owner, 0, 1, &["notes"]).unwrap();
    t.open(&owner, 1, mode::ORDWR).unwrap();
    assert_eq!(t.write(&owner, 1, 0, b"L"), Ok(1));

    t.attach(&above, 0).unwrap();
    t.walk(&above, 0, 1, &["notes"]).unwrap();
    assert_eq!(t.open(&above, 1, mode::OWRITE).unwrap_err(), "permission denied");
    t.open(&above, 1, mode::OREAD).unwrap();
    assert_eq!(t.read(&above, 1, 0, 100).unwrap(), b"Labelled");
    t.walk(&above, 0, 2, &[]).unwrap();
    assert_eq!(t.create(&above, 2, "mine", 0o644, mode::OWRITE).unwrap_err(), "permission denied");
    t.walk(&above, 0, 3, &["notes"]).unwrap();
    assert_eq!(t.remove(&above, 3).unwrap_err(), "permission denied");
    t.open(&above, 2, mode::OREAD).unwrap();
    assert_eq!(t.list(&above, 2).unwrap(), ["notes"]);

    assert_eq!(t.attach(&nobody, 0).unwrap_err(), "permission denied");
    assert_eq!(t.walk(&nobody, 0, 1, &[]).unwrap_err(), "unknown fid");
}

/// A blank range is formatted once; what it then holds is mounted, never formatted again. A
/// range smaller than walfs's smallest volume is none.
#[test]
fn a_blank_range_is_formatted_and_only_a_blank_one() {
    let disk = Memory::blank(SECTORS);
    assert!(matches!(mount(disk.clone()), Ok(Mounted::Files { .. })));
    let formatted = disk.bytes();
    assert!(formatted.iter().any(|b| *b != 0), "a blank range is formatted");
    assert!(matches!(mount(disk.clone()), Ok(Mounted::Files { .. })));
    assert!(disk.bytes() == formatted, "a formatted range is mounted, not formatted again");
    assert!(!Walfsd::new(mount(disk.clone()).unwrap(), vec![]).is_corrupt());
    let blocks = |n: u32| n as usize * SECTORS_PER_BLOCK as usize;
    assert_eq!(mount(Memory::blank(blocks(MIN_BLOCKS) - 1)).err(), Some(NoVolume::TooSmall));
    assert!(matches!(mount(Memory::blank(blocks(MIN_BLOCKS))), Ok(Mounted::Files { .. })));
}

/// Noise is not a volume: served as corrupt, every attach refused, the bytes untouched, and the
/// server still answering whatever comes.
#[test]
fn noise_is_never_formatted_and_never_mounted() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let noise: Vec<u8> = (0..512 * S)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    let disk = Memory::holding(noise.clone());
    assert!(noise.len() / 4096 >= MIN_BLOCKS as usize, "a range walfs could hold");
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    for _ in 0..3 {
        assert_eq!(t.attach(&who, 0).unwrap_err(), "corrupt");
        assert!(matches!(
            t.rpc(&who, Body::Tversion { msize: 8192, version: "9P2000" }),
            Body::Rversion { .. }
        ));
        assert_eq!(t.read(&who, 0, 0, 1).unwrap_err(), "unknown fid");
    }
    assert!(disk.bytes() == noise, "a range that holds anything is never formatted");
    assert!(
        Walfsd::new(mount(disk.clone()).unwrap(), vec![]).is_corrupt(),
        "the program says so on its console"
    );
    // A log header with one byte set is not blank either.
    let mut almost = vec![0u8; 512 * S];
    almost[4096 + 100] = 1;
    let disk = Memory::holding(almost.clone());
    assert!(matches!(mount(disk.clone()), Ok(Mounted::Corrupt(_))));
    assert!(disk.bytes() == almost);
}

/// R49 where it is read: one bit flipped in a file's data block makes a read of that file
/// `corrupt`, and nothing else: the other file reads, the volume still writes, and the server
/// stays up. The verdict is walfs's hash, not the test's.
#[test]
fn a_flipped_bit_in_a_file_is_corrupt_for_that_file_alone() {
    let disk = Memory::blank(SECTORS);
    let who = caller(1, &[]);
    {
        let mut t = T::on(&disk, &[]);
        t.attach(&who, 0).unwrap();
        t.put(&who, "damaged", &[0xd5; 4096]);
        t.put(&who, "whole", b"untouched");
    }
    let bytes = disk.bytes();
    let block = bytes.chunks(4096).position(|b| b == [0xd5; 4096]).expect("the data block");
    let sector = (block * 8) as u64;
    let mut flipped = disk.get(sector, S);
    flipped[17] ^= 0x04;
    disk.put(sector, &flipped);
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    assert_eq!(t.get(&who, &["damaged"]).unwrap_err(), "corrupt");
    assert_eq!(t.get(&who, &["whole"]).unwrap(), b"untouched");
    t.put(&who, "after", b"still writes");
    assert_eq!(t.get(&who, &["after"]).unwrap(), b"still writes");
    assert_eq!(t.get(&who, &["damaged"]).unwrap_err(), "corrupt", "and stays corrupt");
}

/// A mounted volume whose device starts failing answers `corrupt` from then on, to every
/// request, even once the device answers again: only a new mount trusts it.
#[test]
fn a_device_that_fails_makes_the_volume_corrupt_until_it_is_mounted_again() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    with_notes(&mut t, &who, b"before");
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.open(&who, 1, mode::ORDWR).unwrap();
    disk.fail();
    assert_eq!(t.write(&who, 1, 0, b"after").unwrap_err(), "corrupt");
    disk.0.borrow_mut().failing = false;
    assert_eq!(t.read(&who, 1, 0, 10).unwrap_err(), "corrupt");
    assert_eq!(t.stat(&who, 1).unwrap_err(), "corrupt");
    assert_eq!(t.walk(&who, 0, 2, &["notes"]).unwrap_err(), "corrupt");
    assert_eq!(t.attach(&who, 5).unwrap_err(), "corrupt");
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    assert_eq!(t.get(&who, &["notes"]).unwrap(), b"before");
}

/// A read-only range is never written: every change is refused as `read-only volume` before the
/// device is asked, reads go on, and the bytes do not change. A blank one is not formatted.
#[test]
fn a_read_only_range_is_never_written() {
    let blank = Memory::blank(SECTORS).read_only();
    assert!(matches!(mount(blank.clone()), Ok(Mounted::Corrupt(_))));
    assert!(blank.bytes().is_empty() || blank.0.borrow().sectors.is_empty(), "not formatted");

    let disk = Memory::blank(SECTORS);
    let who = caller(1, &[]);
    {
        let mut t = T::on(&disk, &[]);
        with_notes(&mut t, &who, b"kept");
    }
    let (before, writes) = (disk.0.borrow().sectors.clone(), disk.0.borrow().writes);
    let disk = disk.read_only();
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    assert_eq!(t.open(&who, 1, mode::OWRITE).unwrap_err(), "read-only volume");
    assert_eq!(t.open(&who, 1, mode::OREAD | mode::OTRUNC).unwrap_err(), "read-only volume");
    t.walk(&who, 0, 2, &[]).unwrap();
    assert_eq!(t.create(&who, 2, "new", 0o644, mode::OWRITE).unwrap_err(), "read-only volume");
    t.walk(&who, 0, 3, &["notes"]).unwrap();
    assert_eq!(t.remove(&who, 3).unwrap_err(), "read-only volume");
    assert_eq!(t.get(&who, &["notes"]).unwrap(), b"kept");
    assert_eq!(disk.0.borrow().writes, writes);
    assert!(disk.0.borrow().sectors == before);
}

/// The power-loss workload of `walfsd-power-loss`: `f` holds three blocks of `a`; a write of
/// three blocks of `b` over it, then `f` renamed to `g`.
const SPAN: usize = 3 * 4096;

/// R50 through the server, at every block write of the workload: the device loses every write
/// from the cut on, and the next mount finds `f` as before the write, `f` as after it, or `g` as
/// after both, never a mix, with walfs's check finding nothing. The workload's block writes are
/// counted too: `walfsd-power-loss` draws its cut from 1 to that count.
#[test]
fn a_cut_at_every_write_of_a_write_and_a_rename_leaves_before_or_after() {
    use redoubt_rt::server::typed::TypedServer;
    use redoubt_rt::wire::proto::littlefsd::{Message, Rename};
    let who = caller(1, &[]);
    let base = Memory::blank(SECTORS);
    let ino = {
        let mut t = T::on(&base, &[]);
        t.attach(&who, 0).unwrap();
        t.put(&who, "f", &[b'a'; SPAN]);
        t.walk(&who, 0, 1, &["f"]).unwrap()[0]
    };
    let workload = |disk: &Memory| {
        let mut t = T::on(disk, &[]);
        t.attach(&who, 0).unwrap();
        t.walk(&who, 0, 1, &["f"]).unwrap();
        t.open(&who, 1, mode::OWRITE).unwrap();
        let wrote = t.write(&who, 1, 0, &[b'b'; SPAN]);
        let rename = Message::Rename(Rename { old_dir: 0, old_name: "f", new_dir: 0, new_name: "g" });
        let renamed = crate::typed::Typed(&mut t.server).handle(&who, rename, &[]).map(|_| ());
        (wrote, renamed)
    };
    let whole = base.copy();
    let before = whole.0.borrow().writes;
    assert_eq!(workload(&whole), (Ok(SPAN as u32), Ok(())));
    let writes = whole.0.borrow().writes - before;
    // The cut's range in the case: tests/walfsd-power-loss.toml's client draws from it.
    assert_eq!(writes, CUT_WRITES, "the workload's block writes");
    for cut in 1..=writes {
        let disk = base.copy();
        let at = disk.0.borrow().writes + cut;
        disk.0.borrow_mut().fail_at = Some(at);
        let _ = workload(&disk);
        disk.0.borrow_mut().fail_at = None;
        let mut t = T::on(&disk, &[]);
        t.attach(&who, 0).unwrap();
        let (f, g) = (t.get(&who, &["f"]), t.get(&who, &["g"]));
        let state = match (f.as_deref(), g.as_deref()) {
            (Ok(f), Err(_)) if f == [b'a'; SPAN] => "before",
            (Ok(f), Err(_)) if f == [b'b'; SPAN] => "written",
            (Err(_), Ok(g)) if g == [b'b'; SPAN] => "renamed",
            _ => panic!("cut at write {cut}: neither before nor after"),
        };
        let name = if state == "renamed" { "g" } else { "f" };
        assert_eq!(t.walk(&who, 0, 2, &[name]).unwrap(), [ino], "cut at {cut}: the same file");
        assert_eq!(t.stat(&who, 2).unwrap().1, SPAN as u64);
        sound(&disk);
    }
}

/// The block writes `walfsd-power-loss`'s workload takes on a fresh 4 MiB volume.
pub(crate) const CUT_WRITES: usize = 20;

/// Every step of a transaction is behind a flush: the format's `sync` is `blkd`'s `flush`, four to
/// a transaction (servers/walfsd.md, "The log").
#[test]
fn every_transaction_is_four_flushes() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    with_notes(&mut t, &who, b"one");
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.open(&who, 1, mode::OWRITE).unwrap();
    let before = disk.0.borrow().flushes;
    assert_eq!(t.write(&who, 1, 0, b"two"), Ok(3));
    assert_eq!(disk.0.borrow().flushes - before, 4);
}

/// A sequential read of a file costs a fixed number of block reads per request, finding the
/// file again and opening it, and one or two per data block, each block through its hash
/// (servers/walfsd.md, "The hash region"): measured here for 4 KiB and 32 KiB reads, and bounded.
#[test]
fn a_sequential_read_costs_a_few_block_reads_per_request_and_per_block() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    let blocks = 64;
    with_notes(&mut t, &who, &vec![7u8; blocks * 4096][..]);
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    let per_block = |t: &mut T, size: usize| {
        let before = disk.0.borrow().reads;
        for at in (0..blocks * 4096).step_by(size) {
            assert_eq!(t.read(&who, 1, at as u64, size as u32).unwrap().len(), size);
        }
        (disk.0.borrow().reads - before) as f64 / blocks as f64
    };
    let (small, large) = (per_block(&mut t, 4096), per_block(&mut t, 32 * 1024));
    std::println!("block reads per data block: {small} in 4 KiB reads, {large} in 32 KiB reads");
    assert!(small <= 11.0 && large <= 3.0, "{small}, {large} block reads per data block");
}

/// The 9P2000 conformance vectors (libs/wire/vectors/9p.txt) against `walfsd`: whatever they
/// send, every answer decodes and carries its tag, and the volume still mounts and checks sound.
#[test]
fn the_conformance_vectors_run_against_walfsd() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    with_notes(&mut t, &who, b"hello");
    t.clunk(&who, 0).unwrap();
    let counts = redoubt_fake_kernel::vectors::run(&mut t.server, &who);
    assert!(counts.well_formed > 20 && counts.malformed > 5, "{counts:?}");
    t.attach(&who, 100).unwrap();
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    assert_eq!(t.walk(&who, 0, 1, &["notes"]).map(|q| q.len()), Ok(1));
    sound(&disk);
}

/// A listing reads the directory once per window of 64 entries, and lists each entry once.
#[test]
fn listing_a_directory_reads_it_once_per_window() {
    let disk = Memory::blank(SECTORS * 4);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    for i in 0..200 {
        t.walk(&who, 0, 1, &[]).unwrap();
        t.create(&who, 1, &std::format!("f{i:03}"), 0o644, mode::OWRITE).unwrap();
        t.clunk(&who, 1).unwrap();
    }
    t.walk(&who, 0, 1, &[]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    let passes = t.server.fs.passes;
    let names = t.list_by(&who, 1, 1024).unwrap();
    assert_eq!(names.len(), 200);
    assert!(names.iter().enumerate().all(|(i, n)| *n == std::format!("f{i:03}")));
    assert_eq!(t.server.fs.passes - passes, 200usize.div_ceil(64) as u32);
}
