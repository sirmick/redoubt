//! Model-based tests: random operations, valid or not, against the filesystem and against
//! an in-memory model of what it should hold; after every operation the outcomes must
//! agree, and at intervals the volume is remounted, dumped, compared and checked.

mod common;

use common::ops::*;
use common::*;
use littlefs::{Config, Error, FileHandle, Filesystem, OpenOptions};

/// Mostly paths that exist or whose parent exists, so most operations get somewhere; now
/// and then a random one, so errors are exercised too. Empty: nothing to pick from.
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
    let size = |rng: &mut Rng| p.size(rng);
    match rng.below(10) {
        0 | 1 => {
            let n = size(rng);
            Op::Write { path, data: rng.bytes(n) }
        }
        2 => {
            let old = match tree.get(&path) {
                Some(Node::File { data, .. }) => data.len() as u64,
                _ => 0,
            };
            let at = rng.below(old + p.grow) as u32;
            let n = size(rng);
            let data = rng.bytes(n);
            let cut = (rng.below(3) == 0).then(|| rng.below(old + 4 * p.grow) as u32);
            Op::Patch { path, at, data, cut }
        }
        3 => Op::Mkdir(path),
        4 | 5 => Op::Remove(path),
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
                // A long name, inline data and three attributes must fit one metadata block.
                let n = rng.below(p.max_attr) as usize;
                Op::SetAttr(target, t, rng.bytes(n))
            }
        }
    }
}

/// A handle held open across other operations: where its file is now (`None` once removed
/// or replaced), and what the handle's view of the file holds.
struct Held {
    h: FileHandle,
    path: Option<String>,
    data: Vec<u8>,
    dirty: bool,
    /// A write through it failed: closing reports the error and commits nothing.
    erred: bool,
}

/// Opens, writes through or closes a handle held across other operations. Closing commits
/// the handle's whole view of the file to wherever the file is now, if it still exists.
fn handle_step(fs: &mut Filesystem<&mut Ram>, rng: &mut Rng, p: &Profile, tree: &mut Tree, held: &mut Vec<Held>, what: &str) {
    match rng.below(3) {
        0 if held.len() < 4 => {
            let Some(path) = pick(rng, tree, |_, v| matches!(v, Node::File { .. })).cloned() else { return };
            let Some(Node::File { data, .. }) = tree.get(&path) else { return };
            let h = fs.open(&path, OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
            trace(|| format!("{what}: open {path} as {h:?}"));
            held.push(Held { h, path: Some(path), data: data.clone(), dirty: false, erred: false });
        }
        0 | 1 if held.iter().any(|h| !h.erred) => {
            let live: Vec<usize> = (0..held.len()).filter(|&k| !held[k].erred).collect();
            let k = live[rng.below(live.len() as u64) as usize];
            let at = rng.below(held[k].data.len() as u64 + p.grow) as usize;
            let n = p.size(rng);
            let bytes = rng.bytes(n);
            // Seeking flushes pending writes, which may need blocks.
            match fs.seek(held[k].h, at as u32).and_then(|()| fs.write(held[k].h, &bytes)) {
                Ok(_) => {}
                // A full volume: the handle is errored and commits nothing from now on.
                Err(Error::NoSpace) => {
                    trace(|| format!("{what}: write through {:?}: NoSpace", held[k].h));
                    held[k].erred = true;
                    return;
                }
                Err(e) => panic!("{what}: handle write: {e:?}"),
            }
            let d = &mut held[k].data;
            // Writing nothing inside the file changes nothing; past the end it still fills
            // the gap with zeros (as the reference does).
            if n == 0 && at <= d.len() {
                return;
            }
            if d.len() < at + n {
                d.resize(at + n, 0);
            }
            d[at..at + n].copy_from_slice(&bytes);
            held[k].dirty = true;
            trace(|| format!("{what}: write {n} at {at} through {:?} ({:?})", held[k].h, held[k].path));
        }
        _ if !held.is_empty() => {
            let k = rng.below(held.len() as u64) as usize;
            close_held(fs, tree, held.remove(k), what);
        }
        _ => {}
    }
}

/// With MODEL_TRACE set, prints what the test does.
fn trace(f: impl FnOnce() -> String) {
    if std::env::var_os("MODEL_TRACE").is_some() {
        eprintln!("{}", f());
    }
}

fn close_held(fs: &mut Filesystem<&mut Ram>, tree: &mut Tree, h: Held, what: &str) {
    trace(|| format!("{what}: close {:?} ({:?}, dirty {})", h.h, h.path, h.dirty));
    if h.erred {
        assert_eq!(fs.close(h.h), Err(Error::NoSpace), "{what}: close of an errored handle");
        return;
    }
    match fs.close(h.h) {
        Ok(()) => {}
        // The flush or the commit found the volume full: nothing was committed.
        Err(Error::NoSpace) => return,
        Err(e) => panic!("{what}: close: {e:?}"),
    }
    if let (Some(path), true) = (h.path, h.dirty) {
        match tree.get_mut(&path) {
            Some(Node::File { data, .. }) => *data = h.data,
            other => panic!("{what}: held file {path} is {other:?} in the model"),
        }
    }
}

/// What an operation does to the handles held open: removal detaches, renames carry them.
fn follow(held: &mut [Held], op: &Op) {
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
    }
}

fn run(cfg: Config, p: Profile, seed: u64, steps: usize) {
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    let mut tree = dump(&mut fs).unwrap();
    let mut rng = Rng(seed);

    let mut held: Vec<Held> = Vec::new();

    for step in 0..steps {
        // Debugging aid: check the whole volume before every step.
        if std::env::var_os("MODEL_PEEK").is_some() {
            let seen = dump(&mut fs).unwrap();
            assert!(seen == tree, "seed {seed} before step {step}: {:#?}", tree_diff(&seen, &tree));
            fs.fsck().unwrap_or_else(|e| panic!("seed {seed} before step {step}: fsck {e:?}"));
        }
        if rng.below(4) == 0 {
            handle_step(&mut fs, &mut rng, &p, &mut tree, &mut held, &format!("seed {seed} step {step}"));
            continue;
        }
        // Now and then empty a directory of files (`rm dir/*`): its later pairs empty and
        // drop, possibly under an open handle.
        if rng.below(40) == 0 {
            let Some(dir) = pick(&mut rng, &tree, |_, v| matches!(v, Node::Dir { .. })).cloned() else { continue };
            let files: Vec<String> = tree
                .iter()
                .filter(|(k, v)| !k.is_empty() && parent(k) == dir && matches!(v, Node::File { .. }))
                .map(|(k, _)| k.clone())
                .collect();
            for f in files {
                let op = Op::Remove(f);
                trace(|| format!("seed {seed} step {step}: {op:?}"));
                match apply_rust(&mut fs, &op) {
                    Ok(()) => {
                        apply_model(&mut tree, &op);
                        follow(&mut held, &op);
                    }
                    Err(Error::NoSpace) => {}
                    Err(e) => panic!("seed {seed} step {step} {op:?}: {e:?}"),
                }
            }
            // Then new files take the freed blocks, and the handles held open write and
            // close: a handle left pointing at a dropped pair would now write into them.
            for _ in 0..6 {
                let Some(path) = fresh(&mut rng, &tree, &p) else { continue };
                let op = Op::Write { path: path.clone(), data: rng.bytes(300) };
                match apply_rust(&mut fs, &op) {
                    Ok(()) => apply_model(&mut tree, &op),
                    Err(Error::NoSpace) if fs.stat(&path).is_ok() => {
                        tree.insert(path, Node::File { data: Vec::new(), attrs: Default::default() });
                    }
                    Err(Error::NoSpace) => {}
                    Err(e) => panic!("seed {seed} step {step} {op:?}: {e:?}"),
                }
            }
            while !held.is_empty() {
                handle_step(&mut fs, &mut rng, &p, &mut tree, &mut held, &format!("seed {seed} step {step}"));
            }
            continue;
        }
        let path = random_path(&mut rng, &tree, &p);
        if path.is_empty() {
            continue;
        }
        let op = random_op(&mut rng, &tree, path, &p);
        let want = expect(&tree, &op);
        trace(|| format!("seed {seed} step {step}: {:.120} -> {want:?}", format!("{op:?}")));
        let got = apply_rust(&mut fs, &op);
        if (&want, &got) == (&Ok(()), &Err(Error::NoSpace)) {
            // A full volume: the operation changed nothing, except that writing a new file
            // creates it (one commit) before its data fails to fit.
            if let Op::Write { path, .. } = &op {
                if !tree.contains_key(path) && fs.stat(path).is_ok() {
                    tree.insert(path.clone(), Node::File { data: Vec::new(), attrs: Default::default() });
                }
            }
            continue;
        }
        assert_eq!(want, got, "seed {seed} step {step} {op:?}");
        if want.is_ok() {
            apply_model(&mut tree, &op);
            follow(&mut held, &op);
        }

        if step % 50 == 49 {
            for h in held.drain(..) {
                close_held(&mut fs, &mut tree, h, &format!("seed {seed} step {step}"));
            }
            drop(fs);
            fs = Filesystem::mount(&mut ram, cfg).unwrap();
            let seen = dump(&mut fs).unwrap();
            assert!(seen == tree, "seed {seed} after step {step}: {:#?}", tree_diff(&seen, &tree));
            fs.fsck().unwrap();
        }
    }
    for h in held.drain(..) {
        close_held(&mut fs, &mut tree, h, &format!("seed {seed} at the end"));
    }
    let t = dump(&mut fs).unwrap();
    assert_eq!(t, tree);
    fs.fsck().unwrap();
    eprintln!("seed {seed}: {} paths at the end", t.len());
}

#[test]
fn random_operations_small_blocks() {
    for seed in 1..=6 {
        let cfg = Config { block_size: 256, block_count: 4096, prog_size: 16 };
        run(cfg, Profile::default_for(cfg), seed, 1500);
    }
}

#[test]
fn random_operations_large_blocks() {
    for seed in 10..=13 {
        let cfg = Config { block_size: 4096, block_count: 128, prog_size: 512 };
        run(cfg, Profile::default_for(cfg), seed, 1000);
    }
}

#[test]
fn random_operations_tiny_blocks() {
    for seed in 20..=23 {
        let cfg = Config { block_size: 128, block_count: 16384, prog_size: 1 };
        run(cfg, Profile::default_for(cfg), seed, 1000);
    }
}

/// Handles held open while crowded directories split, empty and drop, on a volume small
/// enough that freed blocks are soon reused and often full. With the fix for the red team's
/// stale-handle corruption reverted (a handle left on a dropped pair), seed 36 fails.
#[test]
fn random_operations_crowded_small_volume() {
    let seeds: u64 = std::env::var("MODEL_SEEDS").map_or(40, |s| s.parse().unwrap());
    for seed in 30..30 + seeds {
        run(Config { block_size: 256, block_count: 40, prog_size: 16 }, Profile::crowded(), seed, 1500);
    }
}

/// Many entries in one directory: the metadata splits over several pairs, and removing them
/// all drops the extra pairs again.
#[test]
fn directory_split_and_drop() {
    let cfg = Config { block_size: 256, block_count: 256, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    fs.mkdir("/d").unwrap();
    for i in 0..120 {
        write_file(&mut fs, &format!("/d/file{i:03}"), format!("contents {i}").as_bytes()).unwrap();
    }
    let mut n = 0;
    fs.read_dir("/d", |_| n += 1).unwrap();
    assert_eq!(n, 120);
    fs.fsck().unwrap();
    for i in 0..120 {
        assert_eq!(read_file(&mut fs, &format!("/d/file{i:03}")).unwrap(), format!("contents {i}").as_bytes());
    }
    for i in 0..120 {
        fs.remove(&format!("/d/file{i:03}")).unwrap();
    }
    fs.remove("/d").unwrap();
    fs.fsck().unwrap();
    drop(fs);
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    let mut n = 0;
    fs.read_dir("/", |_| n += 1).unwrap();
    assert_eq!(n, 0);
    fs.fsck().unwrap();
}

/// Filling the volume reports NoSpace and leaves it usable.
#[test]
fn full_volume() {
    let cfg = Config { block_size: 256, block_count: 64, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    write_file(&mut fs, "/keep", b"survives").unwrap();
    let h = fs.open("/big", OpenOptions { write: true, create: true, ..Default::default() }).unwrap();
    let chunk = vec![7u8; 1000];
    let err = loop {
        if let Err(e) = fs.write(h, &chunk) {
            break e;
        }
    };
    assert_eq!(err, Error::NoSpace);
    fs.close(h).unwrap_or(());
    assert_eq!(read_file(&mut fs, "/keep").unwrap(), b"survives");
    fs.remove("/big").unwrap();
    write_file(&mut fs, "/after", &vec![1u8; 3000]).unwrap();
    fs.fsck().unwrap();
}

/// Open handles follow renames and survive removal.
#[test]
fn handles_follow_renames() {
    let cfg = Config { block_size: 256, block_count: 128, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    fs.mkdir("/x").unwrap();
    let h = fs.open("/f", OpenOptions { read: true, write: true, create: true, ..Default::default() }).unwrap();
    fs.write(h, b"hello").unwrap();
    fs.rename("/f", "/x/g").unwrap();
    fs.write(h, b" world").unwrap();
    fs.close(h).unwrap();
    assert_eq!(read_file(&mut fs, "/x/g").unwrap(), b"hello world");
    assert_eq!(fs.stat("/f"), Err(Error::NoEntry));

    let h = fs.open("/x/g", OpenOptions { read: true, ..Default::default() }).unwrap();
    fs.remove("/x/g").unwrap();
    let mut buf = [0u8; 32];
    let n = fs.read(h, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello world");
    fs.close(h).unwrap();
    fs.fsck().unwrap();
}

#[test]
fn bad_arguments() {
    let cfg = Config { block_size: 256, block_count: 32, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    assert_eq!(fs.mkdir("/a/.."), Err(Error::Invalid));
    assert_eq!(fs.mkdir("/"), Err(Error::Exists));
    assert_eq!(fs.remove("/"), Err(Error::Invalid));
    assert_eq!(fs.mkdir(&"n".repeat(256)), Err(Error::NameTooLong));
    fs.mkdir("/d").unwrap();
    assert_eq!(fs.rename("/d", "/d/e"), Err(Error::Invalid));
    assert_eq!(fs.open("/d", OpenOptions { read: true, ..Default::default() }), Err(Error::IsDir));
    assert_eq!(fs.set_attr("/d", 1, &[0; 1023]), Err(Error::NoSpace));
    assert_eq!(fs.get_attr("/d", 1), Err(Error::NoAttr));
    assert_eq!(fs.open("/nope", OpenOptions { read: true, create: true, ..Default::default() }), Err(Error::Invalid));
}
