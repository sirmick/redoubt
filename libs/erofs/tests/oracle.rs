//! erofs-utils 1.9 as the oracle (docs/servers/erofsd.md, "The packer"): what `mkfs.erofs` packs
//! from a tree, in the options that keep it in the subset, our parser reads as that tree; and what
//! our writer packs, `fsck.erofs` checks and extracts as that tree, with its attributes.
//!
//! The options, the ones that keep `mkfs.erofs` 1.9 in the subset: no `-z` (no compression), no
//! `--chunksize` (no chunks), and no `-E` feature that is off by default (`fragments`, `dedupe`,
//! `48bit`, `dot-omitted`); `-b4096`, `-T` 0 and `--all-root` for 4 KiB blocks, a fixed timestamp and
//! owner. Each variant adds one of `-Eforce-inode-compact`, `-Eforce-inode-extended`,
//! `-E^inline_data` (flat plain only) and `--root-xattr-isize=64` (attributes to skip).
//!
//! Needs `mkfs.erofs`, `fsck.erofs` and `dump.erofs` on the path: the bench's `erofs-oracle`
//! case reports a named skip without them, and these tests fail. Built only with the `oracle`
//! feature, which that case sets, so `erofs-host-tests` runs without erofs-utils.
#![cfg(feature = "oracle")]

use std::path::{Path, PathBuf};
use std::process::Command;

use erofs::{Entry, Tree, read_tree};
use sha2::{Digest, Sha256};

fn sha256(data: &[u8]) -> [u8; 32] { Sha256::digest(data).into() }

/// The options every `mkfs.erofs` run here takes.
const MKFS: &[&str] = &["-d0", "-b4096", "-T0", "--all-root"];
/// The variants, each one more option.
const VARIANTS: &[&[&str]] = &[
    &[],
    &["-Eforce-inode-compact"],
    &["-Eforce-inode-extended"],
    &["-E^inline_data"],
    &["--root-xattr-isize=64"],
];

/// xorshift64: the same tree for the same seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

/// A random tree, parents first: names that sort around `.` and `..`, some long enough to spread
/// a directory over several blocks, and sizes at and around the block's edges.
fn random_tree(seed: u64) -> Tree {
    const ALPHABET: &[u8] = b"+-.0189AZaz_~";
    fn fill(rng: &mut Rng, dir: &str, depth: u32, out: &mut Tree) {
        let count = if depth == 0 { 120 + rng.below(80) } else { rng.below(20) };
        let mut names: Vec<String> = Vec::new();
        while (names.len() as u64) < count {
            let len = if rng.below(8) == 0 { 200 + rng.below(56) } else { 1 + rng.below(12) };
            let name: String =
                (0..len).map(|_| ALPHABET[rng.below(ALPHABET.len() as u64) as usize] as char).collect();
            if name != "." && name != ".." && !names.contains(&name) {
                names.push(name);
            }
        }
        for name in names {
            let path = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if depth < 3 && rng.below(6) == 0 {
                out.push((path.clone(), None));
                fill(rng, &path, depth + 1, out);
            } else {
                let size = [0, 1, 4095, 4096, 4097, 8192, 4096 - 120, 4096 - 64][rng.below(8) as usize];
                let size = if rng.below(3) == 0 { rng.below(20_000) as usize } else { size };
                out.push((path, Some((0..size).map(|_| rng.next() as u8).collect())));
            }
        }
    }
    let mut out = Vec::new();
    fill(&mut Rng(0x9e37_79b9_7f4a_7c15 ^ seed), "", 0, &mut out);
    out
}

/// A fresh directory of this test's own under the target directory.
fn scratch(name: &str) -> PathBuf {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("erofs-oracle-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stage(tree: &Tree, dir: &Path) {
    for (path, data) in tree {
        match data {
            Some(data) => std::fs::write(dir.join(path), data).unwrap(),
            None => std::fs::create_dir(dir.join(path)).unwrap(),
        }
    }
}

/// The tree under `dir`, as the parser lists it.
fn staged(dir: &Path) -> Tree {
    let mut out = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        for entry in std::fs::read_dir(dir.join(&rel)).unwrap() {
            let entry = entry.unwrap();
            let path = rel.join(entry.file_name());
            let name = path.to_str().unwrap().to_string();
            if entry.file_type().unwrap().is_dir() {
                out.push((name, None));
                todo.push(path);
            } else {
                out.push((name, Some(std::fs::read(entry.path()).unwrap())));
            }
        }
    }
    out.sort();
    out
}

fn run(program: &str, args: &[&str]) -> String {
    let out = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{program} (erofs-utils 1.9) could not run: {e}"));
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{program} {args:?}: {}\n{text}", out.status);
    text
}

fn sorted(mut tree: Tree) -> Tree {
    tree.sort();
    tree
}

#[test]
fn what_mkfs_erofs_packs_our_parser_reads_as_the_tree() {
    for seed in 1..=3 {
        let tree = random_tree(seed);
        let dir = scratch(&format!("mkfs-{seed}"));
        let source = dir.join("tree");
        std::fs::create_dir(&source).unwrap();
        stage(&tree, &source);
        for (v, variant) in VARIANTS.iter().enumerate() {
            let image = dir.join(format!("{v}.erofs"));
            let mut args: Vec<&str> = MKFS.to_vec();
            args.extend_from_slice(variant);
            args.extend([image.to_str().unwrap(), source.to_str().unwrap()]);
            run("mkfs.erofs", &args);
            let bytes = std::fs::read(&image).unwrap();
            let read = read_tree(&bytes).unwrap_or_else(|_| panic!("seed {seed} {variant:?}: corrupt"));
            assert_eq!(sorted(read), sorted(tree.clone()), "seed {seed} {variant:?}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn what_our_writer_packs_fsck_erofs_checks_and_extracts_as_the_tree() {
    for seed in 1..=3 {
        let tree = random_tree(seed);
        let entries: Vec<Entry<'_>> = tree
            .iter()
            .map(|(path, data)| match data {
                Some(data) => Entry::File(path, data),
                None => Entry::Dir(path),
            })
            .collect();
        let dir = scratch(&format!("fsck-{seed}"));
        let image = dir.join("ours.erofs");
        std::fs::write(&image, erofs::pack(&entries, sha256).unwrap()).unwrap();
        let image = image.to_str().unwrap();
        run("fsck.erofs", &["-d0", "--xattrs", image]);
        let out = dir.join("out");
        run("fsck.erofs", &["-d0", &format!("--extract={}", out.display()), image]);
        assert_eq!(staged(&out), sorted(tree.clone()), "seed {seed}");
        // Each file carries its attribute area: `user.sha256`, 56 bytes.
        let file = tree.iter().find(|(_, data)| data.is_some()).unwrap();
        let shown = run("dump.erofs", &[&format!("--path=/{}", file.0), image]);
        assert!(shown.contains("Xattr size: 56"), "seed {seed}: {shown}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
