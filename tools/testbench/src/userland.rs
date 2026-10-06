//! The userland disk's objects (image/userland.toml; docs/testbench.md, "Disks and network
//! cards"): every module beamlet loads by name after boot, stripped, staged as one plain file
//! under the name the VM asks for, `Elixir.Enum.beam` for a module and `elixir.app` for an
//! application's resource. The volume they are packed into is verified as a whole
//! (docs/servers/verityd.md), so no object carries a check of its own. The same inputs stage the
//! same bytes and tree.
//!
//! Beside them, if the recipe names its entries, the boot pack, `boot.pack`: those objects' bytes
//! again, in one file behind an index, which beamlet reads whole at start in place of a lookup
//! each (docs/userland/beamlet.md, "beamlet on Redoubt"; its format is beamlet's,
//! userland/otp/redoubt/src/pack.rs). The same objects and names give the same pack, byte for
//! byte: its entries are sorted by name and nothing in it depends on when or where it was built.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::disk::Verified;

/// What the userland disk holds: a recipe's `[objects]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objects {
    /// The applications whose modules the disk holds, each whole, by the modules its resource
    /// lists: OTP's and Elixir's from the pinned toolchain, and those the `mix` projects build.
    #[serde(default)]
    pub applications: Vec<String>,
    /// Single modules besides, by name, from the same code path: a test's few.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Erlang sources, relative to the workspace root, compiled by the pinned `erlc`
    /// (`+deterministic`): a test's own modules.
    #[serde(default)]
    pub erlang: Vec<PathBuf>,
    /// Mix projects, relative to the workspace root, compiled first: their applications join the
    /// code path the others are found on.
    #[serde(default)]
    pub mix: Vec<PathBuf>,
    /// Files left out (`application.beam`): the modules the VM embeds, which it loads before it
    /// can read the disk.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Whether each module keeps its `Docs` chunk.
    #[serde(default)]
    pub docs: bool,
    /// The boot pack's entries, by file name (`lists.beam`, `kernel.app`), each one of the
    /// objects; none, no pack.
    #[serde(default)]
    pub pack: Vec<String>,
    /// Test-only: the pack written with this fault, for beamlet to refuse (the `pack-bad-*`
    /// cases).
    #[serde(default)]
    pub pack_fault: Option<PackFault>,
}

/// A fault a test writes into the boot pack.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PackFault {
    /// The pack ends one byte early, inside its last entry.
    Truncated,
    /// The first entry's length is one more than its bytes.
    WrongLength,
    /// The first entry holds the second's module, under its own name.
    WrongName,
}

/// The boot pack's file at the root of the userland volume.
pub const PACK: &str = "boot.pack";
/// The pack's first four bytes and its format's version (userland/otp/redoubt/src/pack.rs).
const PACK_MAGIC: &[u8; 4] = b"RBPK";
const PACK_VERSION: u32 = 1;

/// A userland disk staged and packed once in a run: its objects' directory, the disk packed from
/// them, and its verified volumes' roots, which the bundle's manifest pins.
#[derive(Clone, Debug)]
pub struct Staged {
    pub objects: PathBuf,
    pub image: PathBuf,
    pub verified: Vec<Verified>,
}

/// The lowercase hex of `bytes`' SHA-256: a name the bench gives what it stages.
pub fn name(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect() }

/// The objects in `dir`, the helper's output, each `MODULE.beam` or `APP.app` by its file name,
/// the `exclude`d left out, in name order.
fn read_objects(dir: &Path, exclude: &[String]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut objects = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let file = entry.file_name().into_string().map_err(|f| anyhow::anyhow!("{f:?} is not UTF-8"))?;
        ensure!(file.ends_with(".beam") || file.ends_with(".app"), "{file}: neither a module nor a resource");
        if !exclude.contains(&file) {
            objects.push((file, std::fs::read(entry.path())?));
        }
    }
    objects.sort();
    Ok(objects)
}

/// Writes `objects` into `stage`, emptied first, each under its own file name.
pub fn write(objects: &[(String, Vec<u8>)], stage: &Path) -> Result<()> {
    if stage.exists() {
        std::fs::remove_dir_all(stage).with_context(|| format!("emptying {}", stage.display()))?;
    }
    std::fs::create_dir_all(stage).with_context(|| format!("creating {}", stage.display()))?;
    for (file, bytes) in objects {
        std::fs::write(stage.join(file), bytes)?;
    }
    Ok(())
}

/// A pack of `entries`, in their order: the magic, the version, the count, the index (each name's
/// length, the name, its bytes' offset and length) and the entries' bytes back to back, every
/// integer little-endian.
fn pack_bytes(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let index: usize = entries.iter().map(|(name, _)| 10 + name.len()).sum();
    let mut out = PACK_MAGIC.to_vec();
    out.extend(PACK_VERSION.to_le_bytes());
    out.extend(u32::try_from(entries.len())?.to_le_bytes());
    let mut offset = 12 + index;
    for (name, bytes) in entries {
        out.extend(u16::try_from(name.len())?.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(u32::try_from(offset)?.to_le_bytes());
        out.extend(u32::try_from(bytes.len())?.to_le_bytes());
        offset += bytes.len();
    }
    for (_, bytes) in entries {
        out.extend(*bytes);
    }
    u32::try_from(out.len()).context("a boot pack of 4 GiB or more")?;
    Ok(out)
}

/// The boot pack of `names` among `objects`, sorted by name, with `fault` if a test asks for one:
/// a name that is not one of the objects, or is named twice, is refused.
pub fn boot_pack(
    objects: &[(String, Vec<u8>)],
    names: &[String],
    fault: Option<PackFault>,
) -> Result<Vec<u8>> {
    let mut names: Vec<&str> = names.iter().map(String::as_str).collect();
    names.sort_unstable();
    let mut entries = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        ensure!(i == 0 || names[i - 1] != *name, "{name}: named twice in the boot pack");
        let (_, bytes) = objects
            .iter()
            .find(|(file, _)| file == name)
            .with_context(|| format!("{name}: in the boot pack but not among the objects"))?;
        entries.push((*name, bytes.as_slice()));
    }
    if fault == Some(PackFault::WrongName) {
        ensure!(entries.len() >= 2, "a wrong-name fault needs two entries");
        entries[0].1 = entries[1].1;
    }
    let mut pack = pack_bytes(&entries)?;
    match fault {
        Some(PackFault::Truncated) => {
            pack.pop();
        }
        Some(PackFault::WrongLength) => {
            ensure!(!entries.is_empty(), "a wrong-length fault needs an entry");
            // The first entry's length: after the header, its name's length and the name, and
            // its offset.
            let at = 12 + 2 + entries[0].0.len() + 4;
            let length = u32::from_le_bytes(pack[at..at + 4].try_into()?) + 1;
            pack[at..at + 4].copy_from_slice(&length.to_le_bytes());
        }
        Some(PackFault::WrongName) | None => {}
    }
    Ok(pack)
}

/// Each entry of `pack`, by name, with its bytes: the index read back.
pub fn pack_entries(pack: &[u8]) -> Result<Vec<(String, &[u8])>> {
    ensure!(pack.len() >= 12 && &pack[..4] == PACK_MAGIC, "not a boot pack");
    let word = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(pack.get(at..at + 4).context("a truncated index")?.try_into()?) as usize)
    };
    let mut at = 12;
    let mut entries = Vec::new();
    for _ in 0..word(8)? {
        let len = u16::from_le_bytes(pack.get(at..at + 2).context("a truncated index")?.try_into()?) as usize;
        let name = std::str::from_utf8(pack.get(at + 2..at + 2 + len).context("a truncated index")?)?;
        at += 2 + len;
        let (offset, length) = (word(at)?, word(at + 4)?);
        at += 8;
        entries.push((name.to_string(), pack.get(offset..offset + length).context("an entry past the end")?));
    }
    Ok(entries)
}

/// Builds what `objects` names with the pinned toolchain, strips it, and stages it into `stage`
/// ([`write`]), with the boot pack of the entries it names ([`boot_pack`]). Returns the objects'
/// count and total bytes, the pack's not among them.
pub fn stage(workspace: &Path, objects: &Objects, stage: &Path) -> Result<(usize, usize)> {
    let mut command: Vec<String> = vec!["elixir".into()];
    for project in &objects.mix {
        // A build of its own, apart from the development one `./shell` runs.
        let dir = workspace.join(project);
        crate::build::erlang(
            workspace,
            &[
                "env",
                "MIX_ENV=prod",
                "sh",
                "-c",
                "cd \"$1\" && exec mix compile",
                "_",
                &dir.to_string_lossy(),
            ],
        )?;
        let lib = dir.join("_build/prod/lib");
        let mut apps: Vec<_> = std::fs::read_dir(&lib)
            .with_context(|| format!("reading {}", lib.display()))?
            .collect::<std::io::Result<_>>()?;
        apps.sort_by_key(|e| e.file_name());
        for app in apps {
            command.extend(["-pa".into(), app.path().join("ebin").to_string_lossy().into_owned()]);
        }
    }
    let (out, compiled) = (stage.with_extension("modules"), stage.with_extension("erlang"));
    for dir in [&out, &compiled] {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        std::fs::create_dir_all(dir)?;
    }
    let mut modules = objects.modules.clone();
    for source in &objects.erlang {
        let module = source.file_stem().context("an Erlang source with no name")?.to_string_lossy();
        let (to, source) = (compiled.to_string_lossy(), workspace.join(source));
        crate::build::erlang(workspace, &["erlc", "+deterministic", "-o", &to, &source.to_string_lossy()])?;
        modules.push(module.into_owned());
    }
    if !objects.erlang.is_empty() {
        command.extend(["-pa".into(), compiled.to_string_lossy().into_owned()]);
    }
    let script = workspace.join("tools/testbench/src/userland.exs");
    command.extend([script.to_string_lossy().into_owned(), out.to_string_lossy().into_owned()]);
    command.push(if objects.docs { "docs" } else { "nodocs" }.into());
    command.extend(objects.applications.iter().map(|app| format!("app:{app}")));
    command.extend(modules.iter().map(|module| format!("module:{module}")));
    let command: Vec<&str> = command.iter().map(String::as_str).collect();
    crate::build::erlang(workspace, &command)?;
    let mut found = read_objects(&out, &objects.exclude)?;
    ensure!(!found.is_empty(), "no modules staged");
    let counted = (found.len(), found.iter().map(|(_, b)| b.len()).sum());
    if !objects.pack.is_empty() {
        let pack = boot_pack(&found, &objects.pack, objects.pack_fault)?;
        found.push((PACK.to_string(), pack));
    }
    write(&found, stage)?;
    // Each entry of a sound pack is the file staged beside it, byte for byte.
    if !objects.pack.is_empty() && objects.pack_fault.is_none() {
        for (name, bytes) in pack_entries(&std::fs::read(stage.join(PACK))?)? {
            ensure!(
                std::fs::read(stage.join(&name))? == bytes,
                "{name}: the boot pack's copy differs from its file"
            );
        }
    }
    Ok(counted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("testbench-userland-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Modules as the helper writes them, with an application resource and one excluded module.
    fn modules(name: &str) -> PathBuf {
        let dir = dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lists.beam"), b"FOR1 lists").unwrap();
        std::fs::write(dir.join("Elixir.Enum.beam"), b"FOR1 Enum").unwrap();
        std::fs::write(dir.join("stdlib.app"), b"{application,stdlib,[]}.").unwrap();
        std::fs::write(dir.join("application.beam"), b"FOR1 embedded").unwrap();
        dir
    }

    /// Each object is staged as a plain file under its own name, the excluded left out, and two
    /// packs of the same inputs are byte-identical.
    #[test]
    fn the_userland_pack_is_deterministic_and_stages_each_object_by_name() {
        let input = modules("in");
        let objects = read_objects(&input, &["application.beam".into()]).unwrap();
        let keys: Vec<&str> = objects.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["Elixir.Enum.beam", "lists.beam", "stdlib.app"]);
        let recipe: crate::disk::Recipe = toml::from_str(
            "size_kib = 1024\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\nstage = \"x\"\nverity = true\n",
        )
        .unwrap();
        let mut packs = Vec::new();
        for n in 0..2 {
            let stage = dir(&format!("stage{n}"));
            write(&objects, &stage).unwrap();
            let mut staged: Vec<String> = std::fs::read_dir(&stage)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            staged.sort();
            assert_eq!(staged, keys);
            assert_eq!(std::fs::read(stage.join("lists.beam")).unwrap(), b"FOR1 lists");
            packs.push(crate::disk::pack(&recipe, Path::new("/"), Some(&stage)).unwrap());
            std::fs::remove_dir_all(stage).unwrap();
        }
        assert_eq!(packs[0], packs[1]);
        assert_eq!(packs[0].1.len(), 1, "the volume is verified");
        assert_eq!(name(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        std::fs::remove_dir_all(input).unwrap();
    }

    /// The boot pack of the same objects and names is the same bytes, whatever order the names
    /// come in: sorted by name, each entry the object's bytes, after an index of exactly its
    /// size. A name not among the objects, or named twice, is refused.
    #[test]
    fn the_boot_pack_is_deterministic_sorted_and_only_of_the_objects() {
        let input = modules("pack");
        let objects = read_objects(&input, &["application.beam".into()]).unwrap();
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let pack =
            boot_pack(&objects, &names(&["lists.beam", "stdlib.app", "Elixir.Enum.beam"]), None).unwrap();
        let again =
            boot_pack(&objects, &names(&["Elixir.Enum.beam", "lists.beam", "stdlib.app"]), None).unwrap();
        assert_eq!(pack, again);
        let entries = pack_entries(&pack).unwrap();
        let read: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (n.as_str(), *b)).collect();
        assert_eq!(
            read,
            [
                ("Elixir.Enum.beam", &b"FOR1 Enum"[..]),
                ("lists.beam", b"FOR1 lists"),
                ("stdlib.app", b"{application,stdlib,[]}.")
            ]
        );
        let index =
            12 + ["Elixir.Enum.beam", "lists.beam", "stdlib.app"].iter().map(|n| 10 + n.len()).sum::<usize>();
        assert_eq!(&pack[..12], b"RBPK\x01\0\0\0\x03\0\0\0");
        assert_eq!(pack.len(), index + 9 + 10 + 24);
        assert!(boot_pack(&objects, &names(&["application.beam"]), None).is_err(), "an excluded object");
        assert!(boot_pack(&objects, &names(&["lists.beam", "lists.beam"]), None).is_err(), "a name twice");

        // A test's faults: one byte short; the first length one more; the first entry the
        // second's bytes.
        let two = names(&["Elixir.Enum.beam", "lists.beam"]);
        let sound = boot_pack(&objects, &two, None).unwrap();
        assert_eq!(boot_pack(&objects, &two, Some(PackFault::Truncated)).unwrap(), sound[..sound.len() - 1]);
        let long = boot_pack(&objects, &two, Some(PackFault::WrongLength)).unwrap();
        assert_eq!(long.len(), sound.len());
        assert_eq!(pack_entries(&long).unwrap()[0].1.len(), b"FOR1 Enum".len() + 1);
        let wrong = boot_pack(&objects, &two, Some(PackFault::WrongName)).unwrap();
        let entries = pack_entries(&wrong).unwrap();
        assert_eq!((entries[0].0.as_str(), entries[0].1), ("Elixir.Enum.beam", &b"FOR1 lists"[..]));
        std::fs::remove_dir_all(input).unwrap();
    }
}
