//! Disks: a GPT of equal partitions for a case's `[disk]`, and a disk recipe (`image/disk.toml`)
//! packed whole, its partition table by `blkd`'s builder, each littlefs partition by `littlefsd`'s
//! own packer, each EROFS partition by `libs/erofs`'s writer and each walfs partition by `libs/walfs`
//! itself (docs/testbench.md, "Disks and network cards"; image/README.md; docs/servers/walfsd.md, "The
//! packer"). A partition the recipe marks `verity` holds the largest
//! volume that fits beside its hash tree, and the tree after it (docs/servers/verityd.md, "The
//! tree"); its root and block count are what a manifest pins. One the recipe also `sign`s ends in
//! a root block instead, its N, version and root signed under the volume domain with a seed file
//! on the build host ("The root block, and the two modes"): the device never holds the key (R35).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use ed25519_compact::{KeyPair, Seed};
use redoubt_blkd::image::{Entry, FIRST_USABLE, Image};
use redoubt_littlefsd::pack;
use redoubt_verity::{BLOCK, Geometry, Hash, RootBlock, SECTORS_PER_BLOCK, root_block_at};
use serde::Deserialize;

/// A disk sector, in bytes.
pub const SECTOR: u64 = 512;

/// The bytes a GPT takes at the disk's start, as `blkd`'s builder writes it: the protective
/// sector, the header and the entry array, up to the first usable sector.
pub const TABLE_BYTES: usize = (FIRST_USABLE * SECTOR) as usize;

/// A disk recipe: its size and its partitions, in table order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// The whole disk, in KiB (a whole number of sectors).
    pub size_kib: u64,
    /// For the userland disk, the objects its one partition's stage holds, staged before the
    /// pack (`userland.rs`).
    pub objects: Option<crate::userland::Objects>,
    /// The manifest that serves the disk, relative to the workspace root: each of its volumes
    /// that gives `bytes` is held to its partition's size, and the pack is refused if they differ
    /// ([`hold`]).
    pub manifest: Option<PathBuf>,
    pub partition: Vec<Partition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    /// The volume's name, as the manifest's `volumes` entry names it.
    pub name: String,
    /// What the partition holds: `littlefs`, a writable volume; `erofs`, a read-only one; `walfs`,
    /// a writable volume in Redoubt's own format; or, for a case, `noise`: the same pseudo-random
    /// bytes every time, which no filesystem mounts.
    pub fs: String,
    /// For a volume (all but `noise`), the directory whose tree the volume holds, relative to the
    /// workspace root.
    /// A userland disk's (a recipe with `objects`) is where `--pack-disk` stages them, and is left
    /// out where only the bench packs it, from its own staging.
    pub stage: Option<PathBuf>,
    /// For a volume, files made for the pack in the volume's root, beside the
    /// stage's tree.
    pub generated: Option<Generated>,
    /// For a volume, a verified volume: the volume is followed by its hash tree, and
    /// the pack says its root and block count, which the manifest pins.
    #[serde(default)]
    pub verity: bool,
    /// For `erofs` or `walfs`, for a case: one thing the packed volume is made to hold that its
    /// server must serve as corrupt ([`Damage`]).
    pub damage: Option<Damage>,
    /// For a verified volume, a signed root block in place of a pinned root.
    pub sign: Option<Sign>,
}

/// What an EROFS volume is damaged with after its pack, for `erofs-corrupt`
/// (servers/erofsd.md, "The format"): `magic`, a bit of the superblock's magic flipped;
/// `block-past-count`, the file at `path` (with at least one whole block) starting at the volume's
/// block count; `compressed`, the file at `path` laid out as compressed; `name-offset`, the last
/// name of the first block of the directory at `path` starting past the block's end. And what a
/// walfs volume is damaged with, for `walfsd-flipped-block` (servers/walfsd.md, "What is corrupt"):
/// `flip`, one bit flipped in the first data block of the file at `path`, found by its bytes, which
/// must fill the block and be on the volume once.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Damage {
    pub what: String,
    #[serde(default)]
    pub path: String,
}

/// A signed volume's root block, as a recipe asks for it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sign {
    /// The 32-byte Ed25519 seed file it is signed with, relative to the workspace root.
    pub key: PathBuf,
    /// The version it carries, which `verityd` holds to the manifest's floor.
    pub version: u64,
}

/// A verified partition as packed: the root and data blocks its manifest entry pins, or its root
/// block carries, and where its volume and tree lie on the disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    /// The volume's name, as the manifest's `volumes` entry names it.
    pub name: String,
    pub root: Hash,
    pub geometry: Geometry,
    /// The partition's first byte on the disk.
    pub start: usize,
    /// A signed volume's root block: its version, and its first byte on the disk.
    pub signed: Option<(u64, usize)>,
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
            "{}: objects are one volume's stage, with nothing generated",
            path.display()
        );
        for p in &recipe.partition {
            ensure!(
                p.damage.is_none() || p.fs == "erofs" || p.fs == "walfs",
                "{}: partition {}: only an erofs or walfs volume is damaged",
                path.display(),
                p.name
            );
            match p.fs.as_str() {
                "littlefs" | "erofs" | "walfs" => {
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
                    ensure!(
                        p.verity || p.sign.is_none(),
                        "{}: partition {}: only a verified volume is signed",
                        path.display(),
                        p.name
                    );
                }
                "noise" => ensure!(
                    p.stage.is_none() && p.generated.is_none() && !p.verity && p.sign.is_none(),
                    "{}: partition {}: noise is neither staged nor generated",
                    path.display(),
                    p.name
                ),
                fs => {
                    bail!(
                        "{}: partition {}: fs {fs:?} is not littlefs, erofs, walfs or noise",
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

/// Each partition's size in bytes, in table order, on a disk of `size_kib` KiB holding `count`
/// equal partitions.
pub fn partition_bytes(size_kib: u64, count: u64) -> Vec<u64> {
    shares(size_kib * 1024 / SECTOR, count).iter().map(|e| (e.last_lba - e.first_lba + 1) * SECTOR).collect()
}

/// Every volume of `manifest` that gives `bytes`, a volume home quotas are carved from
/// (servers/init.md, "Home quotas"), holds exactly its partition's size on the disk
/// whose partitions are `sizes` (and, where `names` is given, is the partition of its name):
/// `init` refuses quotas past `bytes`, so `bytes` may not promise more than the partition holds,
/// nor drift from it.
pub fn hold(manifest: &serde_json::Value, sizes: &[u64], names: Option<&[&str]>) -> Result<()> {
    for v in manifest["volumes"].as_array().into_iter().flatten() {
        let Some(bytes) = v.get("bytes") else { continue };
        let name = v["name"].as_str().unwrap_or("?");
        let bytes: u64 =
            bytes.as_str().and_then(|b| b.parse().ok()).with_context(|| format!("volume {name}: bytes"))?;
        let i = v["partition"].as_u64().with_context(|| format!("volume {name}: partition"))? as usize;
        let size = *sizes.get(i).with_context(|| format!("volume {name}: no partition {i} on the disk"))?;
        if let Some(names) = names {
            ensure!(names.get(i) == Some(&name), "volume {name}: partition {i} is {:?}", names.get(i));
        }
        ensure!(bytes == size, "volume {name}: bytes {bytes}, but partition {i} holds {size}");
    }
    Ok(())
}

/// The manifest `recipe` names, read under `root`, held to the recipe's partitions ([`hold`]):
/// by every pack, and by a kept disk before it is attached as it is.
pub fn hold_recipe(recipe: &Recipe, root: &Path) -> Result<()> {
    let Some(path) = &recipe.manifest else { return Ok(()) };
    let text = std::fs::read(root.join(path)).with_context(|| format!("reading {}", path.display()))?;
    let manifest =
        serde_json::from_slice(&text).with_context(|| format!("{} is not JSON", path.display()))?;
    let names: Vec<&str> = recipe.partition.iter().map(|p| p.name.as_str()).collect();
    hold(&manifest, &partition_bytes(recipe.size_kib, names.len() as u64), Some(&names))
        .with_context(|| format!("{} against the recipe's partitions", path.display()))
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

/// The disk `recipe` describes, its stages read under `root`, and each verified partition as
/// packed, in table order; `stage`, if given, stands in for every volume's own.
pub fn pack(recipe: &Recipe, root: &Path, stage: Option<&Path>) -> Result<(Vec<u8>, Vec<Verified>)> {
    ensure!(recipe.size_kib > 0, "a disk of no size");
    let sectors = recipe.size_kib * 1024 / SECTOR;
    let parts = shares(sectors, recipe.partition.len() as u64);
    hold_recipe(recipe, root)?;
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
        // A verified volume is the largest whose data and tree fit the partition, before its root
        // block if it is signed: the partition's last whole block.
        let room = match &p.sign {
            Some(_) => root_block_at(sectors),
            None => Some(sectors / SECTORS_PER_BLOCK),
        };
        let geometry = match p.verity {
            true => Some(
                room.and_then(Geometry::largest)
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
            "walfs" => {
                let mut volume = pack_walfs(data / SECTORS_PER_BLOCK, &staged)
                    .with_context(|| format!("packing {}", p.name))?;
                if let Some(damage) = &p.damage {
                    damage_walfs(&mut volume, damage, &staged)
                        .with_context(|| format!("damaging {}", p.name))?;
                }
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
            let hash = redoubt_verity::build(&geometry, &volume, &mut tree)
                .map_err(|_| anyhow::anyhow!("{}: the volume is not its geometry's size", p.name))?;
            let at = start + volume.len();
            disk[at..at + tree.len()].copy_from_slice(&tree);
            let signed = match (&p.sign, room) {
                (Some(sign), Some(blocks)) => {
                    let at = start + blocks as usize * BLOCK;
                    disk[at..at + BLOCK].copy_from_slice(&root_block(
                        &root.join(&sign.key),
                        sign.version,
                        geometry,
                        hash,
                    )?);
                    Some((sign.version, at))
                }
                _ => None,
            };
            verified.push(Verified { name: p.name.clone(), root: hash, geometry, start, signed });
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

/// A walfs volume in memory, for the packer: whole blocks, and a `sync` that has nothing to do.
struct Ram(Vec<u8>);

impl walfs::BlockDevice for Ram {
    fn block_count(&self) -> u32 { (self.0.len() / BLOCK) as u32 }

    fn read(&mut self, block: u32, buf: &mut walfs::Block) -> Result<(), walfs::Error> {
        let at = block as usize * BLOCK;
        buf.copy_from_slice(self.0.get(at..at + BLOCK).ok_or(walfs::Error::Io)?);
        Ok(())
    }

    fn write(&mut self, block: u32, data: &walfs::Block) -> Result<(), walfs::Error> {
        let at = block as usize * BLOCK;
        self.0.get_mut(at..at + BLOCK).ok_or(walfs::Error::Io)?.copy_from_slice(data);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), walfs::Error> { Ok(()) }
}

/// `staged` as a walfs volume of `blocks` blocks, written by `libs/walfs` itself: formatted with
/// an inode for every 16 blocks (or for every entry, if more), then each directory made and each
/// file created and written, in the stage's order, every mtime 0.
fn pack_walfs(blocks: u64, staged: &[(String, Option<Vec<u8>>)]) -> Result<Vec<u8>> {
    let blocks = u32::try_from(blocks).context("a volume past walfs's 2^32 blocks")?;
    let mut geometry = walfs::Geometry::for_blocks(blocks);
    geometry.inode_count = geometry.inode_count.max((staged.len() as u32 + 2).next_multiple_of(32));
    let fail = |path: &str, e: walfs::Error| anyhow::anyhow!("{path}: {e}");
    let mut ram = Ram(vec![0; blocks as usize * BLOCK]);
    walfs::Filesystem::format(&mut ram, geometry).map_err(|e| fail("format", e))?;
    let mut fs = walfs::Filesystem::mount(&mut ram).map_err(|e| fail("mount", e))?;
    for (path, data) in staged {
        let at = format!("/{path}");
        match data {
            None => fs.mkdir(&at).map_err(|e| fail(path, e))?,
            Some(data) => {
                let o = walfs::OpenOptions { write: true, create: true, ..Default::default() };
                let h = fs.open(&at, o).map_err(|e| fail(path, e))?;
                let wrote = fs.write(h, data);
                fs.close(h).map_err(|e| fail(path, e))?;
                match wrote {
                    Ok(n) if n == data.len() => {}
                    Ok(_) | Err(walfs::Error::NoSpace) => bail!("{path}: the volume is full"),
                    Err(e) => return Err(fail(path, e)),
                }
            }
        }
    }
    drop(fs);
    Ok(ram.0)
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

/// `damage` done to the walfs volume packed from `staged`: `flip`, a bit of the first data block
/// of the file at `damage.path`, which walfs then refuses against its hash slot.
fn damage_walfs(volume: &mut [u8], damage: &Damage, staged: &[(String, Option<Vec<u8>>)]) -> Result<()> {
    ensure!(damage.what == "flip", "no damage {:?}: flip", damage.what);
    let data = staged
        .iter()
        .find_map(|(path, data)| (*path == damage.path).then_some(data.as_deref()).flatten())
        .with_context(|| format!("no file {}", damage.path))?;
    let first = data.get(..BLOCK).with_context(|| format!("{} has no whole block", damage.path))?;
    let mut at = volume.chunks(BLOCK).enumerate().filter(|(_, b)| *b == first).map(|(i, _)| i * BLOCK);
    let (Some(block), None) = (at.next(), at.next()) else {
        bail!("{}'s first block is not on the volume once", damage.path)
    };
    volume[block + 17] ^= 0x04;
    Ok(())
}

/// The root block of a volume of `geometry` at `version` and `root`, signed with the 32-byte seed
/// in the file `key`. Ed25519 is deterministic, so the same inputs sign the same block.
fn root_block(key: &Path, version: u64, geometry: Geometry, root: Hash) -> Result<[u8; BLOCK]> {
    let seed = std::fs::read(key).with_context(|| format!("reading {}", key.display()))?;
    let seed: [u8; 32] =
        seed.try_into().map_err(|_| anyhow::anyhow!("{}: a seed is 32 bytes", key.display()))?;
    let mut block = RootBlock { geometry, version, root, signature: [0; 64] };
    block.signature = *KeyPair::from_seed(Seed::new(seed)).sk.sign(block.signed(), None);
    Ok(block.encode())
}

/// Flips one bit of the version in the signed volume `v`'s root block on `disk`, after the
/// signing: the signature no longer verifies.
pub fn flip_version(disk: &mut [u8], v: &Verified) -> Result<()> {
    let (_, at) = v.signed.with_context(|| format!("{} is not signed", v.name))?;
    let block = &mut disk[at..at + BLOCK];
    let mut changed = RootBlock::parse(block).map_err(|_| anyhow::anyhow!("{}: no root block", v.name))?;
    changed.version ^= 1;
    block.copy_from_slice(&changed.encode());
    Ok(())
}

/// Flips one bit of `file`'s bytes wherever the verified volume `v` holds them on `disk`, after
/// the pack, so the volume no longer hashes to its root: a 64-byte run of the file from its
/// middle, found exactly `copies` times in the volume's data blocks (twice for a file the boot
/// pack holds as well, once for any other), flipped in each. Returns where the first flip is.
pub fn flip_file(disk: &mut [u8], v: &Verified, file: &[u8], copies: usize) -> Result<usize> {
    let data = v.data(disk);
    for from in (file.len() / 2..file.len().saturating_sub(64)).step_by(97) {
        let run = &file[from..from + 64];
        let found: Vec<usize> =
            data.windows(64).enumerate().filter(|(_, w)| *w == run).map(|(i, _)| i).collect();
        if found.len() == copies {
            for at in &found {
                disk[v.start + at + 32] ^= 0x10;
            }
            return Ok(v.start + found[0] + 32);
        }
    }
    bail!("{}: no run of the file is on the volume exactly {copies} times", v.name)
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

    /// The disk alone.
    fn pack_disk(recipe: &Recipe, root: &Path, stage: Option<&Path>) -> Result<Vec<u8>> {
        pack(recipe, root, stage).map(|(disk, _)| disk)
    }

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

    /// A volume that gives `bytes` is its partition's size, by index (and by name, for a recipe):
    /// one byte off, a partition the disk lacks, or another partition's name is refused; a volume
    /// without `bytes` is not held.
    #[test]
    fn a_volume_s_bytes_are_held_to_its_partition() {
        let sizes = partition_bytes(1024, 2);
        let manifest = |bytes: u64, partition: u64| {
            serde_json::json!({ "volumes": [
                { "name": "data", "partition": partition, "bytes": bytes.to_string() },
                { "name": "other", "partition": 7 },
            ] })
        };
        assert!(hold(&manifest(sizes[0], 0), &sizes, Some(&["data", "vault"])).is_ok());
        assert!(hold(&manifest(sizes[1], 1), &sizes, None).is_ok());
        assert!(hold(&manifest(sizes[0] + 1, 0), &sizes, None).is_err());
        assert!(hold(&manifest(sizes[0] - 1, 0), &sizes, None).is_err());
        assert!(hold(&manifest(sizes[0], 2), &sizes, None).is_err());
        assert!(hold(&manifest(sizes[1], 1), &sizes, Some(&["data", "vault"])).is_err());
    }

    /// The image's manifest gives its writable volumes their partitions' sizes on the image's
    /// disk, and a recipe that names a manifest whose `bytes` differ is not packed.
    #[test]
    fn the_image_s_volumes_are_its_disk_s_partitions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let recipe = Recipe::load(&root.join("image/disk.toml")).unwrap();
        assert_eq!(recipe.manifest.as_deref(), Some(Path::new("image/manifest.json")));
        let image: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("image/manifest.json")).unwrap()).unwrap();
        let names: Vec<&str> = recipe.partition.iter().map(|p| p.name.as_str()).collect();
        let sizes = partition_bytes(recipe.size_kib, names.len() as u64);
        hold(&image, &sizes, Some(&names)).unwrap();
        let dir = stage("held");
        let wrong = serde_json::json!({ "volumes": [
            { "name": "data", "partition": 0, "bytes": (partition_bytes(1024, 1)[0] + 4096).to_string() },
        ] });
        std::fs::write(dir.join("manifest.json"), wrong.to_string()).unwrap();
        let text = format!(
            "size_kib = 1024\nmanifest = \"{}\"\n[[partition]]\nname = \"data\"\nfs = \"littlefs\"\n",
            dir.join("manifest.json").display()
        );
        let recipe: Recipe = toml::from_str(&text).unwrap();
        assert!(pack_disk(&recipe, Path::new("/"), Some(&dir)).is_err(), "bytes past the partition");
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
        let at_file = flip_file(&mut flipped, v, &module, 1).unwrap();
        assert_eq!(flipped.iter().zip(&disk).filter(|(a, b)| a != b).count(), 1);
        assert!(at_file >= v.start && at_file < at, "in the data blocks");
        let mut again = vec![0u8; tree.len()];
        assert_ne!(redoubt_verity::build(&g, v.data(&flipped), &mut again).unwrap(), v.root);
        // A file the volume holds twice, as the boot pack holds a module beside its own file,
        // is flipped in both copies, and is not found once.
        std::fs::write(dir.join("boot.pack"), &module).unwrap();
        let (twice, verified) = pack(&recipe, Path::new("/"), Some(&dir)).unwrap();
        let mut flipped = twice.clone();
        assert!(flip_file(&mut flipped, &verified[0], &module, 1).is_err());
        flip_file(&mut flipped, &verified[0], &module, 2).unwrap();
        assert_eq!(flipped.iter().zip(&twice).filter(|(a, b)| a != b).count(), 2);
        std::fs::remove_file(dir.join("boot.pack")).unwrap();
        let mut flipped = disk.clone();
        assert_eq!(flip_tree(&mut flipped, v), at + 7);
        assert!(
            flip_file(&mut flipped, v, b"not on the volume, nowhere near long enough to be found once", 1)
                .is_err()
        );
        let noisy = "size_kib = 1024\n[[partition]]\nname = \"a\"\nfs = \"noise\"\nverity = true\n";
        let path = std::env::temp_dir().join(format!("testbench-verity-noise-{}.toml", std::process::id()));
        std::fs::write(&path, noisy).unwrap();
        assert!(Recipe::load(&path).is_err(), "noise is never verified");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A `walfs` partition is the stage as `libs/walfs` itself writes it, read back whole and sound
    /// by the same crate; two packs of one stage are the same bytes; a flipped file is refused
    /// where it is read; a stage that does not fit is refused.
    #[test]
    fn a_walfs_partition_is_its_stage_and_two_packs_are_the_same_bytes() {
        let dir = stage("walfs");
        let mut big = vec![0u8; 200_000];
        noise(&mut big);
        std::fs::write(dir.join("big"), &big).unwrap();
        std::fs::write(dir.join("etc/empty"), b"").unwrap();
        let recipe: Recipe = toml::from_str(
            "size_kib = 2048\n[[partition]]\nname = \"home\"\nfs = \"walfs\"\nstage = \"x\"\n",
        )
        .unwrap();
        let disk = pack_disk(&recipe, Path::new("/"), Some(&dir)).unwrap();
        assert_eq!(disk, pack_disk(&recipe, Path::new("/"), Some(&dir)).unwrap());
        let at = (FIRST_USABLE * SECTOR) as usize;
        let sectors = (disk.len() - 2 * at) / SECTOR as usize;
        let mut ram = Ram(disk[at..at + sectors / SECTORS_PER_BLOCK as usize * BLOCK].to_vec());
        let mut fs = walfs::Filesystem::mount(&mut ram).unwrap();
        assert!(fs.check().unwrap().is_empty());
        for (path, data) in tree(&dir).unwrap() {
            let at = format!("/{path}");
            let m = fs.stat(&at).unwrap();
            assert_eq!(m.mtime, 0, "{path}");
            let Some(data) = data else {
                assert_eq!(m.kind, walfs::FileType::Dir, "{path}");
                continue;
            };
            let h = fs.open(&at, walfs::OpenOptions { read: true, ..Default::default() }).unwrap();
            let mut read = vec![0u8; data.len() + 1];
            let mut got = 0;
            while let Ok(n @ 1..) = fs.read(h, &mut read[got..]) {
                got += n;
            }
            assert_eq!(&read[..got], &data[..], "{path}");
        }
        let mut names = 0;
        fs.read_dir("/", |_| names += 1).unwrap();
        assert_eq!(names, 3, "big, etc and motd");
        drop(fs);

        // `flip` damages the file's first data block alone: walfs refuses that file's read as
        // corrupt, and still reads the rest.
        let staged = tree(&dir).unwrap();
        let mut volume = pack_walfs(400, &staged).unwrap();
        damage_walfs(&mut volume, &Damage { what: "flip".into(), path: "big".into() }, &staged).unwrap();
        let mut ram = Ram(volume);
        let mut fs = walfs::Filesystem::mount(&mut ram).unwrap();
        let h = fs.open("/big", walfs::OpenOptions { read: true, ..Default::default() }).unwrap();
        assert_eq!(fs.read(h, &mut [0u8; 100]), Err(walfs::Error::Corrupt));
        let h = fs.open("/motd", walfs::OpenOptions { read: true, ..Default::default() }).unwrap();
        assert!(fs.read(h, &mut [0u8; 100]).is_ok());
        drop(fs);
        let mut volume = ram.0;
        for (what, path) in [("flip", "motd"), ("flip", "nothing"), ("magic", "big")] {
            let damage = Damage { what: what.into(), path: path.into() };
            assert!(damage_walfs(&mut volume, &damage, &staged).is_err(), "{what} {path}");
        }
        std::fs::write(dir.join("huge"), vec![1u8; 3 << 20]).unwrap();
        assert!(pack_disk(&recipe, Path::new("/"), Some(&dir)).is_err(), "3 MiB does not fit 2 MiB");
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
        // Only a littlefs volume is never damaged.
        let littlefs = "size_kib = 1024\n[[partition]]\nname = \"a\"\nfs = \"littlefs\"\nstage = \"x\"\n\
            damage = { what = \"magic\" }\n";
        std::fs::write(&path, littlefs).unwrap();
        assert!(Recipe::load(&path).is_err(), "only an erofs or walfs volume is damaged");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A signed partition is its volume, its tree, and its root block in its last whole block,
    /// signed with the seed file so it verifies under the bundle's key (the seed is the bundle
    /// builder's): byte-identical in two packs, Ed25519 being deterministic. A flipped version
    /// no longer verifies; a seed of the wrong length and a signed volume that is not verified are
    /// refused.
    #[test]
    fn a_signed_partition_ends_in_its_root_block_signed_deterministically() {
        use ed25519_compact::{PublicKey, Signature};
        let dir = stage("signed");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let seed = root.join("tests/data/verity/dev-seed");
        let text = |sign: &str| {
            format!(
                "size_kib = 2048\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\nverity = true\n{sign}"
            )
        };
        let sign = format!("sign = {{ key = {:?}, version = 3 }}\n", seed.display().to_string());
        let recipe: Recipe = toml::from_str(&text(&sign)).unwrap();
        let (disk, verified) = pack(&recipe, &root, Some(&dir)).unwrap();
        assert_eq!((disk.clone(), verified.clone()), pack(&recipe, &root, Some(&dir)).unwrap());
        let [v] = &verified[..] else { panic!("one verified partition") };
        let (version, at) = v.signed.unwrap();
        let end = disk.len() - (FIRST_USABLE * SECTOR) as usize;
        assert_eq!((version, at), (3, (end - v.start) / BLOCK * BLOCK + v.start - BLOCK));
        assert!(
            v.start + (v.geometry.total_sectors() / SECTORS_PER_BLOCK) as usize * BLOCK <= at,
            "before it"
        );
        let block = RootBlock::parse(&disk[at..at + BLOCK]).unwrap();
        assert_eq!((block.geometry, block.version, block.root), (v.geometry, 3, v.root));
        let verify = |b: &RootBlock| {
            PublicKey::new(redoubt_signing::DEV_PUBLIC_KEY).verify(b.signed(), &Signature::new(b.signature))
        };
        assert!(verify(&block).is_ok());
        let mut flipped = disk.clone();
        flip_version(&mut flipped, v).unwrap();
        assert_eq!(flipped.iter().zip(&disk).filter(|(a, b)| a != b).count(), 1);
        let changed = RootBlock::parse(&flipped[at..at + BLOCK]).unwrap();
        assert_eq!(changed.version, 2);
        assert!(verify(&changed).is_err());
        // Without `sign`, the same partition has no root block: the volume fills it.
        let (_, plain) = pack(&toml::from_str(&text("")).unwrap(), &root, Some(&dir)).unwrap();
        assert!(plain[0].signed.is_none() && plain[0].geometry.data_blocks() >= v.geometry.data_blocks());
        assert!(flip_version(&mut flipped, &plain[0]).is_err());
        let short = dir.join("short-seed");
        std::fs::write(&short, [0x42; 31]).unwrap();
        let sign = format!("sign = {{ key = {:?}, version = 3 }}\n", short.display().to_string());
        assert!(pack(&toml::from_str(&text(&sign)).unwrap(), &root, Some(&dir)).is_err(), "31 bytes");
        let path = std::env::temp_dir().join(format!("testbench-signed-{}.toml", std::process::id()));
        std::fs::write(&path, text(&sign).replace("verity = true\n", "stage = \"s\"\n")).unwrap();
        assert!(Recipe::load(&path).is_err(), "only a verified volume is signed");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
