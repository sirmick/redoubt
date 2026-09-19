//! Model-based tests: random operations, valid or not, against the filesystem and against
//! an in-memory model of what it should hold; after every operation the outcomes must
//! agree, and at intervals the volume is remounted, dumped, compared and checked.

mod common;

use common::ops::*;
use common::*;
use littlefs::{Config, Error, Filesystem, OpenOptions};

/// Mostly paths that exist or whose parent exists, so most operations get somewhere; now
/// and then a random one, so errors are exercised too. Empty: nothing to pick from.
fn random_path(rng: &mut Rng, tree: &Tree, names: usize) -> String {
    let name = |rng: &mut Rng| NAMES[rng.below(names as u64) as usize];
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
fn random_op(rng: &mut Rng, tree: &Tree, path: String, names: usize, max_attr: u64) -> Op {
    let size = |rng: &mut Rng| SIZES[rng.below(SIZES.len() as u64) as usize];
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
            let at = rng.below(old + 300) as u32;
            let n = size(rng);
            let data = rng.bytes(n);
            let cut = (rng.below(3) == 0).then(|| rng.below(old + 2000) as u32);
            Op::Patch { path, at, data, cut }
        }
        3 => Op::Mkdir(path),
        4 | 5 => Op::Remove(path),
        6 | 7 => {
            let to = random_path(rng, tree, names);
            Op::Rename(path, if to.is_empty() { "/x".to_string() } else { to })
        }
        _ => {
            let t = ATTR_TYPES[rng.below(ATTR_TYPES.len() as u64) as usize];
            let target = if rng.below(8) == 0 { String::new() } else { path };
            if rng.below(3) == 0 {
                Op::RemoveAttr(target, t)
            } else {
                // A long name, inline data and three attributes must fit one metadata block.
                let n = rng.below(max_attr) as usize;
                Op::SetAttr(target, t, rng.bytes(n))
            }
        }
    }
}

fn run(cfg: Config, seed: u64, steps: usize) {
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    let mut tree = dump(&mut fs).unwrap();
    let mut rng = Rng(seed);
    // With 128-byte blocks one entry with the long name and three attributes cannot fit a
    // metadata block at all; the reference fails the same way.
    let names = if cfg.block_size < 256 { NAMES.len() - 1 } else { NAMES.len() };

    for step in 0..steps {
        let path = random_path(&mut rng, &tree, names);
        if path.is_empty() {
            continue;
        }
        let op = random_op(&mut rng, &tree, path, names, (cfg.block_size / 16) as u64);
        let want = expect(&tree, &op);
        let got = apply_rust(&mut fs, &op);
        match (&want, &got) {
            (Ok(()), Err(Error::NoSpace)) => panic!("seed {seed} step {step}: volume full; shrink the test"),
            _ => assert_eq!(want, got, "seed {seed} step {step} {op:?}"),
        }
        if want.is_ok() {
            apply_model(&mut tree, &op);
        }

        if step % 50 == 49 {
            drop(fs);
            fs = Filesystem::mount(&mut ram, cfg).unwrap();
            assert_eq!(dump(&mut fs).unwrap(), tree, "seed {seed} after step {step}");
            fs.fsck().unwrap();
        }
    }
    let t = dump(&mut fs).unwrap();
    assert_eq!(t, tree);
    fs.fsck().unwrap();
    eprintln!("seed {seed}: {} paths at the end", t.len());
}

#[test]
fn random_operations_small_blocks() {
    for seed in 1..=6 {
        run(Config { block_size: 256, block_count: 4096, prog_size: 16 }, seed, 1500);
    }
}

#[test]
fn random_operations_large_blocks() {
    for seed in 10..=13 {
        run(Config { block_size: 4096, block_count: 128, prog_size: 512 }, seed, 1000);
    }
}

#[test]
fn random_operations_tiny_blocks() {
    for seed in 20..=23 {
        run(Config { block_size: 128, block_count: 16384, prog_size: 1 }, seed, 1000);
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
