//! Power loss at every block write: for each operation of fixed and random workloads, the device
//! is cut at its first write, then its second, and so on to its last; each time, writes since the
//! last `sync` land whole, not at all or torn, in any order. The next mount recovers the log and
//! must read the model's state before the operation or after it, nothing else, and check sound. A
//! second cut inside that recovery must leave the same.

mod common;

use common::ops::*;
use common::*;
use walfs::{BLOCK, Error, Filesystem, OpenOptions};

/// Shapes that are one transaction each: a create, a write of at most two blocks, a truncate, and
/// the directory and attribute operations.
fn single(rng: &mut Rng, tree: &Tree, p: &Profile) -> Option<Op> {
    Some(match generate(rng, tree, p)? {
        Op::Write { path, .. } if !tree.contains_key(&path) => Op::Write { path, data: Vec::new() },
        Op::Write { path, data } | Op::Patch { path, data, cut: None, .. } => {
            let Some(Node::File { data: old, .. }) = tree.get(&path) else { return None };
            let at = p.at(rng, old.len() as u64);
            Op::Patch { path, at, data: data[..data.len().min(2 * BLOCK)].to_vec(), cut: None }
        }
        Op::Patch { path, cut, .. } => Op::Patch { path, at: 0, data: Vec::new(), cut },
        op => op,
    })
}

/// The writes `op` makes on `image`.
fn writes(image: &[u8], op: &Op) -> u64 {
    let mut ram = Ram::from_image(image.to_vec());
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    apply(&mut fs, op).unwrap();
    drop(fs);
    ram.writes
}

/// The volume `image` holds, mounted, checked and read whole.
fn mounted(image: Vec<u8>, what: &str) -> (Vec<u8>, Tree) {
    let mut ram = Ram::from_image(image);
    let mut fs = Filesystem::mount(&mut ram).unwrap_or_else(|e| panic!("{what}: mount: {e:?}"));
    let tree = sound(&mut fs, what);
    drop(fs);
    (ram.data, tree)
}

/// Cuts `op` at every write it makes on `image`, whose volume holds `before`; checks each mount
/// against `before` and `after`, and, if `twice`, cuts each recovery at every write too.
fn cut_everywhere(image: &[u8], before: &Tree, after: &Tree, op: &Op, twice: bool, what: &str) {
    for n in 1..=writes(image, op) {
        let what = format!("{what}, cut at write {n}");
        let mut ram = Ram::from_image(image.to_vec()).failing_at(n, n * 7919);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        let r = apply(&mut fs, op);
        drop(fs);
        assert!(matches!(r, Err(Error::Io)), "{what}: {op:?} gave {r:?}");
        let cut = ram.data;
        if twice {
            for m in 1.. {
                let mut ram = Ram::from_image(cut.clone()).failing_at(m, m * 31);
                match Filesystem::mount(&mut ram) {
                    Ok(_) => break,
                    Err(Error::Io) => {}
                    Err(e) => panic!("{what}, recovery cut at write {m}: {e:?}"),
                }
                let (_, seen) = mounted(ram.data, &format!("{what}, recovery cut at write {m}"));
                assert!(
                    seen == *before || seen == *after,
                    "{what}, recovery cut at {m}: {:#?}",
                    tree_diff(&seen, after)
                );
            }
        }
        let (_, seen) = mounted(cut, &what);
        assert!(
            seen == *before || seen == *after,
            "{what}: {op:?}: neither before ({:#?}) nor after ({:#?})",
            tree_diff(&seen, before),
            tree_diff(&seen, after)
        );
    }
}

/// Runs `ops` from an empty volume of `blocks` blocks, cutting each at every write.
fn workload(blocks: u32, ops: &[Op], twice: bool, what: &str) {
    let mut image = Ram::formatted(blocks).data;
    let mut tree = empty_tree();
    for (k, op) in ops.iter().enumerate() {
        assert_eq!(expect(&tree, op), Ok(()), "{what} op {k}: {op:?}");
        let mut after = tree.clone();
        apply_model(&mut after, op, Done::All);
        cut_everywhere(&image, &tree, &after, op, twice, &format!("{what} op {k} {:.80}", format!("{op:?}")));
        let mut ram = Ram::from_image(image);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert_eq!(apply(&mut fs, op), Ok(Done::All), "{what} op {k}");
        drop(fs);
        let (img, seen) = mounted(ram.data, what);
        assert!(seen == after, "{what} op {k}: {:#?}", tree_diff(&seen, &after));
        (image, tree) = (img, after);
    }
}

fn s(x: &str) -> String { x.to_string() }

/// Every kind of operation, files past the direct and single-indirect blocks, renames over files
/// and across directories, and the removal of a file large enough that freeing it takes several
/// transactions.
fn fixed() -> Vec<Op> {
    let blk = |b: u8| vec![b; BLOCK];
    vec![
        Op::Mkdir(s("/d")),
        Op::Write { path: s("/d/f"), data: vec![] },
        Op::Patch { path: s("/d/f"), at: 0, data: [blk(1), blk(2)].concat(), cut: None },
        Op::Patch { path: s("/d/f"), at: 100, data: b"in place".to_vec(), cut: None },
        Op::Patch { path: s("/d/f"), at: 49_000, data: blk(3), cut: None },
        Op::Patch { path: s("/d/f"), at: 4_300_000, data: blk(4), cut: None },
        Op::SetAttr(s("/d/f"), 1, vec![5; 200]),
        Op::Write { path: s("/g"), data: vec![] },
        Op::Patch { path: s("/g"), at: 0, data: b"g".to_vec(), cut: None },
        Op::Rename(s("/g"), s("/d/f")),
        Op::Write { path: s("/big"), data: vec![] },
        Op::Patch { path: s("/big"), at: 4_000_000, data: blk(6), cut: None },
        Op::Patch { path: s("/big"), at: 0, data: blk(7), cut: None },
        Op::Patch { path: s("/big"), at: 0, data: vec![], cut: Some(5000) },
        Op::Mkdir(s("/e")),
        Op::Rename(s("/d"), s("/e/d")),
        Op::RemoveAttr(s("/e/d/f"), 1),
        Op::Remove(s("/e/d/f")),
        Op::Remove(s("/big")),
        Op::Remove(s("/e/d")),
    ]
}

#[test]
fn crash_at_every_write_fixed_workload() { workload(512, &fixed(), false, "fixed"); }

/// A cut while recovery copies a committed transaction home, at every write of it.
#[test]
fn crash_during_recovery() { workload(512, &fixed(), true, "fixed, recovery cut"); }

#[test]
fn crash_at_every_write_random_workloads() {
    let seeds: u64 = std::env::var("CRASH_SEEDS").map_or(4, |s| s.parse().unwrap());
    let p = Profile { sizes: &[0, 1, 300, 4096, 5000, 8192], ..Profile::default() };
    for seed in 1..=seeds {
        let mut rng = Rng(seed);
        let mut tree = empty_tree();
        let mut ops = Vec::new();
        while ops.len() < 40 {
            if let Some(op) = single(&mut rng, &tree, &p).filter(|op| expect(&tree, op).is_ok()) {
                apply_model(&mut tree, &op, Done::All);
                ops.push(op);
            }
        }
        workload(1024, &ops, false, &format!("seed {seed}"));
    }
}

/// Removing a sparse file whose blocks lie under many indirect blocks frees them over several
/// transactions; a cut in any of them, or in the mount that then finishes the freeing, leaves the
/// file there whole or gone, and once gone, the mount frees the rest.
#[test]
fn crash_while_freeing_a_large_file() {
    let mut ram = Ram::formatted(512);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let h = fs.open("/big", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    let free = fs.free_blocks();
    let at = |j: u64| (12 + 1024 + j * 1024) * BLOCK as u64;
    for j in 0..40 {
        fs.seek(h, at(j)).unwrap();
        fs.write(h, &[j as u8 + 1]).unwrap();
    }
    fs.close(h).unwrap();
    let held = free - fs.free_blocks();
    drop(fs);
    let image = ram.data;
    let op = Op::Remove(s("/big"));
    let total = writes(&image, &op);
    assert!(total > 2 * walfs::LOG_BLOCKS as u64 + 2, "freeing took {total} writes, one transaction");
    // The volume after a cut: the file there whole, or gone with every block it held free.
    let sound_after = |image: Vec<u8>, what: &str| {
        let mut ram = Ram::from_image(image);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert!(fs.check().unwrap().is_empty(), "{what}");
        match fs.stat("/big") {
            Ok(m) => {
                assert_eq!((m.size, fs.free_blocks()), (at(39) + 1, free - held), "{what}");
                let h = fs.open("/big", OpenOptions { read: true, ..Default::default() }).unwrap();
                fs.seek(h, at(17)).unwrap();
                let mut b = [0u8; 2];
                assert_eq!((fs.read(h, &mut b).unwrap(), b), (2, [18, 0]), "{what}");
            }
            Err(Error::NoEntry) => assert_eq!(fs.free_blocks(), free, "{what}"),
            Err(e) => panic!("{what}: {e:?}"),
        }
    };
    for n in 1..=total {
        let mut ram = Ram::from_image(image.clone()).failing_at(n, n);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert_eq!(apply(&mut fs, &op), Err(Error::Io));
        drop(fs);
        let cut = ram.data;
        // Every seventh cut, the mount that finishes the freeing is cut too, at each of its writes.
        if n % 7 == 0 {
            for m in 1.. {
                let mut ram = Ram::from_image(cut.clone()).failing_at(m, m * 31);
                match Filesystem::mount(&mut ram) {
                    Ok(_) => break,
                    Err(Error::Io) => sound_after(ram.data, &format!("cut at {n}, the mount cut at {m}")),
                    Err(e) => panic!("cut at {n}, the mount cut at {m}: {e:?}"),
                }
            }
        }
        sound_after(cut, &format!("cut at {n}"));
    }
}

/// A write of many transactions cut in any of them leaves the file as before, overwritten by a
/// prefix of the write that ends where a transaction ended.
#[test]
fn crash_inside_a_write_of_many_transactions_leaves_a_prefix() {
    let mut ram = Ram::formatted(1024);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    write_file(&mut fs, "/f", &vec![1u8; 30_000]).unwrap();
    drop(fs);
    let image = ram.data;
    let new = vec![2u8; 300_000];
    let op = Op::Patch { path: s("/f"), at: 10_000, data: new.clone(), cut: None };
    let mut prefixes = std::collections::BTreeSet::new();
    for n in 1..=writes(&image, &op) {
        let mut ram = Ram::from_image(image.clone()).failing_at(n, n);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert_eq!(apply(&mut fs, &op), Err(Error::Io));
        drop(fs);
        let mut ram = Ram::from_image(ram.data);
        let mut fs = Filesystem::mount(&mut ram).unwrap();
        assert!(fs.check().unwrap().is_empty(), "cut at {n}");
        let got = read_file(&mut fs, "/f").unwrap();
        let k = got[10_000..].iter().take_while(|&&b| b == 2).count();
        let mut want = vec![1u8; 30_000];
        want.resize(30_000.max(10_000 + k), 0);
        want[10_000..10_000 + k].fill(2);
        assert!(got == want, "cut at {n}: not the file with a prefix of {k} bytes written");
        assert!(
            k == 0 || k == new.len() || (10_000 + k) % BLOCK == 0,
            "cut at {n}: a prefix of {k} bytes ends inside a block"
        );
        prefixes.insert(k);
    }
    assert!(prefixes.len() > 3, "the write took several transactions: {prefixes:?}");
}

/// A torn header, a torn logged block or a lost home write: the log's hashes tell each apart.
#[test]
fn a_torn_log_is_dropped_or_replayed_never_half_applied() {
    let mut ram = Ram::formatted(256);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let h = fs.open("/f", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    fs.write(h, b"x").unwrap();
    fs.close(h).unwrap();
    drop(fs);
    let before = mounted(ram.data.clone(), "before").1;
    let op = Op::Patch { path: s("/f"), at: 0, data: vec![9; 6000], cut: None };
    let mut after = before.clone();
    apply_model(&mut after, &op, Done::All);
    // Seeds pick what lands at the cut; many of them reach each of the outcomes.
    for seed in 0..64u64 {
        let image = ram.data.clone();
        for n in 1..=writes(&image, &op) {
            let mut r = Ram::from_image(image.clone()).failing_at(n, seed * 1000 + n);
            let mut fs = Filesystem::mount(&mut r).unwrap();
            let _ = apply(&mut fs, &op);
            drop(fs);
            let (_, seen) = mounted(r.data, &format!("seed {seed} cut {n}"));
            assert!(seen == before || seen == after, "seed {seed} cut {n}");
        }
    }
}
