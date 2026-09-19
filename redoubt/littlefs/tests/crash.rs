//! Crash injection: power fails at every single block write of a workload (the failing
//! program persists only half its bytes, nothing after it persists). After each crash the
//! image must mount, show exactly the state before or after the interrupted operation, pass
//! the volume check after the repairs a write triggers, and keep working.

mod common;

use common::*;
use littlefs::{BlockDevice, Config, Error, Filesystem, OpenOptions, SeekFrom};

type Op = Box<dyn Fn(&mut Filesystem<&mut Ram>) -> Result<(), Error>>;

fn patch(path: &'static str, at: u32, data: Vec<u8>) -> Op {
    Box::new(move |fs| {
        let h = fs.open(path, OpenOptions { write: true, ..Default::default() })?;
        fs.seek(h, SeekFrom::Start(at))?;
        fs.write(h, &data)?;
        fs.close(h)
    })
}

fn truncate(path: &'static str, size: u32) -> Op {
    Box::new(move |fs| {
        let h = fs.open(path, OpenOptions { write: true, ..Default::default() })?;
        fs.truncate(h, size)?;
        fs.close(h)
    })
}

/// Each operation must be one atomic filesystem call as far as the disk goes: creating a file
/// commits its (empty) entry on open, so creating and filling are two operations.
fn create(path: String) -> Op {
    Box::new(move |fs| {
        let h = fs.open(&path, OpenOptions { write: true, create_new: true, ..Default::default() })?;
        fs.close(h)
    })
}

fn fill(path: String, data: Vec<u8>) -> Op {
    Box::new(move |fs| {
        let h = fs.open(&path, OpenOptions { write: true, truncate: true, ..Default::default() })?;
        fs.write(h, &data)?;
        fs.close(h)
    })
}

fn workload() -> Vec<Op> {
    let mut rng = Rng(7);
    let mut ops: Vec<Op> = vec![
        create("/a".into()),
        fill("/a".into(), rng.bytes(20)),
        create("/big".into()),
        fill("/big".into(), rng.bytes(3000)),
        Box::new(|fs| fs.mkdir("/d")),
        Box::new(|fs| fs.mkdir("/d/e")),
        create("/d/f".into()),
        fill("/d/f".into(), rng.bytes(500)),
        Box::new(|fs| fs.rename("/big", "/d/big")),
        Box::new(|fs| fs.set_attr("/d/f", 1, b"mtime")),
        Box::new(|fs| fs.set_attr("/", 2, b"root attribute")),
        Box::new(|fs| fs.rename("/d/f", "/g")),
        patch("/d/big", 1000, rng.bytes(700)),
        patch("/a", 20, rng.bytes(40)),
        truncate("/d/big", 100),
        truncate("/a", 5),
        Box::new(|fs| fs.remove("/d/e")),
    ];
    // Enough entries to split /d over several pairs, then remove them to drop the pairs.
    for i in 0..24 {
        ops.push(create(format!("/d/n{i:02}")));
        ops.push(fill(format!("/d/n{i:02}"), rng.bytes(i * 37 % 300)));
    }
    // A directory created in the first pair of a split directory is linked into the list of
    // pairs and named in two commits (an orphan in between).
    ops.push(Box::new(|fs| fs.mkdir("/d/m")));
    ops.push(Box::new(|fs| fs.mkdir("/d/zz")));
    for i in 0..24 {
        ops.push(Box::new(move |fs| fs.remove(&format!("/d/n{i:02}"))));
    }
    ops.push(Box::new(|fs| fs.remove("/d/m")));
    ops.extend::<Vec<Op>>(vec![
        Box::new(|fs| fs.rename("/d", "/d2")),
        Box::new(|fs| fs.rename("/a", "/g")),
        Box::new(|fs| fs.mkdir("/y")),
        Box::new(|fs| fs.mkdir("/z")),
        Box::new(|fs| fs.rename("/y", "/z")),
        Box::new(|fs| fs.remove_attr("/", 2)),
        Box::new(|fs| fs.remove("/d2/big")),
    ]);
    ops
}

/// A random workload from the shared generator. Writing a new file is split into creating
/// it and filling it, the two commits it takes.
fn random_workload(seed: u64, cfg: Config, len: usize) -> Vec<Op> {
    use common::ops::{self, generate, Op as Gen};
    let names = if cfg.block_size < 256 { ops::NAMES.len() - 1 } else { ops::NAMES.len() };
    let mut rng = Rng(seed);
    let mut model = Tree::new();
    model.insert(String::new(), Node::Dir { attrs: Default::default() });
    let mut out: Vec<Op> = Vec::new();
    while out.len() < len {
        let Some(op) = generate(&mut rng, &model, names, (cfg.block_size / 16) as u64) else { continue };
        if let Gen::Write { path, .. } = &op {
            if !model.contains_key(path) {
                out.push(create(path.clone()));
            }
        }
        ops::apply_model(&mut model, &op);
        out.push(Box::new(move |fs| ops::apply_rust(fs, &op)));
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
            op(&mut fs).unwrap();
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
                if op(&mut fs).is_err() {
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
