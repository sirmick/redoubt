//! Crash injection: power fails at every single block write of a workload. The failing write
//! is torn as the `BlockDevice` contract allows (a random prefix of whole program units, then
//! part of one unit; optionally a torn erase), and nothing after it persists. After each
//! crash the image must mount, show exactly the state before or after the interrupted
//! operation, pass the volume check after the repairs a write triggers, and keep working.
//! Optionally the repair itself is crashed at each of its writes too.

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
    let p = Profile::default_for(cfg);
    let mut rng = Rng(seed);
    let mut model = Tree::new();
    model.insert(String::new(), Node::Dir { attrs: Default::default() });
    let mut out = Vec::new();
    while out.len() < len {
        let Some(op) = generate(&mut rng, &model, &p) else { continue };
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

fn crash_everywhere(cfg: Config) { crash_workload(cfg, workload(), Tear::Prefix, false) }

/// Mounts `image`, checks it shows the state before or after operation `k`, repairs and
/// checks it, and uses it. Returns what it showed.
fn check_crashed(cfg: Config, image: Vec<u8>, states: &[Tree], k: usize, what: &str) -> Tree {
    let mut ram = Ram::from_image(cfg, image);
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap_or_else(|e| panic!("{what}: mount: {e}"));
    let seen = dump(&mut fs).unwrap_or_else(|e| panic!("{what}: dump: {e}"));
    assert!(
        seen == states[k] || seen == states[k + 1],
        "{what} (operation {k}): neither the state before nor after: {:#?}",
        tree_diff(&seen, &states[k])
    );
    fs.fsck().unwrap_or_else(|e| panic!("{what} (operation {k}): fsck: {e}"));
    assert_eq!(dump(&mut fs).unwrap(), seen, "{what}: repair changed what is visible");
    write_file(&mut fs, "/after-crash", b"still works").unwrap();
    assert_eq!(read_file(&mut fs, "/after-crash").unwrap(), b"still works");
    fs.fsck().unwrap();
    seen
}

/// Crashes `ops` at each of its writes, torn as `tear` says; with `double`, also crashes the
/// repair after each crash at each of the repair's writes.
fn crash_workload(cfg: Config, ops: Vec<Op>, tear: Tear, double: bool) {
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

    let mut doubles = 0;
    for n in 1..=total {
        let mut ram = Ram::from_image(cfg, formatted.clone()).failing_at(n, tear, n);
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
        let crashed = ram.data;
        let seen = check_crashed(cfg, crashed.clone(), &states, k, &format!("crash at write {n}"));

        // Power fails again while the next mount repairs (at each of the repair's writes).
        for m in 1.. {
            if !double {
                break;
            }
            let mut ram = Ram::from_image(cfg, crashed.clone()).failing_at(m, tear, n << 20 | m);
            let repaired = Filesystem::mount(&mut ram, cfg).and_then(|mut fs| fs.fsck()).is_ok();
            if repaired {
                break;
            }
            doubles += 1;
            let again = check_crashed(cfg, ram.data, &states, k, &format!("crash at write {n}, then repair write {m}"));
            assert_eq!(again, seen, "crash at write {n}, then repair write {m}: the repair changed what is visible");
            assert!(m < 64, "a repair of more than 64 writes");
        }
    }
    eprintln!("{cfg:?} {tear:?}: crashed at each of {total} writes, {doubles} crashes during repair");
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
        crash_workload(cfg, random_workload(seed, cfg, 80), Tear::Prefix, false);
    }
    for seed in 7..=9 {
        let cfg = Config { block_size: 128, block_count: 1024, prog_size: 1 };
        crash_workload(cfg, random_workload(seed, cfg, 60), Tear::Prefix, false);
    }
}

#[test]
fn crash_at_every_write_torn_erases() {
    for seed in 11..=14 {
        let cfg = Config { block_size: 256, block_count: 256, prog_size: 16 };
        crash_workload(cfg, random_workload(seed, cfg, 60), Tear::PrefixAndErase, false);
    }
}

#[test]
fn crash_during_repair() {
    crash_workload(Config { block_size: 256, block_count: 128, prog_size: 16 }, workload(), Tear::PrefixAndErase, true);
    for seed in 21..=23 {
        let cfg = Config { block_size: 256, block_count: 256, prog_size: 16 };
        crash_workload(cfg, random_workload(seed, cfg, 50), Tear::PrefixAndErase, true);
    }
}

/// Why the `BlockDevice` contract asks for prefix tearing: when a torn program may persist
/// any subset of its units, a unit can land after an unwritten one. The forward CRC only
/// vouches for the first unit past a commit, so the next append programs over the stray
/// unit (the first failure seen: "program over unerased bytes"). Not a property this crate
/// can fix; run with `--ignored` to see it fail.
#[test]
#[ignore]
fn subset_tearing_is_outside_the_contract() {
    for seed in 31..=40 {
        let cfg = Config { block_size: 256, block_count: 256, prog_size: 16 };
        crash_workload(cfg, random_workload(seed, cfg, 50), Tear::Subset, false);
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
