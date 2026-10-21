//! A prebuilt directory (docs/testbench.md, "Building once"): every case's kernel, loader,
//! programs, bundle and userland disk for one target, built by one `--prebuild`, with the bench's
//! own binary beside them, so that `--prebuilt` runs a case with no cargo and no packing.
//!
//! ```text
//! DIR/testbench            the bench, copied from the `--prebuild` that made the directory
//! DIR/<arch>/index.json    the tree that built it and its fingerprint, and each case's pieces or
//!                          its build's results
//! DIR/<arch>/...           the pieces: cargo's copies, the bundles, the userland disks
//! ```
//!
//! A target's pieces are built in a directory of their own and renamed into place when done, so
//! a run reading the old ones never sees a half-written file. The fingerprint is the tree's commit
//! and its uncommitted changes: a run from a directory made from another tree, or from this one as
//! it was before a change, is refused.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Outcome;
use crate::disk::Verified;
use crate::userland::Staged;

/// The bench's own binary in a prebuilt directory.
pub const BINARY: &str = "testbench";
const INDEX: &str = "index.json";
/// The git command that prints the tree's uncommitted changes to tracked files: what `git diff
/// HEAD --binary` prints under git's default configuration, renames found (`-M`) as it finds them,
/// with none of the index refresh `git diff` makes and none of the `diff.*` settings it follows.
const CHANGES: [&str; 5] = ["diff-index", "-p", "--binary", "-M", "HEAD"];

/// A boot case's pieces, packed for one target: what it boots, every boot alike.
#[derive(Clone)]
pub struct Packed {
    pub loader: PathBuf,
    pub bundle: PathBuf,
    pub userland: Option<Staged>,
}

/// What a `--prebuild` left for one case: its pieces, or the results of a case that ends at its
/// build (a build case, a target that cannot boot, a build that failed).
pub enum Entry {
    Packed(Packed),
    Results(Vec<(String, Outcome, f32)>),
}

/// One target's index, as written: paths relative to the target's directory.
#[derive(Serialize, Deserialize)]
struct Index {
    /// The workspace whose `--prebuild` made it.
    workspace: PathBuf,
    fingerprint: String,
    cases: BTreeMap<String, Record>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Record {
    Packed { loader: PathBuf, bundle: PathBuf, userland: Option<UserlandRecord> },
    Results(Vec<(String, Outcome, f32)>),
}

#[derive(Serialize, Deserialize)]
struct UserlandRecord {
    objects: PathBuf,
    image: PathBuf,
    verified: Vec<VerifiedRecord>,
}

#[derive(Serialize, Deserialize)]
struct VerifiedRecord {
    name: String,
    root: [u8; 32],
    blocks: u64,
    start: usize,
    signed: Option<(u64, usize)>,
}

/// The tree a prebuilt directory was made from: a digest of its commit, its uncommitted changes
/// to tracked files, and the names and bytes of its untracked files that git does not ignore.
/// Each git command only reads. The changes come from `git diff-index`, not `git diff`: `git diff`
/// refreshes the index and so takes `.git/index.lock`, which a run killed inside it leaves behind
/// to block every later commit.
pub fn fingerprint(workspace: &Path) -> Result<String> {
    let git = |args: &[&str]| -> Result<Vec<u8>> {
        let output = Command::new("git").current_dir(workspace).args(args).output().context("running git")?;
        ensure!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output.stdout)
    };
    let mut digest = Sha256::new();
    digest.update(git(&["rev-parse", "HEAD"])?);
    digest.update(git(&CHANGES)?);
    for name in git(&["ls-files", "--others", "--exclude-standard", "-z"])?.split(|&b| b == 0) {
        if name.is_empty() {
            continue;
        }
        let path = workspace.join(String::from_utf8_lossy(name).as_ref());
        digest.update(name);
        digest.update([0]);
        digest.update(std::fs::read(&path).unwrap_or_default());
    }
    Ok(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The directory a target's pieces are built in before they are renamed into place.
pub fn building(dir: &Path, arch: &str) -> PathBuf { dir.join(format!(".{arch}-{}", std::process::id())) }

/// Write `entries`, built in `staging` for `arch` from `workspace` as `fingerprint` names it, as
/// `dir`'s index for that target, and rename `staging` into place; the target's old pieces are
/// removed.
pub fn finish(
    dir: &Path,
    arch: &str,
    staging: &Path,
    workspace: &Path,
    fingerprint: String,
    entries: Vec<(String, Entry)>,
) -> Result<()> {
    let relative = |path: &Path| -> Result<PathBuf> {
        path.strip_prefix(staging)
            .map(Path::to_path_buf)
            .with_context(|| format!("{} is outside {}", path.display(), staging.display()))
    };
    let mut cases = BTreeMap::new();
    for (name, entry) in entries {
        let record = match entry {
            Entry::Results(results) => Record::Results(results),
            Entry::Packed(Packed { loader, bundle, userland }) => Record::Packed {
                loader: relative(&loader)?,
                bundle: relative(&bundle)?,
                userland: userland
                    .map(|staged| -> Result<UserlandRecord> {
                        Ok(UserlandRecord {
                            objects: relative(&staged.objects)?,
                            image: relative(&staged.image)?,
                            verified: staged
                                .verified
                                .iter()
                                .map(|v| VerifiedRecord {
                                    name: v.name.clone(),
                                    root: v.root,
                                    blocks: v.geometry.data_blocks(),
                                    start: v.start,
                                    signed: v.signed,
                                })
                                .collect(),
                        })
                    })
                    .transpose()?,
            },
        };
        cases.insert(name, record);
    }
    let index = serde_json::to_vec_pretty(&Index { workspace: workspace.to_path_buf(), fingerprint, cases })?;
    std::fs::write(staging.join(INDEX), index)?;
    let target = dir.join(arch);
    let old = dir.join(format!(".{arch}-old-{}", std::process::id()));
    if target.exists() {
        std::fs::rename(&target, &old).with_context(|| format!("moving {} aside", target.display()))?;
    }
    std::fs::rename(staging, &target)
        .with_context(|| format!("renaming {} into place", staging.display()))?;
    if old.exists() {
        std::fs::remove_dir_all(&old).with_context(|| format!("removing {}", old.display()))?;
    }
    Ok(())
}

/// Copy the running bench into `dir`.
pub fn copy_binary(dir: &Path) -> Result<()> {
    copy(&std::env::current_exe().context("finding the bench's own binary")?, &dir.join(BINARY))
}

/// Copy `from` to `to` by a rename, so a run starting meanwhile finds the old file or the new one.
pub fn copy(from: &Path, to: &Path) -> Result<()> {
    let partial = to.with_file_name(format!(
        ".{}-{}",
        to.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    std::fs::copy(from, &partial).with_context(|| format!("copying {}", from.display()))?;
    std::fs::rename(&partial, to).with_context(|| format!("renaming into {}", to.display()))
}

/// One target's entries in `dir`, refused if the directory was made by another workspace than
/// `workspace`, or from it as it was before a change (another `fingerprint`).
pub fn load(dir: &Path, arch: &str, workspace: &Path, fingerprint: &str) -> Result<BTreeMap<String, Entry>> {
    let base = dir.join(arch);
    let path = base.join(INDEX);
    let text = std::fs::read(&path)
        .with_context(|| format!("{} has no {arch} pieces: make them with --prebuild", dir.display()))?;
    let index: Index =
        serde_json::from_slice(&text).with_context(|| format!("reading {}", path.display()))?;
    if index.workspace != workspace {
        bail!(
            "{} was built by the tree at {}, not this one at {}: make it here with --prebuild",
            dir.display(),
            index.workspace.display(),
            workspace.display()
        );
    }
    if index.fingerprint != fingerprint {
        bail!("{} was built from this tree before it changed: make it again with --prebuild", dir.display());
    }
    let mut entries = BTreeMap::new();
    for (name, record) in index.cases {
        let entry = match record {
            Record::Results(results) => Entry::Results(results),
            Record::Packed { loader, bundle, userland } => Entry::Packed(Packed {
                loader: base.join(loader),
                bundle: base.join(bundle),
                userland: userland
                    .map(|u| -> Result<Staged> {
                        let verified = u
                            .verified
                            .into_iter()
                            .map(|v| {
                                let geometry = redoubt_verity::Geometry::new(v.blocks)
                                    .with_context(|| format!("volume {}: {} blocks", v.name, v.blocks))?;
                                Ok(Verified {
                                    name: v.name,
                                    root: v.root,
                                    geometry,
                                    start: v.start,
                                    signed: v.signed,
                                })
                            })
                            .collect::<Result<_>>()?;
                        Ok(Staged { objects: base.join(u.objects), image: base.join(u.image), verified })
                    })
                    .transpose()?,
            }),
        };
        entries.insert(name, entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target's index keeps each case's pieces relative to its directory, so the renamed
    /// directory resolves them; a build's results come back as they were; a directory made by
    /// another workspace is refused naming it, and one made from this workspace before a change is
    /// refused too; a second prebuild replaces the first and leaves nothing beside it.
    #[test]
    fn an_index_survives_its_rename_and_names_its_tree() {
        let dir = std::env::temp_dir().join(format!("testbench-prebuilt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let build = |marker: &str| {
            let staging = building(&dir, "rv64");
            std::fs::create_dir_all(staging.join("userland/u")).unwrap();
            std::fs::write(staging.join("case-rv64.tar"), marker).unwrap();
            let geometry = redoubt_verity::Geometry::new(1000).unwrap();
            let verified =
                Verified { name: "system".into(), root: [7; 32], geometry, start: 512, signed: Some((3, 9)) };
            let packed = Packed {
                loader: staging.join("cargo/loader"),
                bundle: staging.join("case-rv64.tar"),
                userland: Some(Staged {
                    objects: staging.join("userland/u/objects"),
                    image: staging.join("userland/u/userland.img"),
                    verified: vec![verified],
                }),
            };
            let failed = vec![(String::new(), Outcome::Fail("building x failed".into()), 1.5)];
            let entries =
                vec![("case".to_string(), Entry::Packed(packed)), ("broken".into(), Entry::Results(failed))];
            finish(&dir, "rv64", &staging, Path::new("/w/a"), "tree-a".into(), entries).unwrap();
        };
        build("first");
        build("second");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["rv64"], "a prebuild left its old pieces or its staging behind");

        let entries = load(&dir, "rv64", Path::new("/w/a"), "tree-a").unwrap();
        let Some(Entry::Packed(packed)) = entries.get("case") else { panic!("case is not packed") };
        assert_eq!(std::fs::read_to_string(&packed.bundle).unwrap(), "second");
        assert_eq!(packed.loader, dir.join("rv64/cargo/loader"));
        let staged = packed.userland.as_ref().unwrap();
        assert_eq!(staged.image, dir.join("rv64/userland/u/userland.img"));
        let v = &staged.verified[0];
        assert_eq!((v.root, v.geometry.data_blocks(), v.start, v.signed), ([7; 32], 1000, 512, Some((3, 9))));
        let Some(Entry::Results(results)) = entries.get("broken") else { panic!("broken has no results") };
        assert!(matches!(&results[..], [(_, Outcome::Fail(why), _)] if why == "building x failed"));

        let changed = load(&dir, "rv64", Path::new("/w/a"), "tree-b").err().unwrap().to_string();
        assert!(changed.contains("before it changed"), "{changed}");
        let other = load(&dir, "rv64", Path::new("/w/b"), "tree-a").err().unwrap().to_string();
        assert!(other.contains("built by the tree at /w/a, not this one at /w/b"), "{other}");
        assert!(
            load(&dir, "rv32", Path::new("/w/a"), "tree-a")
                .err()
                .unwrap()
                .to_string()
                .contains("no rv32 pieces")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The fingerprint moves with an uncommitted change to a tracked file and with an untracked
    /// file's bytes, and not with an ignored file.
    #[test]
    fn the_fingerprint_follows_the_trees_changes() {
        let dir = std::env::temp_dir().join(format!("testbench-fingerprint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join(".gitignore"), "ignored\n").unwrap();
        std::fs::write(dir.join("tracked"), "a").unwrap();
        git(&["add", ".gitignore", "tracked"]);
        git(&["commit", "-q", "-m", "x"]);
        let clean = fingerprint(&dir).unwrap();
        std::fs::write(dir.join("ignored"), "x").unwrap();
        assert_eq!(fingerprint(&dir).unwrap(), clean, "an ignored file moved it");
        std::fs::write(dir.join("tracked"), "b").unwrap();
        let edited = fingerprint(&dir).unwrap();
        assert_ne!(edited, clean);
        std::fs::write(dir.join("new"), "1").unwrap();
        let untracked = fingerprint(&dir).unwrap();
        assert_ne!(untracked, edited);
        std::fs::write(dir.join("new"), "2").unwrap();
        assert_ne!(fingerprint(&dir).unwrap(), untracked, "an untracked file's bytes did not move it");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The fingerprint never writes the index, so a run killed inside it leaves no
    /// `.git/index.lock`: with a tracked file's times moved since the index recorded them, where
    /// `git diff` refreshes the index and rewrites it, the index's bytes stay as they were; and the
    /// moved times, the file's bytes unchanged, leave the fingerprint as it was before them.
    #[test]
    fn the_fingerprint_writes_no_index() {
        let dir = std::env::temp_dir().join(format!("testbench-fingerprint-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join("tracked"), "a").unwrap();
        git(&["add", "tracked"]);
        git(&["commit", "-q", "-m", "x"]);
        let before = fingerprint(&dir).unwrap();
        let then = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(dir.join("tracked")).unwrap().set_modified(then).unwrap();
        let index = std::fs::read(dir.join(".git/index")).unwrap();
        assert_eq!(fingerprint(&dir).unwrap(), before, "moved times alone moved it");
        assert!(std::fs::read(dir.join(".git/index")).unwrap() == index, "the fingerprint rewrote the index");
        assert!(!dir.join(".git/index.lock").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The changes the fingerprint reads are what `git diff HEAD --binary` prints under git's
    /// default configuration: an edit, a staged new file, a deletion, a staged rename with an edit,
    /// a mode change and a binary file's edit.
    #[test]
    fn the_fingerprints_changes_are_git_diffs() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("testbench-fingerprint-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(&dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args([
                    "-c",
                    "diff.renames=true",
                    "-c",
                    "diff.noprefix=false",
                    "-c",
                    "diff.mnemonicPrefix=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}");
            output.stdout
        };
        let lines = |n: usize| (1..=n).map(|i| format!("{i}\n")).collect::<String>();
        git(&["init", "-q"]);
        std::fs::write(dir.join("edited"), lines(50)).unwrap();
        std::fs::write(dir.join("renamed"), lines(40)).unwrap();
        std::fs::write(dir.join("binary"), b"x\0y").unwrap();
        std::fs::write(dir.join("deleted"), "d\n").unwrap();
        std::fs::write(dir.join("mode"), "m\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "x"]);
        std::fs::write(dir.join("edited"), lines(51)).unwrap();
        git(&["mv", "renamed", "moved"]);
        std::fs::write(dir.join("moved"), lines(41)).unwrap();
        git(&["add", "moved"]);
        std::fs::write(dir.join("binary"), b"x\0z").unwrap();
        std::fs::remove_file(dir.join("deleted")).unwrap();
        std::fs::set_permissions(dir.join("mode"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("added"), "new\n").unwrap();
        git(&["add", "added"]);
        let changes = git(&CHANGES);
        let text = String::from_utf8_lossy(&changes);
        for header in [
            "rename from renamed",
            "new file mode",
            "deleted file mode",
            "new mode 100755",
            "GIT binary patch",
        ] {
            assert!(text.contains(header), "no {header:?} in:\n{text}");
        }
        assert!(changes == git(&["diff", "HEAD", "--binary"]), "diff-index and diff differ");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
