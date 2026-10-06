//! `erofsd`'s files through the 9P skeleton, as a client sends them, on a range in memory standing
//! in for `blkd`'s or a `verityd`'s. The whole program against a fake kernel and a fake `blkd` is
//! in `tests/`.

use alloc::rc::Rc;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::num::NonZeroU64;

use erofs::{Entry, pack};
use redoubt_rt::abi::{Error, Handle, Handles, Labels};
use redoubt_rt::server::ninep::{Answer, FIRST_MINTED_BADGE, Minter, NineServer, mode, ninep_common};
use redoubt_rt::wire::MSIZE;
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names};

use super::*;

extern crate std;
use std::string::String as StdString;

/// A range in memory. Clones share it, so a test can count its reads or break it under a running
/// server.
#[derive(Clone)]
struct Memory(Rc<RefCell<Disk>>);

struct Disk {
    bytes: Vec<u8>,
    /// Every request fails from now on.
    failing: bool,
    /// Reads that reached the range.
    reads: usize,
}

impl Memory {
    fn holding(mut bytes: Vec<u8>) -> Memory {
        bytes.resize(bytes.len().next_multiple_of(512), 0);
        Memory(Rc::new(RefCell::new(Disk { bytes, failing: false, reads: 0 })))
    }

    fn fail(&self) { self.0.borrow_mut().failing = true }

    fn reads(&self) -> usize { self.0.borrow().reads }
}

impl Range for Memory {
    fn sectors(&mut self) -> Result<u64, Fault> {
        let disk = self.0.borrow();
        if disk.failing { Err(Fault) } else { Ok(disk.bytes.len() as u64 / 512) }
    }

    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), Fault> {
        let mut disk = self.0.borrow_mut();
        disk.reads += 1;
        let end = at as usize + out.len();
        if disk.failing || end > disk.bytes.len() {
            return Err(Fault);
        }
        out.copy_from_slice(&disk.bytes[at as usize..end]);
        Ok(())
    }
}

fn caller(badge: u64, labels: &[u64]) -> Caller {
    Caller { badge, account: 1001, labels: Labels::from_slice(labels).unwrap() }
}

/// Bytes 0, 1, 2, ... wrapping, `len` of them.
fn counting(len: usize) -> Vec<u8> { (0..len).map(|i| (i % 251) as u8).collect() }

/// The tree the tests serve: files at and around the block's edges, one whose tail is inline,
/// and a directory of 300 entries that spans several blocks.
fn tree() -> Vec<(StdString, Option<Vec<u8>>)> {
    let mut tree = vec![
        ("big".to_string(), Some(counting(10_000))),
        ("block".to_string(), Some(counting(4096))),
        ("empty".to_string(), Some(Vec::new())),
        ("motd".to_string(), Some(b"hello\n".to_vec())),
        ("lib".to_string(), None),
    ];
    for i in 0..300 {
        tree.push((alloc::format!("lib/Elixir.Module{i:03}.beam"), Some(counting(i))));
    }
    tree.push(("lib/deep".to_string(), None));
    tree.push(("lib/deep/notes".to_string(), Some(b"deep".to_vec())));
    tree
}

fn image(tree: &[(StdString, Option<Vec<u8>>)]) -> Vec<u8> {
    let entries: Vec<Entry<'_>> = tree
        .iter()
        .map(|(path, data)| match data {
            Some(data) => Entry::File(path, data),
            None => Entry::Dir(path),
        })
        .collect();
    pack(&entries, |_| [0; 32]).unwrap()
}

/// A server on `disk` under `labels`, and the 9P a client would send it.
struct T {
    server: NineServer<Erofsd<Memory>>,
    buf: Vec<u8>,
}

impl T {
    fn on(disk: &Memory, labels: &[u64]) -> T {
        let erofsd = Erofsd::new(disk.clone(), labels.to_vec()).expect("a range");
        T { server: NineServer::new(erofsd, limits(4), 0).unwrap(), buf: vec![0; MSIZE] }
    }

    fn rpc(&mut self, who: &Caller, body: Body<'_>) -> Body<'_> {
        self.buf = vec![0; MSIZE];
        Message { tag: 9, body }.encode(&mut self.buf).unwrap();
        assert_eq!(self.server.answer_in_place(who, &mut self.buf), Answer::Replied);
        let reply = Message::decode(&self.buf).unwrap();
        assert_eq!(reply.tag, 9);
        reply.body
    }

    fn try_rpc(&mut self, who: &Caller, body: Body<'_>) -> Result<(), StdString> {
        match self.rpc(who, body) {
            Body::Rerror { ename } => Err(ename.into()),
            _ => Ok(()),
        }
    }

    fn attach(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tattach { fid, afid: NOFID, uname: "", aname: "" })
    }

    fn walk(&mut self, who: &Caller, fid: u32, newfid: u32, names: &[&str]) -> Result<Vec<u64>, StdString> {
        match self.rpc(who, Body::Twalk { fid, newfid, wnames: Names::new(names).unwrap() }) {
            Body::Rwalk { qids } if qids.as_slice().len() == names.len() => {
                Ok(qids.as_slice().iter().map(|q| q.path).collect())
            }
            Body::Rerror { ename } => Err(ename.into()),
            Body::Rwalk { .. } => Err("partial walk".into()),
            other => panic!("walk {names:?}: {other:?}"),
        }
    }

    fn open(&mut self, who: &Caller, fid: u32, m: u8) -> Result<(), StdString> {
        self.try_rpc(who, Body::Topen { fid, mode: m })
    }

    fn read(&mut self, who: &Caller, fid: u32, offset: u64, count: u32) -> Result<Vec<u8>, StdString> {
        match self.rpc(who, Body::Tread { fid, offset, count }) {
            Body::Rread { data } => Ok(data.to_vec()),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    fn stat(&mut self, who: &Caller, fid: u32) -> Result<(StdString, u64, u64, u32), StdString> {
        match self.rpc(who, Body::Tstat { fid }) {
            Body::Rstat { stat } => Ok((stat.name.into(), stat.length, stat.qid.path, stat.mode)),
            Body::Rerror { ename } => Err(ename.into()),
            other => panic!("{other:?}"),
        }
    }

    fn clunk(&mut self, who: &Caller, fid: u32) -> Result<(), StdString> {
        self.try_rpc(who, Body::Tclunk { fid })
    }

    /// The names a directory read of `fid` (opened) lists, `count` bytes at a time.
    fn list(&mut self, who: &Caller, fid: u32, count: u32) -> Result<Vec<StdString>, StdString> {
        let (mut names, mut offset) = (Vec::new(), 0);
        loop {
            let chunk = self.read(who, fid, offset, count)?;
            if chunk.is_empty() {
                return Ok(names);
            }
            offset += chunk.len() as u64;
            names.extend(redoubt_rt::wire::ninep::stats(&chunk).map(|s| StdString::from(s.unwrap().name)));
        }
    }

    /// `path` walked from a fresh attach on fid 0 to `fid`, and opened for reading.
    fn opened(&mut self, who: &Caller, fid: u32, path: &[&str]) -> Result<(), StdString> {
        let _ = self.clunk(who, 0);
        self.attach(who, 0)?;
        self.walk(who, 0, fid, path)?;
        self.open(who, fid, mode::OREAD)
    }
}

#[test]
fn files_read_back_byte_for_byte_at_block_edges_past_the_end_and_across_an_inline_tail() {
    let tree = tree();
    let disk = Memory::holding(image(&tree));
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    for (path, data) in tree.iter().filter_map(|(p, d)| Some((p, d.as_ref()?))) {
        let parts: Vec<&str> = path.split('/').collect();
        t.opened(&who, 1, &parts).unwrap();
        for (offset, count) in
            [(0, 8192), (4095, 2), (4096, 4096), (8191, 3000), (9999, 10), (10_000, 5), (1 << 40, 1)]
        {
            let want = data.get(offset.min(data.len() as u64) as usize..).unwrap_or(&[]);
            let want = &want[..want.len().min(count as usize)];
            assert_eq!(t.read(&who, 1, offset, count).unwrap(), want, "{path} at {offset}");
        }
        t.clunk(&who, 1).unwrap();
    }
}

#[test]
fn a_read_is_one_range_read_and_a_walk_reads_no_more_than_it_needs() {
    let disk = Memory::holding(image(&tree()));
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    // A walk of one name in a directory of 300: a binary search over its blocks, then its inode.
    let before = disk.reads();
    t.walk(&who, 0, 1, &["lib", "Elixir.Module150.beam"]).unwrap();
    let walk = disk.reads() - before;
    assert!(walk <= 6, "{walk} reads for a walk of two names");
    t.open(&who, 1, mode::OREAD).unwrap();
    // The inode was read once at the walk: open, stat and read read no inode again.
    let before = disk.reads();
    assert_eq!(t.stat(&who, 1).unwrap().1, 150);
    assert_eq!(t.read(&who, 1, 0, 8192).unwrap(), counting(150));
    assert_eq!(disk.reads() - before, 1, "one range read for the file's bytes");
    // A file of whole blocks reads in one call; one whose tail is inline in two.
    t.walk(&who, 0, 2, &["block"]).unwrap();
    t.open(&who, 2, mode::OREAD).unwrap();
    let before = disk.reads();
    assert_eq!(t.read(&who, 2, 0, 8192).unwrap(), counting(4096));
    assert_eq!(disk.reads() - before, 1);
    t.walk(&who, 0, 3, &["big"]).unwrap();
    t.open(&who, 3, mode::OREAD).unwrap();
    let before = disk.reads();
    assert_eq!(t.read(&who, 3, 0, 16384).unwrap(), counting(10_000));
    assert_eq!(disk.reads() - before, 2, "the whole blocks, then the inline tail");
}

#[test]
fn stat_says_the_inode_and_the_name_it_was_walked_by() {
    let disk = Memory::holding(image(&tree()));
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    let (name, length, root, mode) = t.stat(&who, 0).unwrap();
    assert_eq!((name.as_str(), length, mode), ("/", 0, DMDIR | 0o555));
    let paths = t.walk(&who, 0, 1, &["lib", "deep", "notes"]).unwrap();
    let (name, length, path, mode) = t.stat(&who, 1).unwrap();
    assert_eq!((name.as_str(), length, path, mode), ("notes", 4, paths[2], 0o444));
    assert!(paths.iter().all(|p| *p != root) && paths[0] != paths[1] && paths[1] != paths[2]);
    // `..` never leaves the root: from the root it is the root.
    t.walk(&who, 0, 2, &[".."]).unwrap();
    assert_eq!(t.stat(&who, 2).unwrap().2, root);
    assert_eq!(t.walk(&who, 0, 3, &["lib", "deep", "..", ".."]).unwrap()[3], root);
    assert_eq!(t.walk(&who, 0, 4, &["absent"]), Err("file does not exist".into()));
    // A walk past a file stops there (intro(5): the fid is not made).
    assert_eq!(t.walk(&who, 0, 4, &["motd", "x"]).map(|_| ()), Err("partial walk".into()));
}

#[test]
fn a_directory_lists_exactly_its_children_in_order_by_page() {
    let tree = tree();
    let disk = Memory::holding(image(&tree));
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    let mut lib: Vec<StdString> = tree
        .iter()
        .filter_map(|(p, _)| p.strip_prefix("lib/").filter(|n| !n.contains('/')).map(Into::into))
        .collect();
    lib.sort();
    for count in [128, 200, 4096, 8192] {
        t.opened(&who, 1, &["lib"]).unwrap();
        let before = disk.reads();
        assert_eq!(t.list(&who, 1, count).unwrap(), lib, "{count} bytes a read");
        // Each block once, and each entry's inode once, or twice when it did not fit the reply
        // and is asked for again by the next: no listing starts again from the top.
        let reads = disk.reads() - before;
        let bound = if count < 4096 { 2 * lib.len() } else { lib.len() + 16 } + 6;
        assert!(reads <= bound, "{reads} reads at {count} bytes a read");
        t.clunk(&who, 1).unwrap();
    }
    t.opened(&who, 1, &[]).unwrap();
    assert_eq!(t.list(&who, 1, 4096).unwrap(), ["big", "block", "empty", "lib", "motd"]);
}

#[test]
fn every_way_of_writing_is_refused_read_only() {
    let disk = Memory::holding(image(&tree()));
    let before = disk.0.borrow().bytes.clone();
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["motd"]).unwrap();
    for m in [mode::OWRITE, mode::ORDWR, mode::OREAD | mode::OTRUNC] {
        assert_eq!(t.open(&who, 1, m), Err("read-only volume".into()), "mode {m}");
    }
    t.walk(&who, 0, 2, &["lib"]).unwrap();
    let created = t.try_rpc(&who, Body::Tcreate { fid: 2, name: "new", perm: 0o644, mode: mode::OWRITE });
    assert_eq!(created, Err("read-only volume".into()));
    assert_eq!(t.try_rpc(&who, Body::Tremove { fid: 1 }), Err("read-only volume".into()));
    t.walk(&who, 0, 3, &["motd"]).unwrap();
    t.open(&who, 3, mode::OREAD).unwrap();
    let wrote = t.try_rpc(&who, Body::Twrite { fid: 3, offset: 0, data: b"x" });
    assert!(wrote.is_err());
    assert_eq!(disk.0.borrow().bytes, before);
    assert_eq!(t.read(&who, 3, 0, 100).unwrap(), b"hello\n");
}

#[test]
fn the_volumes_labels_are_checked_on_every_node() {
    let disk = Memory::holding(image(&tree()));
    let mut t = T::on(&disk, &[7]);
    assert!(t.attach(&caller(1, &[]), 0).is_err(), "a caller without the volume's label");
    let who = caller(1, &[7]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["lib", "deep", "notes"]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    assert_eq!(t.read(&who, 1, 0, 10).unwrap(), b"deep");
}

#[test]
fn a_volume_that_is_not_in_the_subset_is_served_as_corrupt() {
    let good = image(&tree());
    let mut flipped = good.clone();
    flipped[1024] ^= 1;
    let mut noise = vec![0u8; good.len()];
    noise.iter_mut().enumerate().for_each(|(i, b)| *b = (i * 7 + i / 13) as u8);
    for (what, bytes) in [("a flipped magic", flipped), ("noise", noise), ("a range too short", vec![0; 512])]
    {
        let disk = Memory::holding(bytes);
        let mut t = T::on(&disk, &[]);
        assert!(t.server.fs.is_corrupt(), "{what}");
        assert_eq!(t.attach(&caller(1, &[]), 0), Err("corrupt".into()), "{what}");
        // And again: the server is up, and still says so.
        assert_eq!(t.attach(&caller(2, &[]), 0), Err("corrupt".into()), "{what}");
    }
    // An inode the root names but that is outside the subset: the walk is corrupt, the rest
    // serves on.
    let tree = tree();
    let mut broken = image(&tree);
    let sb = erofs::Superblock::parse(&broken, (broken.len() / BLOCK) as u64).unwrap();
    let disk = Memory::holding(broken.clone());
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    let motd = t.walk(&who, 0, 1, &["motd"]).unwrap()[0];
    let at = sb.inode_at(motd).unwrap() as usize;
    broken[at..at + 2].copy_from_slice(&(3u16 << 1).to_le_bytes());
    let disk = Memory::holding(broken);
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    assert_eq!(t.walk(&who, 0, 1, &["motd"]), Err("corrupt".into()));
    assert!(t.walk(&who, 0, 2, &["block"]).is_ok());
}

#[test]
fn a_range_that_fails_makes_the_volume_corrupt_until_erofsd_starts_again() {
    let disk = Memory::holding(image(&tree()));
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["big"]).unwrap();
    t.open(&who, 1, mode::OREAD).unwrap();
    disk.fail();
    assert_eq!(t.read(&who, 1, 0, 100), Err("corrupt".into()));
    disk.0.borrow_mut().failing = false;
    // The range answers again, but what it would give cannot be trusted: corrupt from now on.
    let reads = disk.reads();
    assert_eq!(t.read(&who, 1, 0, 100), Err("corrupt".into()));
    assert_eq!(t.walk(&who, 0, 2, &["motd"]), Err("corrupt".into()));
    assert_eq!(t.stat(&who, 0), Err("corrupt".into()));
    assert_eq!(t.attach(&who, 3), Err("corrupt".into()));
    assert_eq!(disk.reads(), reads, "a poisoned volume reads nothing");
    // A new start reads it afresh.
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
}

#[test]
fn a_range_that_cannot_be_sized_is_no_volume() {
    let disk = Memory::holding(image(&tree()));
    disk.fail();
    assert!(Erofsd::new(disk, Vec::new()).is_err());
}

/// A kernel for `answer_common`: mints handles 100, 101, ... and remembers the badges.
struct Kernel(Vec<u64>);

impl Minter for Kernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        self.0.push(badge.get());
        Ok(Handle::new(99 + self.0.len() as u32).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> { Ok(0x9e37_79b9_7f4a_7c15 + self.0.len() as u64) }
}

/// A connection minted with `new_connection` at `root` is rooted there: its attach lands there
/// and `..` stops there.
#[test]
fn a_minted_connection_sees_only_below_its_root() {
    let disk = Memory::holding(image(&tree()));
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    let mut kernel = Kernel(Vec::new());
    let message =
        ninep_common::Message::NewConnection(ninep_common::NewConnection { root: "lib/deep", quota: 0 });
    let mut lend = vec![0u8; 4096];
    let words = message.encode(&mut lend).unwrap();
    let outcome = t.server.answer_common(&who, &words, &Handles::new(), &mut lend, &mut kernel);
    assert_eq!(outcome.words[0], 0, "minted");
    let client = Caller { badge: *kernel.0.last().unwrap(), ..who };
    assert!(client.badge >= FIRST_MINTED_BADGE);
    t.attach(&client, 0).unwrap();
    assert_eq!(t.stat(&client, 0).unwrap().0, "deep");
    assert_eq!(t.walk(&client, 0, 1, &["notes"]).map(|p| p.len()), Ok(1));
    // `..` from its root stays there, so the volume's root's `motd` is not reached.
    assert_eq!(t.walk(&client, 0, 2, &["..", "..", "motd"]).map(|_| ()), Err("partial walk".into()));
}

#[test]
fn arguments_it_does_not_understand_stop_it_before_serving() {
    fn ok<'a>(args: &[&'a str]) -> Result<Args<'a>, BadArgs> { parse_args(args.iter().copied()) }
    assert_eq!(ok(&["endpoint=erofsd:system"]), Ok(Args { endpoint: "erofsd:system", labels: Vec::new() }));
    assert_eq!(ok(&["endpoint=e", "labels=3,1"]).map(|a| a.labels), Ok(vec![3, 1]));
    for bad in [
        &[][..],
        &["endpoint=e", "endpoint=f"],
        &["endpoint="],
        &["endpoint=e", "labels=01"],
        &["endpoint=e", "labels=1,1"],
        &["endpoint=e", "labels="],
        &["endpoint=e", "labels=1", "labels=2"],
        &["endpoint=e", "volume=x"],
    ] {
        assert_eq!(ok(bad), Err(BadArgs), "{bad:?}");
    }
}

#[test]
fn the_conformance_vectors_run_against_erofsd() {
    let tree = tree();
    let disk = Memory::holding(image(&tree));
    let before = disk.0.borrow().bytes.clone();
    let mut t = T::on(&disk, &[]);
    let who = caller(1, &[]);
    let counts = redoubt_fake_kernel::vectors::run(&mut t.server, &who);
    assert!(counts.well_formed > 20 && counts.malformed > 5, "{counts:?}");
    assert_eq!(disk.0.borrow().bytes, before);
    assert!(!t.server.fs.is_corrupt());
}

#[test]
fn the_limits_fit_the_budget() {
    assert!(limits(4).fits(&COST, BUDGET));
    assert!(!limits(5).fits(&COST, BUDGET));
}
