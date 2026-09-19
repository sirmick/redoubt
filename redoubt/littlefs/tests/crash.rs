//! Crash injection: power fails at every single block write of a workload (the failing
//! program persists only half its bytes, nothing after it persists). After each crash the
//! image must mount, show exactly the state before or after the interrupted operation, pass
//! the volume check after the repairs a write triggers, and keep working.

mod common;

use common::ops::*;
use common::*;
use littlefs::{BlockDevice, Config, Error, Filesystem};

/// Each operation must be one atomic filesystem call as far as the disk goes. Creating a
/// file commits its (empty) entry on open, so a new file is written as two operations: an
/// empty write (the creation) and the write proper.
fn create(path: &str) -> Op { Op::Write { path: path.into(), data: Vec::new() } }

fn write(path: &str, data: Vec<u8>) -> Op { Op::Write { path: path.into(), data } }

fn patch(path: &str, at: u32, data: Vec<u8>) -> Op { Op::Patch { path: path.into(), at, data, cut: None } }

fn truncate(path: &str, size: u32) -> Op { Op::Patch { path: path.into(), at: 0, data: Vec::new(), cut: Some(size) } }

fn mkdir(path: &str) -> Op { Op::Mkdir(path.into()) }

fn remove(path: &str) -> Op { Op::Remove(path.into()) }

fn rename(from: &str, to: &str) -> Op { Op::Rename(from.into(), to.into()) }

fn workload() -> Vec<Op> {
    let mut rng = Rng(7);
    let mut ops = vec![
        create("/a"),
        write("/a", rng.bytes(20)),
        create("/big"),
        write("/big", rng.bytes(3000)),
        mkdir("/d"),
        mkdir("/d/e"),
        create("/d/f"),
        write("/d/f", rng.bytes(500)),
        rename("/big", "/d/big"),
        Op::SetAttr("/d/f".into(), 1, b"mtime".to_vec()),
        Op::SetAttr(String::new(), 2, b"root attribute".to_vec()),
        rename("/d/f", "/g"),
        patch("/d/big", 1000, rng.bytes(700)),
        patch("/a", 20, rng.bytes(40)),
        truncate("/d/big", 100),
        truncate("/a", 5),
        remove("/d/e"),
    ];
    // Enough entries to split /d over several pairs, then remove them to drop the pairs.
    for i in 0..24 {
        ops.push(create(&format!("/d/n{i:02}")));
        ops.push(write(&format!("/d/n{i:02}"), rng.bytes(i * 37 % 300)));
    }
    // A directory created in the first pair of a split directory is linked into the list of
    // pairs and named in two commits (an orphan in between).
    ops.push(mkdir("/d/m"));
    ops.push(mkdir("/d/zz"));
    ops.extend((0..24).map(|i| remove(&format!("/d/n{i:02}"))));
    ops.extend([
        remove("/d/m"),
        rename("/d", "/d2"),
        rename("/a", "/g"),
        mkdir("/y"),
        mkdir("/z"),
        rename("/y", "/z"),
        Op::RemoveAttr(String::new(), 2),
        remove("/d2/big"),
    ]);
    ops
}

/// A random workload from the shared generator, new files created first (see `create`).
fn random_workload(seed: u64, cfg: Config, len: usize) -> Vec<Op> {
    let names = if cfg.block_size < 256 { NAMES.len() - 1 } else { NAMES.len() };
    let mut rng = Rng(seed);
    let mut model = Tree::new();
    model.insert(String::new(), Node::Dir { attrs: Default::default() });
    let mut out = Vec::new();
    while out.len() < len {
        let Some(op) = generate(&mut rng, &model, names, (cfg.block_size / 16) as u64) else { continue };
        if let Op::Write { path, .. } = &op {
            if !model.contains_key(path) {
                out.push(create(path));
            }
        }
        apply_model(&mut model, &op);
        out.push(op);
    }
    out
}

fn crash_everywhere(cfg: Config) { crash_workload(cfg, workload()) }

fn crash_workload(cfg: Config, ops: Vec<Op>) {
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let formatted = ram.data.clone();

    // The run without faults: the state after each operation, and how many writes it takes.
    let mut states = Vec::new();
    ram.writes = 0;
    {
        let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
        states.push(dump(&mut fs).unwrap());
        for op in &ops {
            apply_rust(&mut fs, op).unwrap();
            states.push(dump(&mut fs).unwrap());
        }
        fs.fsck().unwrap();
    }
    let total = ram.writes;
    assert!(total > 50);

    for n in 1..=total {
        let mut ram = Ram::from_image(cfg, formatted.clone());
        ram.budget = Some(n);
        let mut failed_at = None;
        {
            let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
            for (k, op) in ops.iter().enumerate() {
                if apply_rust(&mut fs, op).is_err() {
                    failed_at = Some(k);
                    break;
                }
            }
        }
        let k = failed_at.unwrap_or_else(|| panic!("write {n} of {total} failed but no operation did"));

        let mut ram = Ram::from_image(cfg, ram.data);
        let mut fs = Filesystem::mount(&mut ram, cfg).unwrap_or_else(|e| panic!("crash at write {n}: mount: {e}"));
        let seen = dump(&mut fs).unwrap_or_else(|e| panic!("crash at write {n}: dump: {e}"));
        assert!(
            seen == states[k] || seen == states[k + 1],
            "crash at write {n} (operation {k}): neither the state before nor after\n{seen:#?}"
        );
        fs.fsck().unwrap_or_else(|e| panic!("crash at write {n} (operation {k}): fsck: {e}"));
        assert_eq!(dump(&mut fs).unwrap(), seen, "crash at write {n}: repair changed what is visible");
        // Still fully usable.
        write_file(&mut fs, "/after-crash", b"still works").unwrap();
        assert_eq!(read_file(&mut fs, "/after-crash").unwrap(), b"still works");
        fs.fsck().unwrap();
    }
    eprintln!("{cfg:?}: crashed at each of {total} writes");
}

#[test]
fn crash_at_every_write_small_blocks() { crash_everywhere(Config { block_size: 256, block_count: 128, prog_size: 16 }); }

#[test]
fn crash_at_every_write_tiny_blocks() { crash_everywhere(Config { block_size: 128, block_count: 256, prog_size: 1 }); }

#[test]
fn crash_at_every_write_large_blocks() {
    crash_everywhere(Config { block_size: 4096, block_count: 32, prog_size: 512 });
}

#[test]
fn crash_at_every_write_random_workloads() {
    for seed in 1..=6 {
        let cfg = Config { block_size: 256, block_count: 512, prog_size: 16 };
        crash_workload(cfg, random_workload(seed, cfg, 80));
    }
    for seed in 7..=9 {
        let cfg = Config { block_size: 128, block_count: 1024, prog_size: 1 };
        crash_workload(cfg, random_workload(seed, cfg, 60));
    }
}

/// A device that fails an operation without losing power (an I/O error) poisons the
/// filesystem rather than letting memory and disk drift apart.
#[test]
fn io_error_poisons_until_remount() {
    let cfg = Config { block_size: 256, block_count: 64, prog_size: 16 };
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    ram.writes = 0;
    ram.budget = Some(1);
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    assert_eq!(fs.mkdir("/a"), Err(Error::Io));
    assert_eq!(fs.mkdir("/b"), Err(Error::Poisoned));
    assert_eq!(fs.stat("/"), Err(Error::Poisoned));
    let ram = fs.unmount();
    ram.budget = None;
    let mut fs = Filesystem::mount(ram, cfg).unwrap();
    fs.mkdir("/b").unwrap();
    fs.fsck().unwrap();
    let _ = BlockDevice::sync(fs.unmount());
}
