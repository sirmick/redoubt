//! Disks: a GPT of equal partitions for a case's `[disk]`, and a disk recipe (`image/disk.toml`)
//! packed whole, its partition table by `blkd`'s builder, each littlefs partition by `littlefsd`'s
//! own packer and each EROFS partition by `libs/erofs`'s writer (docs/testbench.md, "Disks and
//! network cards"; image/README.md). A partition the
//! recipe marks `verity` holds the largest volume that fits beside its hash tree, and the tree
//! after it (docs/servers/verityd.md, "The tree"); its root and block count are what a manifest
//! pins.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use redoubt_blkd::image::{Entry, FIRST_USABLE, Image};
use redoubt_littlefsd::pack;
use redoubt_verity::{BLOCK, Geometry, Hash, SECTORS_PER_BLOCK};
use serde::Deserialize;

/// A disk sector, in bytes.
pub const SECTOR: u64 = 512;

/// A disk recipe: its size and its partitions, in table order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// The whole disk, in KiB (a whole number of sectors).
    pub size_kib: u64,
    /// For the userland disk, the objects its one partition's stage holds, staged before the
    /// pack (`userland.rs`).
    pub objects: Option<crate::userland::Objects>,
    pub partition: Vec<Partition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    /// The volume's name, as the manifest's `volumes` entry names it.
    pub name: String,
    /// What the partition holds: `littlefs`, a writable volume; `erofs`, a read-only one; or, for
    /// a case, `noise`: the same pseudo-random bytes every time, which no filesystem mounts.
    pub fs: String,
    /// For `littlefs` or `erofs`, the directory whose tree the volume holds, relative to the
    /// workspace root.
    /// A userland disk's (a recipe with `objects`) is where `--pack-disk` stages them, and is left
    /// out where only the bench packs it, from its own staging.
    pub stage: Option<PathBuf>,
    /// For `littlefs` or `erofs`, files made for the pack in the volume's root, beside the
    /// stage's tree.
    pub generated: Option<Generated>,
    /// For `littlefs` or `erofs`, a verified volume: the volume is followed by its hash tree, and
    /// the pack says its root and block count, which the manifest pins.
    #[serde(default)]
    pub verity: bool,
    /// For `erofs`, for a case: one thing the packed volume is made to hold that `erofsd` must
    /// serve as corrupt ([`Damage`]).
    pub damage: Option<Damage>,
}

/// What an EROFS volume is damaged with after its pack, for `erofs-corrupt`
/// (servers/erofsd.md, "The format"): `magic`, a bit of the superblock's magic flipped;
/// `block-past-count`, the file at `path` (with at least one whole block) starting at the volume's
/// block count; `compressed`, the file at `path` laid out as compressed; `name-offset`, the last
/// name of the first block of the directory at `path` starting past the block's end.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Damage {
    pub what: String,
    #[serde(default)]
    pub path: String,
}

/// A verified partition as packed: the root and data blocks its manifest entry pins, and where
/// its volume and tree lie on the disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    /// The volume's name, as the manifest's `volumes` entry names it.
    pub name: String,
    pub root: Hash,
    pub geometry: Geometry,
    /// The partition's first byte on the disk.
    pub start: usize,
}

impl Verified {
    /// The root as the manifest takes it: 64 lowercase hex digits.
    pub fn root_hex(&self) -> String { self.root.iter().map(|b| format!("{b:02x}")).collect() }

    /// The bytes of the volume's data blocks on `disk`.
    pub fn data<'d>(&self, disk: &'d [u8]) -> &'d [u8] {
        &disk[self.start..self.start + self.geometry.data_blocks() as usize * BLOCK]
    }
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
        // The objects are the partition's stage, staged whole before the pack: nothing beside them.
        ensure!(
            recipe.objects.is_none()
                || matches!(&recipe.partition[..], [p] if p.fs != "noise" && p.generated.is_none()),
            "{}: objects are one littlefs or erofs partition's stage, with nothing generated",
            path.display()
        );
        for p in &recipe.partition {
            ensure!(
                p.damage.is_none() || p.fs == "erofs",
                "{}: partition {}: only an erofs volume is damaged",
                path.display(),
                p.name
            );
            match p.fs.as_str() {
                "littlefs" | "erofs" => {
                    let (stage, generated) = (p.stage.is_some(), p.generated.as_ref());
                    ensure!(
                        stage || generated.is_some() || recipe.objects.is_some(),
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
                    p.stage.is_none() && p.generated.is_none() && !p.verity,
                    "{}: partition {}: noise is neither staged nor generated",
                    path.display(),
                    p.name
                ),
                fs => {
                    bail!(
                        "{}: partition {}: fs {fs:?} is not littlefs, erofs or noise",
                        path.display(),
                        p.name
                    )
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

/// The tree under `stage`, parents first, in name order, as both packers take it: each path
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
/// every littlefs and erofs partition's own.
pub fn pack_disk(recipe: &Recipe, root: &Path, stage: Option<&Path>) -> Result<Vec<u8>> {
    pack(recipe, root, stage).map(|(disk, _)| disk)
}

/// [`pack_disk`], and each verified partition's root and geometry, in table order.
pub fn pack(recipe: &Recipe, root: &Path, stage: Option<&Path>) -> Result<(Vec<u8>, Vec<Verified>)> {
    ensure!(recipe.size_kib > 0, "a disk of no size");
    let sectors = recipe.size_kib * 1024 / SECTOR;
    let parts = shares(sectors, recipe.partition.len() as u64);
    let mut disk = Image::new(sectors, &parts).bytes;
    let mut verified = Vec::new();
    for (p, at) in recipe.partition.iter().zip(&parts) {
        let (start, end) = ((at.first_lba * SECTOR) as usize, ((at.last_lba + 1) * SECTOR) as usize);
        if p.fs == "noise" {
            noise(&mut disk[start..end]);
            continue;
        }
        let own = stage.or(p.stage.as_deref());
        ensure!(
            own.is_some() || p.generated.is_some(),
            "partition {}: no stage and nothing generated",
            p.name
        );
        let mut staged = match own {
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
        let sectors = at.last_lba - at.first_lba + 1;
        // A verified volume is the largest whose data and tree fit the partition.
        let geometry = match p.verity {
            true => Some(
                Geometry::largest(sectors / SECTORS_PER_BLOCK)
                    .with_context(|| format!("{}: no room for a verified volume", p.name))?,
            ),
            false => None,
        };
        let data = geometry.map_or(sectors, |g| g.data_blocks() * SECTORS_PER_BLOCK);
        let volume = match p.fs.as_str() {
            "erofs" => {
                let mut volume = pack_erofs(&staged).with_context(|| format!("packing {}", p.name))?;
                ensure!(
                    volume.len() as u64 <= data * SECTOR,
                    "{}: the volume's {} bytes do not fit the partition's {}",
                    p.name,
                    volume.len(),
                    data * SECTOR
                );
                if let Some(damage) = &p.damage {
                    damage_erofs(&mut volume, damage).with_context(|| format!("damaging {}", p.name))?;
                }
                // The rest of the range reads as zeros: a verified volume's tree covers it too.
                volume.resize((data * SECTOR) as usize, 0);
                volume
            }
            _ => {
                let entries: Vec<pack::Entry> = staged
                    .iter()
                    .map(|(path, data)| match data {
                        Some(data) => pack::Entry::File(path, data),
                        None => pack::Entry::Dir(path),
                    })
                    .collect();
                pack::pack(data, &entries)
                    .map_err(|e| anyhow::anyhow!("packing {}: {}: {}", p.name, e.path, e.why))?
            }
        };
        disk[start..start + volume.len()].copy_from_slice(&volume);
        if let Some(geometry) = geometry {
            let mut tree = vec![0u8; geometry.tree_blocks() as usize * BLOCK];
            let root = redoubt_verity::build(&geometry, &volume, &mut tree)
                .map_err(|_| anyhow::anyhow!("{}: the volume is not its geometry's size", p.name))?;
            let at = start + volume.len();
            disk[at..at + tree.len()].copy_from_slice(&tree);
            verified.push(Verified { name: p.name.clone(), root, geometry, start });
        }
    }
    Ok((disk, verified))
}

/// `staged` as an EROFS volume, each file's SHA-256 its `user.sha256`.
fn pack_erofs(staged: &[(String, Option<Vec<u8>>)]) -> Result<Vec<u8>> {
    use sha2::Digest;
    let entries: Vec<erofs::Entry> = staged
        .iter()
        .map(|(path, data)| match data {
            Some(data) => erofs::Entry::File(path, data),
            None => erofs::Entry::Dir(path),
        })
        .collect();
    erofs::pack(&entries, |data| sha2::Sha256::digest(data).into())
        .map_err(|e| anyhow::anyhow!("{}: {}", e.path, e.why))
}

/// Where the inode at `path` (`""` the root) starts in the EROFS `volume`, and the inode, found
/// through the parser `erofsd` uses.
fn erofs_inode(volume: &[u8], path: &str) -> Result<(usize, erofs::Inode)> {
    let corrupt = |_| anyhow::anyhow!("the packed volume does not parse");
    let sb = erofs::Superblock::parse(volume, (volume.len() / BLOCK) as u64).map_err(corrupt)?;
    let inode = |nid| -> Result<(usize, erofs::Inode)> {
        let at = sb.inode_at(nid).map_err(corrupt)? as usize;
        let end = volume.len().min(at + erofs::EXTENDED);
        Ok((at, erofs::Inode::parse(&sb, nid, &volume[at..end]).map_err(corrupt)?))
    };
    let mut found = inode(sb.root)?;
    for name in path.split('/').filter(|n| !n.is_empty()) {
        let dir = found.1;
        let nid = (0..dir.dir_blocks())
            .filter_map(|i| dir.dir_block(i))
            .find_map(|(at, len)| {
                let block = &volume[at as usize..at as usize + len];
                erofs::Dirents::parse(block).ok()?.lookup(name.as_bytes()).map(|e| e.nid)
            })
            .with_context(|| format!("{path}: no {name}"))?;
        found = inode(nid)?;
    }
    Ok(found)
}

/// Damages the packed EROFS `volume` as `damage` says.
fn damage_erofs(volume: &mut [u8], damage: &Damage) -> Result<()> {
    let put16 = |volume: &mut [u8], at: usize, v: u16| volume[at..at + 2].copy_from_slice(&v.to_le_bytes());
    match damage.what.as_str() {
        "magic" => volume[erofs::SUPERBLOCK_AT] ^= 0x01,
        "block-past-count" => {
            let (at, inode) = erofs_inode(volume, &damage.path)?;
            ensure!(inode.size() >= BLOCK as u64, "{} has no whole block", damage.path);
            let count = (volume.len() / BLOCK) as u32;
            let start = at + erofs::field::inode::START;
            volume[start..start + 4].copy_from_slice(&count.to_le_bytes());
        }
        "compressed" => {
            let (at, _) = erofs_inode(volume, &damage.path)?;
            // Layout 3, compressed with compact indexes; the inode's size bit kept.
            let at = at + erofs::field::inode::FORMAT;
            let format = u16::from_le_bytes([volume[at], volume[at + 1]]);
            put16(volume, at, (format & 1) | 3 << 1);
        }
        "name-offset" => {
            let (_, dir) = erofs_inode(volume, &damage.path)?;
            let (at, len) = dir.dir_block(0).context("an empty directory")?;
            let block = &volume[at as usize..at as usize + len];
            let count = erofs::Dirents::parse(block).map_err(|_| anyhow::anyhow!("a bad block"))?.len();
            put16(
                volume,
                at as usize + (count - 1) * erofs::DIRENT + erofs::field::dirent::NAME,
                len as u16 + 1,
            );
        }
        what => bail!("no damage {what:?}: magic, block-past-count, compressed or name-offset"),
    }
    Ok(())
}

/// Flips one bit of `file`'s bytes where the verified volume `v` holds them on `disk`, after the
/// pack, so the volume no longer hashes to its root: a 64-byte run of the file from its middle,
/// found exactly once in the volume's data blocks.
pub fn flip_file(disk: &mut [u8], v: &Verified, file: &[u8]) -> Result<usize> {
    let data = v.data(disk);
    for from in (file.len() / 2..file.len().saturating_sub(64)).step_by(97) {
        let run = &file[from..from + 64];
        let mut found = data.windows(64).enumerate().filter(|(_, w)| *w == run).map(|(i, _)| i);
        if let (Some(at), None) = (found.next(), found.next()) {
            let at = v.start + at + 32;
            disk[at] ^= 0x10;
            return Ok(at);
        }
    }
    bail!("{}: no run of the file is on the volume once", v.name)
}

/// Flips one bit of the first level-1 tree block of the verified volume `v` on `disk`: the block
/// that covers the volume's first data blocks, which `littlefsd`'s mount reads.
pub fn flip_tree(disk: &mut [u8], v: &Verified) -> usize {
    let at = v.start + v.geometry.data_blocks() as usize * BLOCK + 7;
    disk[at] ^= 0x10;
    at
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
    /// is the recipe's size; with no stage of its own, only one given.
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
        // A userland disk's recipe may leave its stage out: the bench packs it from its own.
        let bare: Recipe =
            toml::from_str("size_kib = 1024\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\n").unwrap();
        assert_eq!(pack_disk(&bare, Path::new("/"), Some(&dir)).unwrap().len(), 1024 * 1024);
        assert!(pack_disk(&bare, Path::new("/"), None).is_err(), "nothing to pack");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A generated directory is its files in the volume's root, beside the stage's tree: all
    /// empty but the one to read, and none in the stage's place; never beside the userland
    /// disk's objects.
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
        let objects = "[objects]\napplications = []\n";
        assert!(recipe(&format!("stage = \"s\"\n{objects}")).is_ok(), "objects are the stage");
        assert!(recipe(objects).is_ok(), "or the bench stages them, for its own pack");
        assert!(
            recipe(&format!("generated = {{ files = 10, read = \"f9\" }}\n{objects}")).is_err(),
            "nothing generated for the objects"
        );
        assert!(
            recipe(&format!("stage = \"s\"\ngenerated = {{ files = 10, read = \"f9\" }}\n{objects}"))
                .is_err(),
            "nor beside their stage"
        );
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

    /// A verified partition is the largest volume that fits beside its tree, then the tree; two
    /// packs of the same inputs are byte-identical; and the root the pack gives is the one
    /// `redoubt-verity` computes over the volume on the disk, as `verityd` will.
    #[test]
    fn a_verified_partition_is_its_volume_then_its_tree() {
        let dir = stage("verity");
        let mut module = vec![0u8; 10_000];
        noise(&mut module);
        std::fs::write(dir.join("Elixir.Version.beam"), &module).unwrap();
        let recipe: Recipe = toml::from_str(
            "size_kib = 2048\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\nverity = true\n",
        )
        .unwrap();
        let (disk, verified) = pack(&recipe, Path::new("/"), Some(&dir)).unwrap();
        assert_eq!((disk.clone(), verified.clone()), pack(&recipe, Path::new("/"), Some(&dir)).unwrap());
        let [v] = &verified[..] else { panic!("one verified partition") };
        assert_eq!(v.name, "system");
        assert_eq!(v.start, (FIRST_USABLE * SECTOR) as usize);
        let g = v.geometry;
        let partition = (disk.len() - 2 * (FIRST_USABLE * SECTOR) as usize) / BLOCK;
        assert!(
            (g.total_sectors() / SECTORS_PER_BLOCK) as usize <= partition
                && g.data_blocks() > 400
                && g.levels() == 2
        );
        assert!(v.data(&disk)[..BLOCK].windows(8).any(|w| w == b"littlefs"));
        let mut tree = vec![0u8; g.tree_blocks() as usize * BLOCK];
        let root = redoubt_verity::build(&g, v.data(&disk), &mut tree).unwrap();
        assert_eq!(root, v.root);
        let at = v.start + g.data_blocks() as usize * BLOCK;
        assert_eq!(&disk[at..at + tree.len()], &tree[..], "the tree follows the volume");
        assert_eq!(v.root_hex().len(), 64);
        // An unverified pack of the same partition is a littlefs volume the whole partition long.
        let plain: Recipe =
            toml::from_str("size_kib = 2048\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\n").unwrap();
        assert!(pack(&plain, Path::new("/"), Some(&dir)).unwrap().1.is_empty());
        // A case's damage changes the disk and never the root: the volume no longer hashes to it.
        let mut flipped = disk.clone();
        let at_file = flip_file(&mut flipped, v, &module).unwrap();
        assert_eq!(flipped.iter().zip(&disk).filter(|(a, b)| a != b).count(), 1);
        assert!(at_file >= v.start && at_file < at, "in the data blocks");
        let mut again = vec![0u8; tree.len()];
        assert_ne!(redoubt_verity::build(&g, v.data(&flipped), &mut again).unwrap(), v.root);
        let mut flipped = disk.clone();
        assert_eq!(flip_tree(&mut flipped, v), at + 7);
        assert!(
            flip_file(&mut flipped, v, b"not on the volume, nowhere near long enough to be found once")
                .is_err()
        );
        let noisy = "size_kib = 1024\n[[partition]]\nname = \"a\"\nfs = \"noise\"\nverity = true\n";
        let path = std::env::temp_dir().join(format!("testbench-verity-noise-{}.toml", std::process::id()));
        std::fs::write(&path, noisy).unwrap();
        assert!(Recipe::load(&path).is_err(), "noise is never verified");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// An `erofs` partition is the stage as `libs/erofs` packs it, read back whole by the parser
    /// `erofsd` uses, and verified like any other; each damage a case asks for is refused by that
    /// parser exactly where it was made, and the rest of the volume still reads.
    #[test]
    fn an_erofs_partition_is_its_stage_and_each_damage_is_corrupt_where_it_is() {
        let dir = stage("erofs");
        std::fs::write(dir.join("big"), vec![7u8; 5000]).unwrap();
        let recipe: Recipe = toml::from_str(
            "size_kib = 2048\n[[partition]]\nname = \"system\"\nfs = \"erofs\"\nverity = true\n",
        )
        .unwrap();
        let (disk, verified) = pack(&recipe, Path::new("/"), Some(&dir)).unwrap();
        let [v] = &verified[..] else { panic!("one verified partition") };
        let mut read = erofs::read_tree(v.data(&disk)).unwrap();
        let mut staged = tree(&dir).unwrap();
        read.sort();
        staged.sort();
        assert_eq!(read, staged);
        let mut tree = vec![0u8; v.geometry.tree_blocks() as usize * BLOCK];
        assert_eq!(redoubt_verity::build(&v.geometry, v.data(&disk), &mut tree).unwrap(), v.root);

        let packed = pack_erofs(&staged).unwrap();
        let damaged = |what: &str, path: &str| {
            let mut volume = packed.clone();
            damage_erofs(&mut volume, &Damage { what: what.into(), path: path.into() }).unwrap();
            volume
        };
        let volume = damaged("magic", "");
        assert!(erofs::Superblock::parse(&volume, (volume.len() / BLOCK) as u64).is_err());
        for (what, path) in [("block-past-count", "big"), ("compressed", "motd"), ("name-offset", "etc")] {
            let volume = damaged(what, path);
            let broken = if what == "name-offset" { "etc/deep" } else { path };
            assert!(erofs_inode(&volume, broken).is_err(), "{what}");
            assert!(erofs_inode(&volume, "etc").is_ok() && erofs_inode(&volume, "").is_ok(), "{what}");
        }
        let mut volume = packed.clone();
        assert!(
            damage_erofs(&mut volume, &Damage { what: "block-past-count".into(), path: "motd".into() })
                .is_err()
        );
        assert!(damage_erofs(&mut volume, &Damage { what: "nothing".into(), path: String::new() }).is_err());
        let path = std::env::temp_dir().join(format!("testbench-erofs-damage-{}.toml", std::process::id()));
        let littlefs = "size_kib = 1024\n[[partition]]\nname = \"a\"\nfs = \"littlefs\"\nstage = \"x\"\n\
            damage = { what = \"magic\" }\n";
        std::fs::write(&path, littlefs).unwrap();
        assert!(Recipe::load(&path).is_err(), "only an erofs volume is damaged");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
