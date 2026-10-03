//! Disks: a GPT of equal partitions for a case's `[disk]`, and a disk recipe (`image/disk.toml`)
//! packed whole, its partition table by `blkd`'s builder and each littlefs partition by `fsd`'s
//! own packer (docs/testbench.md, "Disks and network cards"; image/README.md).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use redoubt_blkd::image::{Entry, FIRST_USABLE, Image};
use redoubt_fsd::pack;
use serde::Deserialize;

/// A disk sector, in bytes.
pub const SECTOR: u64 = 512;

/// A disk recipe: its size and its partitions, in table order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// The whole disk, in KiB (a whole number of sectors).
    pub size_kib: u64,
    pub partition: Vec<Partition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    /// The volume's name, as the manifest's `volumes` entry names it.
    pub name: String,
    /// What the partition holds: `littlefs`, the only filesystem there is, or, for a case,
    /// `noise`: the same pseudo-random bytes every time, which no filesystem mounts.
    pub fs: String,
    /// For `littlefs`, the directory whose tree the volume holds, relative to the workspace root.
    pub stage: Option<PathBuf>,
    /// For `littlefs`, files made for the pack in the volume's root, beside the stage's tree.
    pub generated: Option<Generated>,
}

/// `files` files in a volume's root, `f000` and on, as many digits as the last needs: empty, but
/// for `read`, which holds its own name and a newline.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generated {
    pub files: usize,
    pub read: String,
}

impl Generated {
    /// The files, in name order.
    fn files(&self) -> Vec<(String, Option<Vec<u8>>)> {
        let width = self.files.saturating_sub(1).to_string().len();
        (0..self.files)
            .map(|i| {
                let name = format!("f{i:0width$}");
                let data = if name == self.read { format!("{name}\n").into_bytes() } else { Vec::new() };
                (name, Some(data))
            })
            .collect()
    }
}

impl Recipe {
    pub fn load(path: &Path) -> Result<Recipe> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let recipe: Recipe = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        ensure!(!recipe.partition.is_empty(), "{}: no partition", path.display());
        for p in &recipe.partition {
            match p.fs.as_str() {
                "littlefs" => {
                    let (stage, generated) = (p.stage.is_some(), p.generated.as_ref());
                    ensure!(
                        stage || generated.is_some(),
                        "{}: partition {}: no stage and nothing generated",
                        path.display(),
                        p.name
                    );
                    if let Some(g) = generated {
                        ensure!(
                            g.files().iter().any(|(n, _)| *n == g.read),
                            "{}: partition {}: {} is not among the generated files",
                            path.display(),
                            p.name,
                            g.read
                        );
                    }
                }
                "noise" => ensure!(
                    p.stage.is_none() && p.generated.is_none(),
                    "{}: partition {}: noise is neither staged nor generated",
                    path.display(),
                    p.name
                ),
                fs => {
                    bail!("{}: partition {}: fs {fs:?} is neither littlefs nor noise", path.display(), p.name)
                }
            }
        }
        Ok(recipe)
    }
}

/// The partitions of a disk of `sectors` sectors: `count` equal shares of the space between the
/// table and its backup at the end, which takes as many sectors as the array and a header.
fn shares(sectors: u64, count: u64) -> Vec<Entry> {
    let share = sectors.saturating_sub(2 * FIRST_USABLE) / count;
    (0..count)
        .map(|i| Entry { first_lba: FIRST_USABLE + i * share, last_lba: FIRST_USABLE + (i + 1) * share - 1 })
        .collect()
}

/// A disk of `sectors` sectors holding a GPT with `partitions` equal, empty partitions.
pub fn gpt_disk(sectors: u64, partitions: u64) -> Vec<u8> {
    Image::new(sectors, &shares(sectors, partitions)).bytes
}

/// The tree under `stage`, parents first, in name order, as `fsd`'s packer takes it: each path
/// relative to `stage`, and each file's bytes.
fn tree(stage: &Path) -> Result<Vec<(String, Option<Vec<u8>>)>> {
    let mut out = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(dir) = todo.pop() {
        let mut names: Vec<_> = std::fs::read_dir(stage.join(&dir))
            .with_context(|| format!("reading {}", stage.join(&dir).display()))?
            .collect::<std::io::Result<_>>()?;
        names.sort_by_key(|e| e.file_name());
        let mut dirs = Vec::new();
        for entry in &names {
            let rel = dir.join(entry.file_name());
            let Some(path) = rel.to_str().map(str::to_string) else {
                bail!("{} is not UTF-8", rel.display())
            };
            let kind = entry.file_type()?;
            if kind.is_dir() {
                out.push((path, None));
                dirs.push(rel);
            } else if kind.is_file() {
                out.push((path, Some(std::fs::read(entry.path())?)));
            } else {
                bail!("{}: neither a file nor a directory", entry.path().display());
            }
        }
        // Walked in name order: the first directory is popped first.
        todo.extend(dirs.into_iter().rev());
    }
    // A directory before anything in it: the walk above pushed it before its children.
    Ok(out)
}

/// Fills `bytes` with the same pseudo-random bytes every time (xorshift64).
fn noise(bytes: &mut [u8]) {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for b in bytes {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *b = seed as u8;
    }
}

/// The disk `recipe` describes, its stages read under `root`; `stage`, if given, stands in for
/// every littlefs partition's own.
pub fn pack_disk(recipe: &Recipe, root: &Path, stage: Option<&Path>) -> Result<Vec<u8>> {
    ensure!(recipe.size_kib > 0, "a disk of no size");
    let sectors = recipe.size_kib * 1024 / SECTOR;
    let parts = shares(sectors, recipe.partition.len() as u64);
    let mut disk = Image::new(sectors, &parts).bytes;
    for (p, at) in recipe.partition.iter().zip(&parts) {
        let (start, end) = ((at.first_lba * SECTOR) as usize, ((at.last_lba + 1) * SECTOR) as usize);
        if p.fs == "noise" {
            noise(&mut disk[start..end]);
            continue;
        }
        let mut staged = match stage.or(p.stage.as_deref()) {
            Some(dir) => tree(&root.join(dir))?,
            None => Vec::new(),
        };
        for (name, data) in p.generated.iter().flat_map(Generated::files) {
            ensure!(
                !staged.iter().any(|(path, _)| *path == name),
                "{}: {name} is staged and generated",
                p.name
            );
            staged.push((name, data));
        }
        let entries: Vec<pack::Entry> = staged
            .iter()
            .map(|(path, data)| match data {
                Some(data) => pack::Entry::File(path, data),
                None => pack::Entry::Dir(path),
            })
            .collect();
        let volume = pack::pack(at.last_lba - at.first_lba + 1, &entries)
            .map_err(|e| anyhow::anyhow!("packing {}: {}: {}", p.name, e.path, e.why))?;
        disk[start..start + volume.len()].copy_from_slice(&volume);
    }
    Ok(disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stage of its own under the system's temporary directory.
    fn stage(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("testbench-disk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("etc/deep")).unwrap();
        std::fs::write(dir.join("motd"), b"hello").unwrap();
        std::fs::write(dir.join("etc/deep/notes"), b"deep").unwrap();
        dir
    }

    #[test]
    fn a_stage_is_walked_parents_first_in_name_order() {
        let dir = stage("walk");
        let names: Vec<String> = tree(&dir).unwrap().into_iter().map(|(p, _)| p).collect();
        assert_eq!(names, ["etc", "motd", "etc/deep", "etc/deep/notes"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The recipe's disk holds a GPT whose one partition starts with a littlefs superblock, and
    /// is the recipe's size.
    #[test]
    fn a_recipe_packs_a_table_and_a_volume_per_partition() {
        let dir = stage("pack");
        let recipe: Recipe = toml::from_str(
            "size_kib = 1024\n[[partition]]\nname = \"data\"\nfs = \"littlefs\"\nstage = \"x\"\n",
        )
        .unwrap();
        let disk = pack_disk(&recipe, Path::new("/"), Some(&dir)).unwrap();
        assert_eq!(disk.len(), 1024 * 1024);
        let at = (FIRST_USABLE * SECTOR) as usize;
        assert!(disk[at..at + 4096].windows(8).any(|w| w == b"littlefs"));
        assert!(disk[at + 4096..].iter().any(|b| *b != 0), "the files are past the superblock pair");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A generated directory is its files in the volume's root, beside the stage's tree: all
    /// empty but the one to read, and none in the stage's place.
    #[test]
    fn a_recipe_can_generate_a_directory_of_files() {
        let recipe = |extra: &str| {
            let text = format!("size_kib = 1024\n[[partition]]\nname = \"data\"\nfs = \"littlefs\"\n{extra}");
            let path = std::env::temp_dir().join(format!("testbench-recipe-{}.toml", std::process::id()));
            std::fs::write(&path, text).unwrap();
            let recipe = Recipe::load(&path);
            std::fs::remove_file(path).unwrap();
            recipe
        };
        let g = Generated { files: 600, read: "f007".into() };
        let files = g.files();
        assert_eq!((files.len(), files[0].0.as_str(), files[599].0.as_str()), (600, "f000", "f599"));
        assert!(
            files.iter().all(|(n, d)| d.as_deref() == Some(if n == "f007" { &b"f007\n"[..] } else { b"" }))
        );
        assert!(recipe("generated = { files = 10, read = \"f10\" }\n").is_err(), "f10 is not made");
        assert!(recipe("").is_err(), "a littlefs partition holds something");
        let only = recipe("generated = { files = 10, read = \"f9\" }\n").unwrap();
        let disk = pack_disk(&only, Path::new("/"), None).unwrap();
        assert!(disk.windows(3).any(|w| w == b"f9\n"), "f9 holds its line");
        let dir = stage("generated");
        std::fs::write(dir.join("f1"), b"staged").unwrap();
        assert!(pack_disk(&only, Path::new("/"), Some(&dir)).is_err(), "f1 is staged and generated");
        std::fs::remove_file(dir.join("f1")).unwrap();
        let both = pack_disk(&only, Path::new("/"), Some(&dir)).unwrap();
        assert!(both.windows(5).any(|w| w == b"hello") && both.windows(3).any(|w| w == b"f9\n"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A `noise` partition is the same bytes in every pack, and no littlefs superblock.
    #[test]
    fn a_noise_partition_is_the_same_noise_every_time() {
        let recipe: Recipe = toml::from_str(
            "size_kib = 1024\n[[partition]]\nname = \"a\"\nfs = \"noise\"\n[[partition]]\nname = \"b\"\nfs = \"noise\"\n",
        )
        .unwrap();
        let disk = pack_disk(&recipe, Path::new("/"), None).unwrap();
        assert_eq!(disk, pack_disk(&recipe, Path::new("/"), None).unwrap());
        let at = (FIRST_USABLE * SECTOR) as usize;
        assert!(!disk[at..at + 8192].windows(8).any(|w| w == b"littlefs"));
        assert!(disk[at..at + 512].iter().any(|b| *b != 0));
    }
}
