//! `littlefsd`'s files through the 9P skeleton, as a client sends them, on a block device in memory
//! standing in for `blkd`'s range. The whole program against a fake kernel and a fake `blkd`,
//! and the client library against it, are in `tests/`.

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
use crate::volume::{Fault, Geometry, SECTOR, SECTORS_PER_BLOCK, mount};

extern crate std;
use std::string::String as StdString;

/// A range in memory, sparse: a sector never written reads as zero, so a test can have a
/// range of gigabytes. Clones share the sectors, so a test can look at or break the disk under
/// a running server.
#[derive(Clone)]
pub(crate) struct Memory(Rc<RefCell<Disk>>);

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
    /// Reads that reached the disk.
    pub reads: usize,
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

    fn put(&self, sector: u64, data: &[u8]) {
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

    fn flush(&mut self) -> Result<(), Fault> { self.check(0, 0) }
}

/// A volume of 64 blocks (256 KiB).
pub(crate) const SECTORS: usize = 64 * 8;

pub(crate) fn caller(badge: u64, labels: &[u64]) -> Caller {
    Caller { badge, account: 1001, labels: Labels::from_slice(labels).unwrap() }
}

/// A server on `disk` under `labels`, and the 9P a client would send it.
pub(crate) struct T {
    pub server: NineServer<Littlefsd<Memory>>,
    pub buf: Vec<u8>,
}

impl T {
    pub fn on(disk: &Memory, labels: &[u64]) -> T {
        let mounted = mount(disk.clone()).expect("a volume");
        let server = NineServer::new(Littlefsd::new(mounted, labels.to_vec()), limits(4), 0).unwrap();
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

    pub fn stat(&mut self, who: &Caller, fid: u32) -> Result<(StdString, u64, u64), StdString> {
        match self.rpc(who, Body::Tstat { fid }) {
            Body::Rstat { stat } => Ok((stat.name.into(), stat.length, stat.qid.path)),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    /// The qid version `Tstat` reports.
    pub fn version(&mut self, who: &Caller, fid: u32) -> u32 {
        match self.rpc(who, Body::Tstat { fid }) {
            Body::Rstat { stat } => stat.qid.version,
            other => panic!("{other:?}"),
        }
    }

    pub fn clunk(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tclunk { fid })
    }

    pub fn remove(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tremove { fid })
    }

    /// The names a directory read of `fid` (opened) lists.
    pub fn list(&mut self, who: &Caller, fid: u32) -> Result<Vec<StdString>, StdString> {
        let mut names = Vec::new();
        let mut offset = 0;
        loop {
            let chunk = self.read(who, fid, offset, 4096)?;
            if chunk.is_empty() {
                return Ok(names);
            }
            offset += chunk.len() as u64;
            names.extend(redoubt_rt::wire::ninep::stats(&chunk).map(|s| StdString::from(s.unwrap().name)));
        }
    }

    /// The entries one directory read of `fid` (opened) returns, and the bytes it took.
    pub fn entries(
        &mut self,
        who: &Caller,
        fid: u32,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<Entry>, u64), StdString> {
        let chunk = self.read(who, fid, offset, count)?;
        Ok((redoubt_rt::wire::ninep::stats(&chunk).map(|s| entry(&s.unwrap())).collect(), chunk.len() as u64))
    }

    /// Every entry a directory read of `fid` (opened) lists, from offset 0.
    pub fn listing(&mut self, who: &Caller, fid: u32) -> Result<Vec<Entry>, StdString> {
        let (mut all, mut offset) = (Vec::new(), 0);
        loop {
            let (entries, n) = self.entries(who, fid, offset, 4096)?;
            if n == 0 {
                return Ok(all);
            }
            offset += n;
            all.extend(entries);
        }
    }

    /// What `Tstat` says of `fid`, as a directory read would list it.
    pub fn entry(&mut self, who: &Caller, fid: u32) -> Entry {
        match self.rpc(who, Body::Tstat { fid }) {
            Body::Rstat { stat } => entry(&stat),
            other => panic!("{other:?}"),
        }
    }
}

/// A stat as owned values: name, qid type, version and path, mode, length.
pub(crate) type Entry = (StdString, u8, u32, u64, u32, u64);

fn entry(s: &redoubt_rt::wire::ninep::Stat<'_>) -> Entry {
    (s.name.into(), s.qid.kind, s.qid.version, s.qid.path, s.mode, s.length)
}

/// Makes the root's directory `dir` holding `n` files named by `name`, as `who` on fid 0: file
/// `i` holds `i % 7` bytes, written `i % 3` times, and every hundredth entry is a directory.
pub(crate) fn many(t: &mut T, who: &Caller, dir: &str, n: usize, name: impl Fn(usize) -> StdString) {
    t.walk(who, 0, 1, &[]).unwrap();
    t.create(who, 1, dir, DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(who, 1).unwrap();
    for i in 0..n {
        t.walk(who, 0, 1, &[dir]).unwrap();
        if i % 100 == 99 {
            t.create(who, 1, &name(i), DMDIR | 0o755, mode::OREAD).unwrap();
        } else {
            t.create(who, 1, &name(i), 0o644, mode::OWRITE).unwrap();
            for _ in 0..i % 3 {
                t.write(who, 1, 0, &[b'x'; 7][..i % 7]).unwrap();
            }
        }
        t.clunk(who, 1).unwrap();
    }
}

/// The names littlefs holds in the directory `dir`, in its order.
pub(crate) fn names_in(t: &mut T, dir: &str) -> Vec<StdString> {
    let mut names = Vec::new();
    t.server
        .fs
        .with(|fs| fs.read_dir(dir, |e| names.push(StdString::from_utf8(e.name.to_vec()).unwrap())))
        .unwrap();
    names
}

/// A fresh volume with `notes` holding `data`, written over 9P; fid 0 is the root, and the file
/// is clunked.
pub(crate) fn with_notes(t: &mut T, who: &Caller, data: &[u8]) {
    t.attach(who, 0).unwrap();
    t.walk(who, 0, 1, &[]).unwrap();
    t.create(who, 1, "notes", 0o644, mode::OWRITE).unwrap();
    assert_eq!(t.write(who, 1, 0, data), Ok(data.len() as u32));
    t.clunk(who, 1).unwrap();
}

#[test]
fn attach_walk_open_read_write() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let alice = caller(1, &[]);
    with_notes(&mut t, &alice, b"hello, world");
    let qids = t.walk(&alice, 0, 1, &["notes"]).unwrap();
    assert_ne!(qids[0], 0, "a file's qid path is its id, never the root's");
    t.open(&alice, 1, mode::ORDWR).unwrap();
    assert_eq!(t.read(&alice, 1, 0, 100).unwrap(), b"hello, world");
    assert_eq!(t.read(&alice, 1, 7, 3).unwrap(), b"wor");
    assert_eq!(t.write(&alice, 1, 0, b"HELLO"), Ok(5));
    assert_eq!(t.read(&alice, 1, 0, 100).unwrap(), b"HELLO, world");
    // Past the end reads nothing, whatever the offset.
    assert_eq!(t.read(&alice, 1, 12, 10).unwrap(), b"");
    assert_eq!(t.read(&alice, 1, u64::MAX - 5, 5).unwrap(), b"");
    assert_eq!(t.stat(&alice, 1).unwrap(), ("notes".into(), 12, qids[0]));
    assert_eq!(t.walk(&alice, 0, 2, &["missing"]).unwrap_err(), "file does not exist");
}

#[test]
fn files_and_directories_survive_a_remount() {
    let disk = Memory::blank(SECTORS);
    let alice = caller(1, &[]);
    let qid = {
        let mut t = T::on(&disk, &[]);
        with_notes(&mut t, &alice, b"kept");
        t.walk(&alice, 0, 1, &[]).unwrap();
        t.create(&alice, 1, "dir", DMDIR | 0o755, mode::OREAD).unwrap();
        t.walk(&alice, 0, 2, &["notes"]).unwrap()[0]
    };
    let mut t = T::on(&disk, &[]);
    t.attach(&alice, 0).unwrap();
    assert_eq!(t.walk(&alice, 0, 1, &["notes"]).unwrap(), [qid], "the id is the file's, on the medium");
    t.open(&alice, 1, mode::OREAD).unwrap();
    assert_eq!(t.read(&alice, 1, 0, 100).unwrap(), b"kept");
    t.walk(&alice, 0, 2, &[]).unwrap();
    t.open(&alice, 2, mode::OREAD).unwrap();
    assert_eq!(t.list(&alice, 2).unwrap(), ["dir", "notes"]);
}

/// littlefsd.md's attack test: after a remove, the file's other fids get `removed` on read, write and
/// stat, and only a clunk succeeds; a file made in its place under the same name is not theirs.
#[test]
fn a_removed_files_other_fids_get_removed() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let (alice, bob) = (caller(1, &[]), caller(2, &[]));
    with_notes(&mut t, &alice, b"secret");
    t.attach(&bob, 0).unwrap();
    t.walk(&bob, 0, 1, &["notes"]).unwrap();
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
    // A new file under the old name is not the old fid's.
    t.walk(&alice, 0, 2, &[]).unwrap();
    t.create(&alice, 2, "notes", 0o644, mode::OWRITE).unwrap();
    t.write(&alice, 2, 0, b"new").unwrap();
    gone(&mut t);
    t.clunk(&bob, 1).unwrap();
    t.walk(&bob, 0, 1, &["notes"]).unwrap();
    t.open(&bob, 1, mode::OREAD).unwrap();
    assert_eq!(t.read(&bob, 1, 0, 10).unwrap(), b"new");
}

/// Labels are the volume's (littlefsd.md): with `labels=7`, {7} reads and writes, {7, 9} reads but
/// cannot write, and {} reaches nothing, not even the root or a listing.
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

/// A blank range is formatted once; what it then holds is mounted, never formatted again.
#[test]
fn a_blank_range_is_formatted_and_only_a_blank_one() {
    let disk = Memory::blank(SECTORS);
    assert!(matches!(mount(disk.clone()), Ok(Mounted::Files { .. })));
    let formatted = disk.bytes();
    assert!(formatted.iter().any(|b| *b != 0), "a blank range is formatted");
    assert!(matches!(mount(disk.clone()), Ok(Mounted::Files { .. })));
    assert!(disk.bytes() == formatted, "a formatted range is mounted, not formatted again");
    assert!(!Littlefsd::new(mount(disk.clone()).unwrap(), vec![]).is_corrupt());
    assert_eq!(mount(Memory::blank(3 * 8 + 7)).err(), Some(crate::volume::NoVolume::TooSmall));
    assert!(matches!(mount(Memory::blank(4 * 8)), Ok(Mounted::Files { .. })));
}

/// Noise is not a volume: served as corrupt, every attach refused, the bytes untouched, and the
/// server still answering whatever comes.
#[test]
fn noise_is_never_formatted_and_never_mounted() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let noise: Vec<u8> = (0..SECTORS * SECTOR as usize)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    let disk = Memory::holding(noise.clone());
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
        Littlefsd::new(mount(disk.clone()).unwrap(), vec![]).is_corrupt(),
        "the program says so on its console"
    );
    // A superblock pair with one byte set is not blank either.
    let mut almost = vec![0u8; SECTORS * SECTOR as usize];
    almost[4096 + 100] = 1;
    let disk = Memory::holding(almost.clone());
    assert!(matches!(mount(disk.clone()), Ok(Mounted::Corrupt(_))));
    assert!(disk.bytes() == almost);
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
}

/// Every write and truncation moves the file's qid version, so a client caching it sees the
/// change; reads and stats do not.
#[test]
fn writes_and_truncations_move_the_qid_version() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    with_notes(&mut t, &who, b"one");
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    let v = t.version(&who, 1);
    t.open(&who, 1, mode::ORDWR).unwrap();
    t.read(&who, 1, 0, 10).unwrap();
    assert_eq!(t.version(&who, 1), v);
    t.write(&who, 1, 0, b"two").unwrap();
    assert_eq!(t.version(&who, 1), v + 1);
    t.walk(&who, 0, 2, &["notes"]).unwrap();
    t.open(&who, 2, mode::OWRITE | mode::OTRUNC).unwrap();
    assert_eq!(t.version(&who, 1), v + 2);
    assert_eq!(t.stat(&who, 1).unwrap().1, 0);
}

/// The 9P2000 conformance vectors (libs/wire/vectors/9p.txt) against `littlefsd`: whatever they
/// send, every answer decodes and carries its tag, and the volume still mounts afterwards.
#[test]
fn the_conformance_vectors_run_against_littlefsd() {
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
}

/// A read-only range (`blkd`'s `info`) is never written: every change is refused as
/// `read-only volume` before the device is asked, so a refused write never poisons the volume,
/// reads go on, and the bytes do not change. A blank read-only range is not formatted.
#[test]
fn a_read_only_range_is_never_written() {
    let blank = Memory::blank(SECTORS).read_only();
    assert!(matches!(mount(blank.clone()), Ok(Mounted::Corrupt(_))));
    assert!(blank.bytes().iter().all(|b| *b == 0), "a blank read-only range is not formatted");

    let disk = Memory::blank(SECTORS);
    let who = caller(1, &[]);
    {
        let mut t = T::on(&disk, &[]);
        with_notes(&mut t, &who, b"kept");
    }
    let before = disk.bytes();
    let disk = disk.read_only();
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    for m in [mode::OWRITE, mode::ORDWR, mode::OREAD | mode::OTRUNC] {
        assert_eq!(t.open(&who, 1, m).unwrap_err(), "read-only volume", "{m:#x}");
    }
    t.walk(&who, 0, 2, &[]).unwrap();
    assert_eq!(t.create(&who, 2, "new", 0o644, mode::OREAD).unwrap_err(), "read-only volume");
    t.walk(&who, 0, 3, &["notes"]).unwrap();
    assert_eq!(t.remove(&who, 3).unwrap_err(), "read-only volume");
    t.open(&who, 1, mode::OREAD).unwrap();
    assert_eq!(t.read(&who, 1, 0, 10).unwrap(), b"kept", "nothing refused poisoned the volume");
    t.walk(&who, 0, 4, &[]).unwrap();
    t.open(&who, 4, mode::OREAD).unwrap();
    assert_eq!(t.list(&who, 4).unwrap(), ["notes"]);
    assert!(disk.bytes() == before, "not one byte of a read-only range changed");
}

/// The mount serves only volumes `littlefsd` wrote: an entry without an id, two entries with one id,
/// or a counter at or below a live id (any of which could let a fid on one file reach another)
/// is a corrupt volume; what `littlefsd` wrote mounts.
#[test]
fn a_volume_whose_ids_do_not_hold_together_is_corrupt() {
    let who = caller(1, &[]);
    type Forge<'a> = &'a dyn Fn(&mut Filesystem<Blocks<Memory>>) -> Result<(), littlefs::Error>;
    let forged = |forge: Forge<'_>| {
        let disk = Memory::blank(SECTORS);
        let mut t = T::on(&disk, &[]);
        with_notes(&mut t, &who, b"a");
        t.walk(&who, 0, 1, &[]).unwrap();
        t.create(&who, 1, "other", 0o644, mode::OWRITE).unwrap();
        t.server.fs.with(forge).unwrap();
        let mut t = T::on(&disk, &[]);
        t.attach(&who, 0)
    };
    assert_eq!(forged(&|_| Ok(())), Ok(()));
    let next =
        |n: u64| move |fs: &mut Filesystem<Blocks<Memory>>| fs.set_attr("", ATTR_NEXT_ID, &n.to_le_bytes());
    // `notes` and `other` hold ids 1 and 2.
    assert_eq!(forged(&next(2)).unwrap_err(), "corrupt", "a counter at a live id");
    assert_eq!(forged(&next(1)).unwrap_err(), "corrupt", "a counter below a live id");
    assert_eq!(
        forged(&|fs| fs.remove_attr("", ATTR_NEXT_ID)).unwrap_err(),
        "corrupt",
        "no counter, live ids"
    );
    assert_eq!(
        forged(&|fs| fs.set_attr("other", ATTR_ID, &1u64.to_le_bytes())).unwrap_err(),
        "corrupt",
        "two entries with one id"
    );
    assert_eq!(
        forged(&|fs| fs.remove_attr("other", ATTR_ID)).unwrap_err(),
        "corrupt",
        "an entry without its id"
    );
    assert_eq!(
        forged(&|fs| {
            let h =
                fs.open("bare", littlefs::OpenOptions { write: true, create: true, ..Default::default() })?;
            fs.close(h)
        })
        .unwrap_err(),
        "corrupt",
        "an entry another writer made"
    );
    assert_eq!(
        forged(&|fs| fs.set_attr("other", ATTR_ID, &0u64.to_le_bytes())).unwrap_err(),
        "corrupt",
        "an entry claiming the root's id"
    );
}

/// A reader whose labels are a superset of the volume's walks, stats, reads and lists: not one
/// write reaches the device (servers/serving.md R25: a write needs the labels equal).
#[test]
fn a_higher_reader_writes_nothing() {
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[7]);
    let owner = caller(1, &[7]);
    with_notes(&mut t, &owner, b"labelled");
    t.walk(&owner, 0, 1, &[]).unwrap();
    t.create(&owner, 1, "d", DMDIR | 0o755, mode::OREAD).unwrap();
    let before = (disk.0.borrow().writes, disk.bytes());
    let reader = caller(2, &[7, 9]);
    t.attach(&reader, 0).unwrap();
    t.walk(&reader, 0, 1, &["notes"]).unwrap();
    t.stat(&reader, 1).unwrap();
    t.open(&reader, 1, mode::OREAD).unwrap();
    t.read(&reader, 1, 0, 100).unwrap();
    t.walk(&reader, 0, 2, &["d"]).unwrap();
    t.walk(&reader, 0, 3, &[]).unwrap();
    t.open(&reader, 3, mode::OREAD).unwrap();
    assert_eq!(t.list(&reader, 3).unwrap(), ["d", "notes"]);
    assert!((disk.0.borrow().writes, disk.bytes()) == before, "a read wrote the volume");
}

/// A rename between two metadata pairs is two commits, with a pending move between them. Power
/// failing at any of its writes leaves a volume that passes the mount's id check (the pending
/// move's source is not a second entry with the id) and holds the file once, under its id.
#[test]
fn a_rename_cut_short_still_mounts() {
    let who = caller(1, &[]);
    // A file, and a directory with a child: the pending move hides the source entry, one
    // dirstruct and one head, so the moved directory and its child are each seen once.
    for (from, to, child) in [("notes", "moved", None), ("d2", "d2", Some("inner"))] {
        let disk = Memory::blank(SECTORS);
        let ids = {
            let mut t = T::on(&disk, &[]);
            with_notes(&mut t, &who, b"moving");
            for dir in ["d", "d2"] {
                t.walk(&who, 0, 1, &[]).unwrap();
                t.create(&who, 1, dir, DMDIR | 0o755, mode::OREAD).unwrap();
                t.clunk(&who, 1).unwrap();
            }
            t.walk(&who, 0, 1, &["d2"]).unwrap();
            t.create(&who, 1, "inner", 0o644, mode::OWRITE).unwrap();
            let mut path = vec![from];
            path.extend(child);
            t.walk(&who, 0, 2, &path).unwrap()
        };
        let image = disk.bytes();
        let target = alloc::format!("d/{to}");
        let writes = {
            let disk = Memory::holding(image.clone());
            let mut t = T::on(&disk, &[]);
            t.server.fs.with(|fs| fs.rename(from, &target)).unwrap();
            let writes = disk.0.borrow().writes;
            writes
        };
        assert!(writes > 1, "a rename across pairs is more than one write");
        for n in 1..=writes {
            let disk = Memory::holding(image.clone());
            disk.0.borrow_mut().fail_at = Some(n);
            let mut t = T::on(&disk, &[]);
            assert!(t.server.fs.with(|fs| fs.rename(from, &target)).is_err(), "{from}: write {n}");
            let after = Memory::holding(disk.bytes());
            let mut t = T::on(&after, &[]);
            t.attach(&who, 0).unwrap_or_else(|e| panic!("{from} cut at write {n}: {e}"));
            let mut at = |fid: u32, path: &[&str]| {
                let mut path = path.to_vec();
                path.extend(child);
                t.walk(&who, 0, fid, &path).ok()
            };
            let found = (at(1, &[from]), at(2, &["d", to]));
            let tail = |q: &Vec<u64>| q[q.len() - ids.len()..].to_vec();
            let found = (found.0.as_ref().map(tail), found.1.as_ref().map(tail));
            assert!(
                found == (Some(ids.clone()), None) || found == (None, Some(ids.clone())),
                "{from} cut at write {n}: {found:?}"
            );
        }
    }
}

/// littlefs's CRC-32 (no final inversion), for forging commits.
fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    crc
}

/// A directory entry's struct, naming its first pair (littlefs SPEC.md, `LFS_TYPE_DIRSTRUCT`).
const DIR_STRUCT: u32 = 0x200;
/// A hard tail, naming the next pair of the same directory (`LFS_TYPE_HARDTAIL`).
const HARD_TAIL: u32 = 0x601;

/// Rewrites every tag of type `typ` naming a pair in a metadata block to name `pair`, and
/// re-signs each commit, so the forgery is a well-formed image (littlefs SPEC.md: tags XORed
/// with the one before, big-endian; each commit closed by a CRC tag over it). Returns how many
/// tags it rewrote.
fn repoint(block: &mut [u8], typ_to_rewrite: u32, pair: [u32; 2]) -> usize {
    let mut crc = crc32(0xffff_ffff, &block[..4]);
    let (mut off, mut ptag, mut rewrote) = (4usize, 0xffff_ffffu32, 0);
    while off + 4 <= block.len() {
        let raw: [u8; 4] = block[off..off + 4].try_into().unwrap();
        let tag = u32::from_be_bytes(raw) ^ ptag;
        if tag & 0x8000_0000 != 0 {
            break;
        }
        let typ = (tag >> 20) & 0x7ff;
        let len = match tag & 0x3ff {
            0x3ff => 0,
            n => n as usize,
        };
        crc = crc32(crc, &raw);
        if typ & 0x7fe == 0x500 {
            // A commit's CRC tag: sign what was rewritten, and start the next commit.
            block[off + 4..off + 8].copy_from_slice(&crc.to_le_bytes());
            ptag = tag ^ ((typ & 1) << 31);
            crc = 0xffff_ffff;
        } else {
            if typ == typ_to_rewrite {
                block[off + 4..off + 8].copy_from_slice(&pair[0].to_le_bytes());
                block[off + 8..off + 12].copy_from_slice(&pair[1].to_le_bytes());
                rewrote += 1;
            }
            crc = crc32(crc, &block[off + 4..off + 4 + len]);
            ptag = tag;
        }
        off += 4 + len;
    }
    rewrote
}

/// Forges `disk`'s root so every directory entry in it names `pair`; returns how many.
fn forge_root(disk: &Memory, pair: [u32; 2]) -> usize {
    let mut root = disk.get(0, 2 * 4096);
    let rewrote = root.chunks_mut(4096).map(|b| repoint(b, DIR_STRUCT, pair)).sum();
    disk.put(0, &root);
    rewrote
}

/// A forged image with a directory that is the root's own pair, on a range of 64k blocks: a walk
/// by path re-resolving each directory would make (blocks / 2)^2 / 2 pair reads. The mount reads
/// each directory by its pair and no more pairs than the volume holds, so it refuses the volume
/// as corrupt within a number of reads linear in the volume.
#[test]
fn a_directory_aliasing_the_root_is_refused_in_linear_time() {
    let blocks = 64 * 1024;
    let disk = Memory::blank(blocks * 8);
    let who = caller(1, &[]);
    {
        let mut t = T::on(&disk, &[]);
        t.attach(&who, 0).unwrap();
        t.walk(&who, 0, 1, &[]).unwrap();
        t.create(&who, 1, "a", DMDIR | 0o755, mode::OREAD).unwrap();
    }
    assert!(forge_root(&disk, [0, 1]) >= 1);
    {
        let Ok(Mounted::Files { mut fs, .. }) = mount(disk.clone()) else { panic!("the forgery mounts") };
        assert!(fs.stat("a/a/a/a").is_ok(), "the directory aliases the root");
    }
    disk.0.borrow_mut().reads = 0;
    let mut t = T::on(&disk, &[]);
    assert_eq!(t.attach(&who, 0).unwrap_err(), "corrupt");
    let reads = disk.0.borrow().reads;
    assert!(reads < 8 * blocks, "the mount's check took {reads} reads for {blocks} blocks");
}

/// Two directories forged onto one pair: a create in one would show in both, and removing one
/// would free a pair the other names. No two directories may share a block, so it is corrupt.
#[test]
fn two_directories_sharing_a_pair_are_corrupt() {
    let disk = Memory::blank(SECTORS);
    let who = caller(1, &[]);
    {
        let mut t = T::on(&disk, &[]);
        t.attach(&who, 0).unwrap();
        for name in ["a", "b"] {
            t.walk(&who, 0, 1, &[]).unwrap();
            t.create(&who, 1, name, DMDIR | 0o755, mode::OREAD).unwrap();
            t.clunk(&who, 1).unwrap();
        }
    }
    let a = {
        let Ok(Mounted::Files { mut fs, .. }) = mount(disk.clone()) else { panic!("mounts") };
        let mut a = None;
        let root = fs.root_dir();
        let named_a = |e: &littlefs::DirEntry| {
            if e.name == b"a" {
                a = e.dir();
            }
        };
        fs.read_dir_at(root, named_a, |_| Ok(())).unwrap();
        a.unwrap().blocks()
    };
    assert_eq!(forge_root(&disk, a), 2);
    let mut t = T::on(&disk, &[]);
    assert_eq!(t.attach(&who, 0).unwrap_err(), "corrupt");
}

/// Makes the directory `name` under the root with `files` empty files, enough of them, with
/// long names, to split it over several pairs.
fn split_dir(disk: &Memory, name: &str, files: usize) {
    let who = caller(1, &[]);
    let mut t = T::on(disk, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &[]).unwrap();
    t.create(&who, 1, name, DMDIR | 0o755, mode::OREAD).unwrap();
    for i in 0..files {
        t.walk(&who, 0, 2, &[name]).unwrap();
        t.create(&who, 2, &alloc::format!("a-name-long-enough-to-fill-a-pair-{i}"), 0o644, mode::OREAD)
            .unwrap();
        t.clunk(&who, 2).unwrap();
    }
}

/// The pairs of the root's directory `name`, as littlefs reads its chain.
fn chain(disk: &Memory, name: &str) -> Vec<[u32; 2]> {
    let mut fs = match mount(disk.clone()) {
        Ok(Mounted::Files { fs, .. }) => fs,
        Ok(Mounted::Corrupt(e)) => panic!("{e:?}"),
        Err(e) => panic!("{e:?}"),
    };
    let mut dir = None;
    let root = fs.root_dir();
    fs.read_dir_at(root, |e| dir = dir.or(e.dir().filter(|_| e.name == name.as_bytes())), |_| Ok(()))
        .unwrap();
    let mut pairs = Vec::new();
    fs.read_dir_at(dir.unwrap(), |_| {}, |p| Ok(pairs.push(p.blocks()))).unwrap();
    pairs
}

/// Forges the pair `pair` so its hard tail names `to`; returns how many tags it rewrote.
fn forge_tail(disk: &Memory, pair: [u32; 2], to: [u32; 2]) -> usize {
    let sectors = SECTORS_PER_BLOCK;
    let mut rewrote = 0;
    for block in pair {
        let mut data = disk.get(u64::from(block) * sectors, 4096);
        rewrote += repoint(&mut data, HARD_TAIL, to);
        disk.put(u64::from(block) * sectors, &data);
    }
    rewrote
}

/// A split directory whose tail is forged to be an empty directory's pair: no entry shows
/// twice and no two directories start at one pair, yet a create in the empty one would show in
/// both, and on a shared volume under another root. A pair named twice is corrupt.
#[test]
fn a_tail_that_is_another_directorys_pair_is_corrupt() {
    let disk = Memory::blank(SECTORS);
    // littlefs lists each new directory's pairs right after the root's, so `b`, made first,
    // follows `a` on the volume's list of pairs, and the forgery makes that list no loop.
    split_dir(&disk, "b", 0);
    split_dir(&disk, "a", 80);
    let (a, b) = (chain(&disk, "a"), chain(&disk, "b"));
    assert!(a.len() >= 2, "a spans {a:?}");
    assert!(forge_tail(&disk, a[0], b[0]) >= 1);
    assert_eq!(chain(&disk, "a"), [a[0], b[0]], "the forgery reads b's pair as a's tail");
    let mut t = T::on(&disk, &[]);
    assert_eq!(t.attach(&caller(1, &[]), 0).unwrap_err(), "corrupt");
}

/// Two split directories whose chains are forged to join at one pair are corrupt.
#[test]
fn two_chains_joining_at_one_pair_are_corrupt() {
    let disk = Memory::blank(SECTORS);
    split_dir(&disk, "b", 80);
    split_dir(&disk, "a", 80);
    let (a, b) = (chain(&disk, "a"), chain(&disk, "b"));
    assert!(a.len() >= 2 && b.len() >= 2, "a spans {a:?}, b {b:?}");
    assert!(forge_tail(&disk, a[0], b[1]) >= 1);
    assert_eq!(chain(&disk, "a")[..2], [a[0], b[1]], "the forgery mounts, and joins a to b");
    let mut t = T::on(&disk, &[]);
    assert_eq!(t.attach(&caller(1, &[]), 0).unwrap_err(), "corrupt");
}

/// A split directory whose tail is forged back to its own first pair is corrupt. A directory's
/// tails are links in the volume's one list of pairs, so littlefs's own mount finds the loop,
/// within its bound of the volume's pairs.
#[test]
fn a_chain_looping_back_to_its_head_is_corrupt() {
    let disk = Memory::blank(SECTORS);
    split_dir(&disk, "a", 80);
    let a = chain(&disk, "a");
    assert!(forge_tail(&disk, a[0], a[0]) >= 1);
    disk.0.borrow_mut().reads = 0;
    let mut t = T::on(&disk, &[]);
    assert_eq!(t.attach(&caller(1, &[]), 0).unwrap_err(), "corrupt");
    let reads = disk.0.borrow().reads;
    assert!(reads < 8 * SECTORS, "the mount took {reads} reads");
}

/// A volume whose directories really are split over several pairs mounts, and its files are
/// all there.
#[test]
fn split_directories_still_mount() {
    let disk = Memory::blank(SECTORS);
    split_dir(&disk, "a", 80);
    split_dir(&disk, "b", 80);
    assert!(chain(&disk, "a").len() >= 2 && chain(&disk, "b").len() >= 2);
    let who = caller(1, &[]);
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["b"]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    assert_eq!(t.list(&who, 1).unwrap().len(), 80);
}

/// A listing of 600 entries makes about one pass over the directory per window and no lookup
/// per entry, and lists every entry once, in littlefs's order, as `Tstat` reports it.
#[test]
fn listing_a_directory_reads_it_once_per_window() {
    let disk = Memory::blank(1024 * 8);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    many(&mut t, &who, "d", 600, |i| alloc::format!("file-{i}"));
    t.walk(&who, 0, 1, &["d"]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    let (passes, reads) = (t.server.fs.passes, disk.0.borrow().reads);
    let listed = t.listing(&who, 1).unwrap();
    let (passes, reads) = (t.server.fs.passes - passes, disk.0.borrow().reads - reads);
    assert!(passes as usize <= 600usize.div_ceil(WINDOW) + 1, "{passes} passes");
    let names: Vec<_> = listed.iter().map(|e| e.0.clone()).collect();
    assert_eq!(names, names_in(&mut t, "d"));
    // The block reads are those of the passes and of finding the directory again for each: no
    // entry is looked up.
    let before = disk.0.borrow().reads;
    t.server.fs.with(|fs| fs.read_dir("d", |_| {})).unwrap();
    let one = disk.0.borrow().reads - before;
    assert!(reads <= (passes as usize + 1) * one, "{reads} block reads for {passes} passes of {one}");
    for e in &listed {
        t.walk(&who, 0, 2, &["d", &e.0]).unwrap();
        assert_eq!(&t.entry(&who, 2), e);
        t.clunk(&who, 2).unwrap();
    }
}

/// One `read` at the largest msize, of a directory longer than a reply holds, makes no more
/// passes than the bound on [`WINDOW`] states.
#[test]
fn one_reply_costs_a_bounded_number_of_passes() {
    let disk = Memory::blank(2048 * 8);
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    many(&mut t, &who, "d", 1400, |i| alloc::format!("f{i:04}"));
    t.walk(&who, 0, 1, &["d"]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    let passes = t.server.fs.passes;
    let (entries, _) = t.entries(&who, 1, 0, MSIZE as u32).unwrap();
    let passes = t.server.fs.passes - passes;
    // A five-byte name's stat is 54 bytes; the shortest, a one-byte name's, is 50.
    let data = MSIZE - redoubt_rt::wire::ninep::IOHDRSZ;
    assert_eq!(entries.len(), data / 54);
    let most = data / 50;
    assert_eq!(most, 1310);
    assert!(passes as usize <= most.div_ceil(WINDOW) + 1, "{passes} passes");
}
