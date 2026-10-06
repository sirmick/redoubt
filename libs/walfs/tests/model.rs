//! Model-based tests: random operations, valid or not, against the file system and against an
//! in-memory model of what it should hold; after every operation the outcomes must agree, and at
//! intervals the volume is remounted, dumped, compared and checked.

mod common;

use common::ops::*;
use common::*;
use walfs::{Error, FileHandle, Filesystem, Geometry, OpenOptions};

/// Mostly paths that exist or whose parent exists, so most operations get somewhere; now and then
/// a random one, so errors are exercised too. Empty: nothing to pick from.
fn random_path(rng: &mut Rng, tree: &Tree, p: &Profile) -> String {
    let name = |rng: &mut Rng| p.name(rng);
    match rng.below(4) {
        0 => pick(rng, tree, |k, _| !k.is_empty()).cloned().unwrap_or_default(),
        1 | 2 => match pick(rng, tree, |_, v| matches!(v, Node::Dir { .. })) {
            Some(dir) => format!("{dir}/{}", name(rng)),
            None => String::new(),
        },
        _ => (0..1 + rng.below(3)).map(|_| format!("/{}", name(rng))).collect(),
    }
}

/// A random operation on `path`, which need not exist or be of the right kind.
fn random_op(rng: &mut Rng, tree: &Tree, path: String, p: &Profile) -> Op {
    match rng.below(10) {
        0 | 1 => {
            let n = p.size(rng);
            Op::Write { path, data: rng.bytes(n) }
        }
        2 | 3 => {
            let old = match tree.get(&path) {
                Some(Node::File { data, .. }) => data.len() as u64,
                _ => 0,
            };
            let at = p.at(rng, old);
            let n = p.size(rng);
            let data = rng.bytes(n);
            let cut = (rng.below(3) == 0).then(|| rng.below(old + 4 * p.grow));
            Op::Patch { path, at, data, cut }
        }
        4 => Op::Mkdir(path),
        5 => Op::Remove(path),
        6 | 7 => {
            let to = random_path(rng, tree, p);
            Op::Rename(path, if to.is_empty() { "/x".to_string() } else { to })
        }
        _ => {
            let t = ATTR_TYPES[rng.below(ATTR_TYPES.len() as u64) as usize];
            let target = if rng.below(8) == 0 { String::new() } else { path };
            if rng.below(3) == 0 {
                Op::RemoveAttr(target, t)
            } else {
                let n = rng.below(p.max_attr) as usize;
                Op::SetAttr(target, t, rng.bytes(n))
            }
        }
    }
}

/// A handle held open across other operations: where its file is now (`None` once removed or
/// replaced), and what the file holds, which a write through it changes at once.
struct Held {
    h: FileHandle,
    /// The file's inode: handles on one file share what it holds.
    ino: u32,
    path: Option<String>,
    data: Vec<u8>,
}

/// With MODEL_TRACE set, prints what the test does.
fn trace(f: impl FnOnce() -> String) {
    if std::env::var_os("MODEL_TRACE").is_some() {
        eprintln!("{}", f());
    }
}

/// Opens, writes and reads through, or closes a handle held across other operations.
fn handle_step(
    fs: &mut Filesystem<&mut Ram>,
    rng: &mut Rng,
    p: &Profile,
    tree: &mut Tree,
    held: &mut Vec<Held>,
    what: &str,
) {
    match rng.below(4) {
        0 if held.len() < 4 => {
            let Some(path) = pick(rng, tree, |_, v| matches!(v, Node::File { .. })).cloned() else { return };
            let Some(Node::File { data, .. }) = tree.get(&path) else { return };
            let h = fs.open(&path, OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
            let ino = fs.stat(&path).unwrap().inode;
            trace(|| format!("{what}: open {path} as {h:?}"));
            held.push(Held { h, ino, path: Some(path), data: data.clone() });
        }
        0 | 1 if !held.is_empty() => {
            let k = rng.below(held.len() as u64) as usize;
            let at = p.at(rng, held[k].data.len() as u64);
            let n = p.size(rng);
            let bytes = rng.bytes(n);
            fs.seek(held[k].h, at).unwrap();
            let n = match fs.write(held[k].h, &bytes) {
                Ok(n) => n,
                Err(Error::NoSpace) => 0,
                Err(e) => panic!("{what}: handle write: {e:?}"),
            };
            trace(|| format!("{what}: write {n} of {} at {at} through {:?}", bytes.len(), held[k].h));
            if n > 0 {
                let d = &mut held[k].data;
                if d.len() < at as usize + n {
                    d.resize(at as usize + n, 0);
                }
                d[at as usize..at as usize + n].copy_from_slice(&bytes[..n]);
            }
            let (ino, now) = (held[k].ino, held[k].data.clone());
            for other in held.iter_mut().filter(|o| o.ino == ino) {
                other.data = now.clone();
            }
            if let Some(path) = &held[k].path {
                if let Some(Node::File { data, .. }) = tree.get_mut(path) {
                    *data = now;
                }
            }
        }
        2 if !held.is_empty() => {
            let k = rng.below(held.len() as u64) as usize;
            fs.seek(held[k].h, 0).unwrap();
            let mut buf = vec![0u8; held[k].data.len() + 10];
            let mut got = 0;
            loop {
                let n = fs.read(held[k].h, &mut buf[got..]).unwrap();
                if n == 0 {
                    break;
                }
                got += n;
            }
            assert!(
                buf[..got] == held[k].data[..],
                "{what}: read through {:?} ({:?})",
                held[k].h,
                held[k].path
            );
        }
        _ if !held.is_empty() => {
            let k = rng.below(held.len() as u64) as usize;
            let h = held.remove(k);
            trace(|| format!("{what}: close {:?} ({:?})", h.h, h.path));
            fs.close(h.h).unwrap();
        }
        _ => {}
    }
}

/// What an operation does to the handles held open: removal and replacement detach them, renames
/// carry them; the files still named are read again from the model.
fn follow(held: &mut [Held], tree: &Tree, op: &Op) {
    for h in held.iter_mut() {
        let Some(p) = h.path.clone() else { continue };
        match op {
            Op::Remove(r) if *r == p => h.path = None,
            Op::Rename(from, to) if from != to => {
                if p == *from || p.starts_with(&format!("{from}/")) {
                    h.path = Some(format!("{to}{}", &p[from.len()..]));
                } else if p == *to {
                    h.path = None;
                }
            }
            _ => {}
        }
        if let Some(Node::File { data, .. }) = h.path.as_ref().and_then(|p| tree.get(p)) {
            h.data = data.clone();
        }
    }
}

fn run(blocks: u32, p: Profile, seed: u64, steps: usize) {
    let mut ram = Ram::formatted(blocks);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let mut tree = empty_tree();
    let mut rng = Rng(seed);
    let mut held: Vec<Held> = Vec::new();

    for step in 0..steps {
        let what = format!("seed {seed} step {step}");
        if rng.below(4) == 0 {
            handle_step(&mut fs, &mut rng, &p, &mut tree, &mut held, &what);
            continue;
        }
        let path = random_path(&mut rng, &tree, &p);
        if path.is_empty() {
            continue;
        }
        let op = random_op(&mut rng, &tree, path, &p);
        let want = expect(&tree, &op);
        trace(|| format!("{what}: {:.120} -> {want:?}", format!("{op:?}")));
        match (want, apply(&mut fs, &op)) {
            (Ok(()), Ok(done)) => {
                apply_model(&mut tree, &op, done);
                follow(&mut held, &tree, &op);
            }
            // A full volume: an operation of one transaction changed nothing.
            (Ok(()), Err(Error::NoSpace)) => {}
            (want, got) => assert_eq!(want, got.map(|_| ()), "{what}: {op:?}"),
        }

        if step % 50 == 49 {
            for h in held.drain(..) {
                fs.close(h.h).unwrap();
            }
            drop(fs);
            fs = Filesystem::mount(&mut ram).unwrap();
            let seen = sound(&mut fs, &what);
            assert!(seen == tree, "{what}: {:#?}", tree_diff(&seen, &tree));
        }
    }
    for h in held.drain(..) {
        fs.close(h.h).unwrap();
    }
    let seen = sound(&mut fs, &format!("seed {seed} at the end"));
    assert!(seen == tree, "seed {seed} at the end: {:#?}", tree_diff(&seen, &tree));
    eprintln!("seed {seed}: {} paths, {} blocks free at the end", seen.len(), fs.free_blocks());
}

/// A volume of 1 MiB: full often.
#[test]
fn random_operations_small_volume() {
    for seed in 1..=6 {
        run(256, Profile::default(), seed, 1500);
    }
}

/// A volume of 32 MiB: deep trees, files past the direct and the single-indirect blocks.
#[test]
fn random_operations_large_volume() {
    for seed in 10..=13 {
        run(8192, Profile::default(), seed, 1500);
    }
}

/// Many small files in few directories, which grow past one block, on a volume soon full, with
/// handles held open.
#[test]
fn random_operations_crowded_small_volume() {
    let seeds: u64 = std::env::var("MODEL_SEEDS").map_or(20, |s| s.parse().unwrap());
    for seed in 30..30 + seeds {
        run(96, Profile::crowded(), seed, 1500);
    }
}

/// Filling the volume ends a write short and leaves it usable; removing the file gives every
/// block back.
#[test]
fn full_volume() {
    let mut ram = Ram::formatted(128);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    write_file(&mut fs, "/keep", b"survives").unwrap();
    let free = fs.free_blocks();
    let h = fs.open("/big", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    let chunk = vec![7u8; 100_000];
    let mut total = 0;
    let err = loop {
        match fs.write(h, &chunk) {
            Ok(n) => total += n,
            Err(e) => break e,
        }
    };
    assert_eq!(err, Error::NoSpace);
    assert_eq!(fs.file_size(h).unwrap(), total as u64);
    assert!(fs.free_blocks() == 0 && total >= 250_000, "{total} bytes written");
    fs.close(h).unwrap();
    assert_eq!(read_file(&mut fs, "/keep").unwrap(), b"survives");
    assert_eq!(read_file(&mut fs, "/big").unwrap().len(), total);
    fs.remove("/big").unwrap();
    assert_eq!(fs.free_blocks(), free);
    write_file(&mut fs, "/after", &vec![1u8; 30_000]).unwrap();
    sound(&mut fs, "full volume");
}

/// Open handles follow renames, and a removed file stays readable and writable through them until
/// the last closes, which frees its blocks.
#[test]
fn handles_follow_renames_and_outlive_removal() {
    let mut ram = Ram::formatted(256);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    fs.mkdir("/x").unwrap();
    let free = fs.free_blocks();
    let rw = OpenOptions { read: true, write: true, create: true, ..Default::default() };
    let h = fs.open("/f", rw).unwrap();
    fs.write(h, b"hello").unwrap();
    fs.rename("/f", "/x/g").unwrap();
    fs.write(h, b" world").unwrap();
    assert_eq!(read_file(&mut fs, "/x/g").unwrap(), b"hello world");
    assert_eq!(fs.stat("/f"), Err(Error::NoEntry));

    fs.remove("/x/g").unwrap();
    fs.write(h, &vec![3u8; 20_000]).unwrap();
    fs.seek(h, 0).unwrap();
    let mut buf = [0u8; 11];
    assert_eq!(fs.read(h, &mut buf).unwrap(), 11);
    assert_eq!(&buf, b"hello world");
    assert!(fs.free_blocks() < free - 5);
    assert!(fs.check().unwrap().is_empty());
    fs.close(h).unwrap();
    fs.remove("/x").unwrap();
    assert_eq!(fs.free_blocks(), free);
    sound(&mut fs, "handles");
}

/// A file open when the volume is unmounted and was removed is freed by the next mount.
#[test]
fn an_orphan_open_at_unmount_is_freed_at_mount() {
    let mut ram = Ram::formatted(256);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    write_file(&mut fs, "/e", b"").unwrap();
    let free = fs.free_blocks();
    write_file(&mut fs, "/f", &vec![9u8; 50_000]).unwrap();
    let h = fs.open("/f", OpenOptions { read: true, ..Default::default() }).unwrap();
    fs.remove("/f").unwrap();
    assert!(fs.free_blocks() < free);
    let _ = h;
    drop(fs);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert_eq!(fs.free_blocks(), free);
    sound(&mut fs, "orphan at mount");
}

/// Truncating and removing files past the direct and the single-indirect blocks gives every block
/// back, sparse or not.
#[test]
fn large_and_sparse_files_free_every_block() {
    let mut ram = Ram::formatted(8192);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    let rw = OpenOptions { read: true, write: true, create: true, ..Default::default() };
    let h = fs.open("/big", rw).unwrap();
    let free = fs.free_blocks();
    fs.write(h, &vec![1u8; 6_000_000]).unwrap();
    fs.seek(h, 3_000_000_000).unwrap();
    fs.write(h, b"far").unwrap();
    assert_eq!(fs.file_size(h).unwrap(), 3_000_000_003);
    fs.truncate(h, 5_000_001).unwrap();
    fs.seek(h, 4_999_999).unwrap();
    let mut buf = [9u8; 4];
    assert_eq!(fs.read(h, &mut buf).unwrap(), 2);
    assert_eq!(buf[..2], [1, 1]);
    fs.truncate(h, 5_000_100).unwrap();
    fs.seek(h, 5_000_000).unwrap();
    assert_eq!(fs.read(h, &mut buf).unwrap(), 4);
    assert_eq!(buf, [1, 0, 0, 0], "the cut tail reads as zeros when the file grows again");
    fs.close(h).unwrap();
    assert!(fs.check().unwrap().is_empty());
    fs.remove("/big").unwrap();
    assert_eq!(fs.free_blocks(), free);
    sound(&mut fs, "large files");
}

#[test]
fn bad_arguments() {
    let mut ram = Ram::formatted(64);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    assert_eq!(fs.mkdir("/a/.."), Err(Error::Invalid));
    assert_eq!(fs.mkdir("/a\0b"), Err(Error::Invalid));
    assert_eq!(fs.mkdir("/"), Err(Error::Exists));
    assert_eq!(fs.remove("/"), Err(Error::Invalid));
    assert_eq!(fs.rename("/", "/x"), Err(Error::Invalid));
    assert_eq!(fs.mkdir(&"n".repeat(256)), Err(Error::NameTooLong));
    fs.mkdir(&"n".repeat(255)).unwrap();
    fs.mkdir("/d").unwrap();
    assert_eq!(fs.rename("/d", "/d/e"), Err(Error::Invalid));
    assert_eq!(fs.open("/d", OpenOptions { read: true, ..Default::default() }), Err(Error::IsDir));
    assert_eq!(fs.open("/", OpenOptions { read: true, ..Default::default() }), Err(Error::IsDir));
    assert_eq!(
        fs.open("/nope", OpenOptions { read: true, create: true, ..Default::default() }),
        Err(Error::Invalid)
    );
    assert_eq!(fs.set_attr("/d", 1, &[0; 255]), Err(Error::NoSpace));
    fs.set_attr("/d", 1, &[0; 254]).unwrap();
    assert_eq!(fs.set_attr("/d", 2, &[]), Err(Error::NoSpace), "the area is full");
    assert_eq!(fs.set_attr("/d", 0, b"x"), Err(Error::Invalid));
    assert_eq!(fs.get_attr("/d", 2), Err(Error::NoAttr));
    let h =
        fs.open("/f", OpenOptions { read: true, write: true, create: true, ..Default::default() }).unwrap();
    assert_eq!(fs.seek(h, walfs::MAX_FILE_SIZE + 1), Err(Error::FileTooBig));
    fs.seek(h, walfs::MAX_FILE_SIZE - 1).unwrap();
    assert_eq!(fs.write(h, b"ab"), Err(Error::FileTooBig));
    assert_eq!(fs.write(h, b"a"), Ok(1));
    assert_eq!(fs.truncate(h, walfs::MAX_FILE_SIZE + 1), Err(Error::FileTooBig));
    fs.close(h).unwrap();
    assert_eq!(fs.close(h), Err(Error::Invalid));
    let r = fs.open("/f", OpenOptions { read: true, ..Default::default() }).unwrap();
    assert_eq!(fs.write(r, b"x"), Err(Error::Invalid));
    assert_eq!(fs.truncate(r, 0), Err(Error::Invalid));
    fs.close(r).unwrap();
    sound(&mut fs, "bad arguments");
    assert_eq!(
        Filesystem::format(&mut Ram::new(64), Geometry { block_count: 64, inode_count: 33 }),
        Err(Error::Invalid)
    );
    assert_eq!(
        Filesystem::format(&mut Ram::new(64), Geometry { block_count: 40, inode_count: 32 }),
        Err(Error::Invalid)
    );
    assert_eq!(
        Filesystem::format(&mut Ram::new(39), Geometry { block_count: 39, inode_count: 32 }),
        Err(Error::Invalid)
    );
}

/// A name long enough to fill an entry, many of them, in a directory of several blocks; and a
/// directory emptied keeps working.
#[test]
fn directories_grow_past_their_direct_blocks() {
    let mut ram = Ram::new(1024);
    Filesystem::format(&mut ram, Geometry { block_count: 1024, inode_count: 512 }).unwrap();
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    fs.mkdir("/d").unwrap();
    // 15 entries a block: 250 entries take 17 blocks, past the 12 direct ones.
    for i in 0..250 {
        let name = format!("/d/{i:03}{}", "x".repeat(252));
        write_file(&mut fs, &name, format!("{i}").as_bytes()).unwrap();
    }
    assert_eq!(fs.stat("/d").unwrap().size, 17 * 4096);
    let mut n = 0;
    fs.read_dir("/d", |e| {
        assert_eq!(e.name.len(), 255);
        n += 1
    })
    .unwrap();
    assert_eq!(n, 250);
    sound(&mut fs, "big directory");
    for i in 0..250 {
        fs.remove(&format!("/d/{i:03}{}", "x".repeat(252))).unwrap();
    }
    write_file(&mut fs, "/d/again", b"a").unwrap();
    assert_eq!(fs.remove("/d"), Err(Error::NotEmpty));
    fs.remove("/d/again").unwrap();
    fs.remove("/d").unwrap();
    sound(&mut fs, "emptied directory");
}

/// The generation counts an inode's allocations, and mtime is what the clock says.
#[test]
fn generations_and_mtimes() {
    let mut ram = Ram::formatted(128);
    let mut fs = Filesystem::mount(&mut ram).unwrap();
    write_file(&mut fs, "/a", b"1").unwrap();
    let first = fs.stat("/a").unwrap();
    assert_eq!(first.mtime, 0);
    fs.remove("/a").unwrap();
    fs.set_time(1_700_000_000_000_000);
    // Inodes are taken in turn, so the next file takes another: go round to the first.
    let mut seen = None;
    for k in 0..40 {
        let path = format!("/f{k}");
        write_file(&mut fs, &path, b"2").unwrap();
        let m = fs.stat(&path).unwrap();
        assert_eq!(m.mtime, 1_700_000_000_000_000);
        if m.inode == first.inode {
            seen = Some(m);
            break;
        }
        fs.remove(&path).unwrap();
    }
    let again = seen.expect("the inode is used again");
    assert_eq!(again.generation, first.generation + 1);
}
