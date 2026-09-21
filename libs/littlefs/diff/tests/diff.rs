//! Differential tests against the C reference: random operations are applied by one
//! implementation, or by both taking turns on the same image, and after each step both must
//! read back exactly what an in-memory model says the volume holds.
//!
//! Most runs use the reference without wear levelling, as this crate has none. The
//! wear-levelling runs turn it on (a relocation every 3 erases), so the reference relocates
//! metadata and grows the superblock chain, which this crate never does but must read.

#[path = "../../tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use common::ops::*;
use common::*;
use littlefs::{Config, Filesystem, Health};
use littlefs_diff::{CConfig, CFs, LFS_ERR_NOATTR};

fn apply_c(fs: &mut CFs, op: &Op) {
    let r = match op {
        Op::Write { path, data } => fs.write(path, true, true, 0, data, None),
        Op::Patch { path, at, data, cut } => fs.write(path, false, false, *at, data, *cut),
        Op::Mkdir(p) => fs.mkdir(p),
        Op::Remove(p) => fs.remove(p),
        Op::Rename(a, b) => fs.rename(a, b),
        Op::SetAttr(p, t, v) => fs.set_attr(slash(p), *t, v),
        Op::RemoveAttr(p, t) => fs.remove_attr(slash(p), *t),
    };
    r.unwrap_or_else(|e| panic!("C {op:?}: {e}"));
}

fn apply_rust_ok(fs: &mut Filesystem<&mut Ram>, op: &Op) {
    apply_rust(fs, op).unwrap_or_else(|e| panic!("Rust {op:?}: {e:?}"));
}

/// Everything the C reference reads from a volume, in the same shape as `common::dump`.
fn dump_c(fs: &mut CFs) -> Tree {
    let attrs = |fs: &mut CFs, p: &str| {
        let mut m = BTreeMap::new();
        for t in ATTR_TYPES {
            match fs.get_attr(slash(p), t) {
                Ok(v) => {
                    m.insert(t, v);
                }
                Err(LFS_ERR_NOATTR) => {}
                Err(e) => panic!("C getattr {p}: {e}"),
            }
        }
        m
    };
    let mut tree = Tree::new();
    let root = attrs(fs, "");
    tree.insert(String::new(), Node::Dir { attrs: root });
    let mut todo = vec![String::new()];
    while let Some(dir) = todo.pop() {
        for (is_dir, size, name) in fs.list(slash(&dir)).expect("C list") {
            let path = format!("{dir}/{name}");
            let node = if is_dir {
                todo.push(path.clone());
                Node::Dir { attrs: attrs(fs, &path) }
            } else {
                let data = fs.read(&path).expect("C read");
                assert_eq!(data.len() as u32, size);
                Node::File { data, attrs: attrs(fs, &path) }
            };
            tree.insert(path, node);
        }
    }
    tree
}

fn rust_cfg(c: CConfig) -> Config { Config { block_size: c.block_size, block_count: c.block_count, prog_size: c.prog_size } }

/// Reads the image with both implementations and compares with the model, then runs the
/// read-only volume check. An image Rust wrote last must be clean. One the reference wrote
/// last may carry leftovers Rust repairs on its first write (the reference's relocations
/// can leave half-orphaned directories with the orphan flag clear); those are reported.
fn check_both(cfg: CConfig, image: &[u8], model: &Tree, last: Who, strict: bool, what: &str) {
    let mut ram = Ram::from_image(rust_cfg(cfg), image.to_vec());
    let mut fs = Filesystem::mount(&mut ram, rust_cfg(cfg)).unwrap_or_else(|e| panic!("{what}: Rust mount: {e:?}"));
    let seen = dump(&mut fs).unwrap();
    assert!(&seen == model, "{what}: Rust reads: {:#?}", tree_diff(&seen, model));
    match fs.check() {
        Ok(Health::Clean) => {}
        Ok(Health::NeedsRepair) if last == Who::C && !strict => eprintln!("{what}: the reference left leftovers to repair"),
        other => panic!("{what}: Rust check after {} wrote: {other:?}", if last == Who::C { "C" } else { "Rust" }),
    }
    drop(fs);
    assert_eq!(ram.data, image, "{what}: the check wrote");
    let mut c = CFs::mount(cfg, image.to_vec()).unwrap_or_else(|e| panic!("{what}: C mount: {e}"));
    let seen = dump_c(&mut c);
    assert!(&seen == model, "{what}: C reads: {:#?}", tree_diff(&seen, model));
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Who {
    Rust,
    C,
    Both,
}

/// Runs `steps` random operations, each done by Rust or C as `who` says (alternating
/// randomly for `Both`), checking both readers against the model every `every` steps.
fn run(cfg: CConfig, who: Who, seed: u64, steps: usize, every: usize) { run_checked(cfg, who, seed, steps, every, false) }

/// `run`, where `strict` also refuses leftovers from the reference.
fn run_checked(cfg: CConfig, who: Who, seed: u64, steps: usize, every: usize, strict: bool) {
    let p = Profile::default_for(rust_cfg(cfg));
    let mut rng = Rng(seed);
    let mut image = if who == Who::C {
        CFs::format(cfg).into_image()
    } else {
        let mut ram = Ram::new(rust_cfg(cfg));
        Filesystem::format(&mut ram, rust_cfg(cfg)).unwrap();
        ram.data
    };
    let mut model = Tree::new();
    model.insert(String::new(), Node::Dir { attrs: BTreeMap::new() });
    check_both(cfg, &image, &model, if who == Who::C { Who::C } else { Who::Rust }, strict, "fresh");

    let mut step = 0;
    while step < steps {
        // A batch of operations by one side, then a check.
        let side = match who {
            Who::Both if rng.below(2) == 0 => Who::Rust,
            Who::Both => Who::C,
            w => w,
        };
        // Diagnosis: DIFF_ONLY=c (or rust) replays the same operations with one side doing
        // every step, to tell the reference's own failures from interoperability ones.
        let side = match std::env::var("DIFF_ONLY").as_deref() {
            Ok("c") => Who::C,
            Ok("rust") => Who::Rust,
            _ => side,
        };
        let mut ops = Vec::new();
        while ops.len() < every {
            if let Some(op) = generate(&mut rng, &model, &p) {
                apply_model(&mut model, &op);
                ops.push(op);
            }
        }
        if side == Who::Rust {
            let mut ram = Ram::from_image(rust_cfg(cfg), image);
            let mut fs = Filesystem::mount(&mut ram, rust_cfg(cfg)).unwrap();
            for op in &ops {
                apply_rust_ok(&mut fs, op);
            }
            drop(fs);
            image = ram.data;
        } else {
            let mut fs = CFs::mount(cfg, image).unwrap();
            for op in &ops {
                apply_c(&mut fs, op);
            }
            image = fs.into_image();
        }
        step += ops.len();
        check_both(cfg, &image, &model, side, strict, &format!("seed {seed} step {step} (after {side:?})"));
    }
}

const SMALL: CConfig = CConfig { block_size: 256, block_count: 4096, prog_size: 16, block_cycles: -1 };

#[test]
fn rust_writes_c_reads() {
    for seed in 1..=20 {
        run(SMALL, Who::Rust, seed, 400, 20);
    }
}

#[test]
fn c_writes_rust_reads() {
    for seed in 101..=120 {
        run(SMALL, Who::C, seed, 400, 20);
    }
}

#[test]
fn interleaved() {
    for seed in 201..=230 {
        run(SMALL, Who::Both, seed, 600, 10);
    }
}

/// The reference relocating metadata for wear levelling and expanding the superblock chain.
#[test]
fn interleaved_with_c_wear_levelling() {
    // With wear levelling this aggressive (a relocation every 3 erases) the reference fails
    // on some operation sequences. In every case below the image it started from was read
    // identically by both implementations and passed Rust's check; Rust replaying the same
    // operations alone (DIFF_ONLY=rust) passes every seed. The reference replaying them alone
    // (DIFF_ONLY=c) fails on other seeds of this range as well, the same ways (C-alone
    // failures between 301 and 360: asserts at 304, 307, 309, 310, 321, 356, 358, and
    // `preporphans` at 331; a spurious NOENT at 335; CORRUPT on reading at 337; misreads at
    // 350, 352). The list depends on where both sides place blocks, so it changes with
    // either allocator; rerun `WL_SEED=n` per seed to recompute it.
    // Reproduce: `WL_SEED=n cargo test --release interleaved_with` in `diff/` (C reference
    // v2.11.3 in `c/`, 256-byte blocks, 4096 of them, program size 16, block_cycles 3).
    const REFERENCE_FAILS: [(u64, &str); 10] = [
        (302, "C asserts: lfs_dir_relocatingcommit: pdir"),
        (308, "C rename returns NOENT for a source that exists"),
        (309, "C loses an attribute removal while relocating"),
        (325, "C asserts: lfs_dir_relocatingcommit: pdir"),
        (336, "C asserts: lfs_dir_relocatingcommit: pdir"),
        (346, "C asserts: lfs_dir_relocatingcommit: pdir"),
        (352, "C rename returns NOENT for a source that exists"),
        (353, "C asserts: lfs_dir_relocatingcommit: pdir"),
        (355, "C asserts: lfs_dir_relocatingcommit: pdir"),
        (358, "C asserts: lfs_dir_relocatingcommit: pdir"),
    ];
    let seeds: Vec<u64> = match std::env::var("WL_SEED") {
        Ok(s) => vec![s.parse().unwrap()],
        Err(_) => (301..=360).filter(|s| !REFERENCE_FAILS.iter().any(|(f, _)| f == s)).collect(),
    };
    for seed in seeds {
        run(CConfig { block_cycles: 3, ..SMALL }, Who::Both, seed, 600, 10);
    }
}

/// The reference alone, with wear levelling, leaves directories on the list of pairs that no
/// entry names, or names under another pair (orphans and half-orphans), with the orphan
/// flag clear: a relocation inside a removal resets the count. Only blocks leak, and Rust's
/// first write after mount repairs it; this shows it happening (seed 300, at step 130;
/// `LEAK_SEED=n` tries others).
#[test]
#[ignore]
fn reference_leaves_orphans_with_the_flag_clear() {
    let seed = std::env::var("LEAK_SEED").map_or(300, |s| s.parse().unwrap());
    run_checked(CConfig { block_cycles: 3, ..SMALL }, Who::C, seed, 600, 10, true);
}

/// The program size is not on disk: each side may use its own on the same image (forward
/// CRCs record the size they cover).
#[test]
fn program_sizes_differ_between_mounts() {
    let rust = Config { block_size: 512, block_count: 1024, prog_size: 16 };
    let mut rng = Rng(77);
    let mut ram = Ram::new(rust);
    Filesystem::format(&mut ram, rust).unwrap();
    let mut model = Tree::new();
    model.insert(String::new(), Node::Dir { attrs: BTreeMap::new() });
    let mut image = ram.data;
    for round in 0..40 {
        let c_prog = [1, 8, 64, 128][round % 4];
        let rust_prog = [16, 4, 512, 32][round % 4];
        let mut ops = Vec::new();
        while ops.len() < 8 {
            if let Some(op) = generate(&mut rng, &model, &Profile { max_attr: 32, ..Profile::default_for(rust) }) {
                apply_model(&mut model, &op);
                ops.push(op);
            }
        }
        let c = CConfig { block_size: 512, block_count: 1024, prog_size: c_prog, block_cycles: -1 };
        if round % 2 == 0 {
            let cfg = Config { prog_size: rust_prog, ..rust };
            let mut ram = Ram::from_image(cfg, image);
            let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
            ops.iter().for_each(|op| apply_rust_ok(&mut fs, op));
            drop(fs);
            image = ram.data;
        } else {
            let mut fs = CFs::mount(c, image).unwrap();
            ops.iter().for_each(|op| apply_c(&mut fs, op));
            image = fs.into_image();
        }
        check_both(c, &image, &model, if round % 2 == 0 { Who::Rust } else { Who::C }, false, &format!("round {round}"));
    }
}

#[test]
fn geometries() {
    let tiny = CConfig { block_size: 128, block_count: 16384, prog_size: 1, block_cycles: -1 };
    let big = CConfig { block_size: 4096, block_count: 512, prog_size: 512, block_cycles: -1 };
    for (i, cfg) in [tiny, big, CConfig { block_cycles: 5, ..tiny }, CConfig { block_cycles: 7, ..big }].into_iter().enumerate() {
        run(cfg, Who::Both, 50 + i as u64, 400, 10);
    }
}
