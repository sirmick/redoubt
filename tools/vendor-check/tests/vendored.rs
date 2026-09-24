//! The vendored crates are the published ones, unmodified, and they are the ones that build
//! (vendor/README.md; answer 174).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use redoubt_keyd::sha256;

/// Each vendored crate: its directory under `vendor/`, its version, and the SHA-256 of the
/// `.crate` file crates.io published, as its index records it.
const VENDORED: &[(&str, &str, &str)] = &[
    ("smoltcp", "0.14.0", "b6f8b28ad56c6e35524a37dd492af5d1a47e31e1a4d175cd12f89c075f01980f"),
    ("managed", "0.8.0", "0ca88d725a0a943b096803bd34e73a4437208b6077654cc4ecb2947a5f91618d"),
    ("heapless", "0.9.3", "25ba4bd83f9415b58b4ed8dc5714c76e626a105be4646c02630ad730ad3b5aa4"),
    ("hash32", "0.3.1", "47d60b12902ba28e2730cd37e95b8c9223af2808df9e902d4df49588d1470606"),
    ("stable_deref_trait", "1.2.1", "6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596"),
    ("byteorder", "1.5.0", "1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b"),
];

/// smoltcp's dependencies left to `Cargo.lock` (vendor/README.md says why): already locked
/// from crates.io for other packages, which a patch would move too. Pinned here to the
/// version and checksum they have now.
const LOCKED: &[(&str, &str, &str)] = &[
    ("cfg-if", "1.0.0", "baf1de4339761588bc0619e3cbc0120ee582ebb74b53b4efbf79117bd2da40fd"),
    ("bitflags", "1.3.2", "bef38d45163c2f1dde094a7dfd33ccf595c92905c8f8f4fdc18d06fb1037718a"),
];

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// Every file under `dir`, as paths relative to `base`, with `/` separators.
fn files(base: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(base, &path, out);
        } else {
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.insert(rel);
        }
    }
}

/// `vendor/SHA256SUMS` lists every vendored file with its SHA-256, each file is exactly that,
/// and no file is missing from the list or from the tree. The sums are generated from the tree,
/// so this is integrity since vendoring; `provenance.sh` checks the tree against crates.io.
#[test]
fn vendored_files_are_the_published_bytes() {
    let vendor = root().join("vendor");
    let sums = std::fs::read_to_string(vendor.join("SHA256SUMS")).expect("vendor/SHA256SUMS");
    let mut listed = BTreeSet::new();
    for line in sums.lines() {
        let (sum, path) = line.split_once("  ").unwrap_or_else(|| panic!("malformed line {line:?}"));
        let bytes = std::fs::read(vendor.join(path)).unwrap_or_else(|e| panic!("vendor/{path}: {e}"));
        assert_eq!(hex(&sha256::hash(&bytes)), sum, "vendor/{path} is not the published file");
        assert!(listed.insert(path.to_string()), "vendor/{path} listed twice");
    }
    let mut present = BTreeSet::new();
    for (name, ..) in VENDORED {
        files(&vendor, &vendor.join(name), &mut present);
    }
    let added: Vec<_> = present.difference(&listed).collect();
    let removed: Vec<_> = listed.difference(&present).collect();
    assert!(added.is_empty(), "files not in vendor/SHA256SUMS: {added:?}");
    assert!(removed.is_empty(), "files listed but missing: {removed:?}");
    for (name, ..) in VENDORED {
        assert!(listed.iter().any(|p| p.starts_with(&format!("{name}/"))), "nothing listed for {name}");
    }
}

/// The `[[package]]` blocks of `Cargo.lock` named `name`, each as its lines.
fn locked(lock: &str, name: &str) -> Vec<Vec<String>> {
    lock.split("[[package]]")
        .map(|block| block.lines().map(str::trim).map(String::from).collect::<Vec<_>>())
        .filter(|lines| lines.iter().any(|l| *l == format!("name = \"{name}\"")))
        .collect()
}

/// `Cargo.lock` builds each vendored crate from its path (a path package has no `source`),
/// at the vendored version, and has no other copy of it; the crates left to the lockfile are
/// the pinned registry versions.
#[test]
fn the_vendored_copies_are_the_ones_that_build() {
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).expect("Cargo.lock");
    for (name, version, _) in VENDORED {
        let blocks = locked(&lock, name);
        assert_eq!(
            blocks.len(),
            1,
            "{name}: expected exactly one copy in Cargo.lock, found {}",
            blocks.len()
        );
        let block = &blocks[0];
        assert!(block.contains(&format!("version = \"{version}\"")), "{name}: not version {version}");
        assert!(
            !block.iter().any(|l| l.starts_with("source") || l.starts_with("checksum")),
            "{name}: Cargo.lock takes it from a registry, not vendor/{name}"
        );
    }
    for (name, version, checksum) in LOCKED {
        let blocks = locked(&lock, name);
        let pinned = blocks.iter().any(|b| {
            b.contains(&format!("version = \"{version}\""))
                && b.contains(
                    &"source = \"registry+https://github.com/rust-lang/crates.io-index\"".to_string(),
                )
                && b.contains(&format!("checksum = \"{checksum}\""))
        });
        assert!(pinned, "{name} {version} is not locked from crates.io with checksum {checksum}");
    }
}

/// Every `"manifest_path":"..."` in `cargo metadata`'s JSON: the resolved graph's packages.
fn manifest_paths(metadata: &str) -> BTreeSet<String> {
    let key = "\"manifest_path\":\"";
    metadata
        .match_indices(key)
        .map(|(at, _)| {
            let rest = &metadata[at + key.len()..];
            rest[..rest.find('"').expect("unterminated manifest_path")].to_string()
        })
        .collect()
}

/// A path package has no source in `Cargo.lock` wherever its directory is, so the lockfile
/// alone cannot show that the patches point at `vendor/`. The resolved graph can: each
/// vendored crate's manifest is `vendor/<name>/Cargo.toml` in this tree, and no registry copy
/// (`.../<name>-<version>/Cargo.toml`) of it is in the graph at all. The riscv64 filter keeps
/// `--offline` from needing host-only crates the registry cache may lack.
#[test]
fn the_patches_point_at_vendor() {
    let root = root().canonicalize().expect("repository root");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .current_dir(&root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--filter-platform",
            "riscv64imac-unknown-none-elf",
        ])
        .output()
        .expect("run cargo metadata");
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    let paths = manifest_paths(&String::from_utf8(out.stdout).expect("UTF-8 metadata"));
    for (name, version, _) in VENDORED {
        let vendored = root.join("vendor").join(name).join("Cargo.toml");
        assert!(paths.contains(vendored.to_str().unwrap()), "{name}: not built from {}", vendored.display());
        let registry = format!("/{name}-{version}/Cargo.toml");
        let copies: Vec<_> = paths.iter().filter(|p| p.ends_with(&registry)).collect();
        assert!(copies.is_empty(), "{name}: a registry copy is in the graph: {copies:?}");
    }
}

/// The versions and checksums above are the ones vendor/README.md records, so the two cannot
/// drift apart.
#[test]
fn the_readme_records_each_crate() {
    let readme = std::fs::read_to_string(root().join("vendor/README.md")).expect("vendor/README.md");
    for (name, version, checksum) in VENDORED.iter().chain(LOCKED) {
        let row = format!("| `{name}` | {version} |");
        let line = readme.lines().find(|l| l.starts_with(&row)).unwrap_or_else(|| panic!("no row {row:?}"));
        assert!(line.contains(checksum), "vendor/README.md's row for {name} lacks {checksum}");
    }
}
