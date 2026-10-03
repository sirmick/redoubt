//! The userland disk's objects (image/userland.toml; docs/testbench.md, "Disks and network
//! cards"): every module beamlet loads by name after boot, stripped, staged as one file named by
//! the SHA-256 of its bytes, and `system.index`, the table the signed bundle carries to bind the
//! disk to it: one line per object, `<file> <sha256 hex> <bytes>`, sorted byte-wise by file, each
//! LF-terminated, and nothing else. The file is the name the VM asks for, `Elixir.Enum.beam` for a
//! module and `elixir.app` for an application's resource, which is an object too. The same inputs
//! stage the same bytes, index and tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

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
    /// Where `--pack-disk` writes `system.index`, relative to the workspace root: the image's
    /// recipe has one; a case's index goes in its run.
    pub index: Option<PathBuf>,
}

/// Each file the index names and the object it is staged under.
pub type Names = BTreeMap<String, String>;

/// A staged userland disk: its objects' directory, its `system.index`, and what the index names.
#[derive(Clone, Debug)]
pub struct Staged {
    pub objects: PathBuf,
    pub index: PathBuf,
    pub names: Names,
}

/// The name an object is staged under: the lowercase hex of its SHA-256.
pub fn name(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect() }

/// `system.index` for `objects`, each a file name and its bytes: one line each, sorted by name.
pub fn index(objects: &[(String, Vec<u8>)]) -> String {
    let mut lines: Vec<(&str, String)> = objects
        .iter()
        .map(|(key, bytes)| (key.as_str(), format!("{key} {} {}\n", name(bytes), bytes.len())))
        .collect();
    lines.sort();
    lines.into_iter().map(|(_, line)| line).collect()
}

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

/// Writes `objects` into `stage`, emptied first, each under its [`name`], and their index to
/// `index`, and returns what the index names. Two keys with the same bytes are one object.
pub fn write(objects: &[(String, Vec<u8>)], stage: &Path, index_path: &Path) -> Result<Names> {
    if stage.exists() {
        std::fs::remove_dir_all(stage).with_context(|| format!("emptying {}", stage.display()))?;
    }
    std::fs::create_dir_all(stage).with_context(|| format!("creating {}", stage.display()))?;
    for (_, bytes) in objects {
        std::fs::write(stage.join(name(bytes)), bytes)?;
    }
    if let Some(dir) = index_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(index_path, index(objects))
        .with_context(|| format!("writing {}", index_path.display()))?;
    Ok(objects.iter().map(|(file, bytes)| (file.clone(), name(bytes))).collect())
}

/// Builds what `objects` names with the pinned toolchain, strips it, and stages it into `stage`
/// with its index at `index_path` ([`write`]). Returns what the index names and the objects'
/// total bytes.
pub fn stage(workspace: &Path, objects: &Objects, stage: &Path, index_path: &Path) -> Result<(Names, usize)> {
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
    let found = read_objects(&out, &objects.exclude)?;
    ensure!(!found.is_empty(), "no modules staged");
    let names = write(&found, stage, index_path)?;
    Ok((names, found.iter().map(|(_, b)| b.len()).sum()))
}

/// A copy of `stage` for one boot at `to`, with the object of the file `flip` one byte different
/// and that of `remove` absent, each found through `names`.
pub fn damaged(
    stage: &Path,
    names: &Names,
    flip: Option<&str>,
    remove: Option<&str>,
    to: &Path,
) -> Result<()> {
    let object = |file: &str| names.get(file).with_context(|| format!("the index has no file {file}"));
    if to.exists() {
        std::fs::remove_dir_all(to)?;
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(stage)? {
        let entry = entry?;
        std::fs::copy(entry.path(), to.join(entry.file_name()))?;
    }
    if let Some(file) = flip {
        let path = to.join(object(file)?);
        let mut bytes = std::fs::read(&path)?;
        let at = bytes.len() / 2;
        bytes[at] ^= 1;
        std::fs::write(&path, bytes)?;
    }
    if let Some(file) = remove {
        std::fs::remove_file(to.join(object(file)?))?;
    }
    Ok(())
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

    /// The index is one line per object, `<file> <sha256 hex> <bytes>`, sorted by file, each
    /// LF-terminated; each object is staged under its hash; and two packs of the same inputs are
    /// byte-identical, the index and the disk.
    #[test]
    fn the_userland_pack_is_deterministic_and_names_each_object_by_its_hash() {
        let input = modules("in");
        let objects = read_objects(&input, &["application.beam".into()]).unwrap();
        let keys: Vec<&str> = objects.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["Elixir.Enum.beam", "lists.beam", "stdlib.app"]);
        let recipe: crate::disk::Recipe = toml::from_str(
            "size_kib = 1024\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\nstage = \"x\"\n",
        )
        .unwrap();
        let mut packs = Vec::new();
        for n in 0..2 {
            let (stage, index_path) = (dir(&format!("stage{n}")), dir(&format!("index{n}")));
            write(&objects, &stage, &index_path).unwrap();
            for entry in std::fs::read_dir(&stage).unwrap() {
                let entry = entry.unwrap();
                let bytes = std::fs::read(entry.path()).unwrap();
                assert_eq!(entry.file_name().into_string().unwrap(), name(&bytes));
            }
            let disk = crate::disk::pack_disk(&recipe, Path::new("/"), Some(&stage)).unwrap();
            packs.push((std::fs::read_to_string(&index_path).unwrap(), disk));
            std::fs::remove_dir_all(stage).unwrap();
            std::fs::remove_file(index_path).unwrap();
        }
        assert_eq!(packs[0], packs[1]);
        let enum_hash = name(b"FOR1 Enum");
        assert_eq!(
            packs[0].0,
            format!(
                "Elixir.Enum.beam {enum_hash} 9\nlists.beam {} 10\nstdlib.app {} 24\n",
                name(b"FOR1 lists"),
                name(b"{application,stdlib,[]}.")
            )
        );
        assert_eq!(name(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        std::fs::remove_dir_all(input).unwrap();
    }

    /// A case's damage: the flipped object differs from its hash in one byte, the removed one is
    /// absent, and the rest are untouched.
    #[test]
    fn a_case_flips_one_object_and_removes_another() {
        let input = modules("damage");
        let objects = read_objects(&input, &[]).unwrap();
        let (stage, index_path, to) = (dir("dstage"), dir("dindex"), dir("dto"));
        let names = write(&objects, &stage, &index_path).unwrap();
        damaged(&stage, &names, Some("lists.beam"), Some("Elixir.Enum.beam"), &to).unwrap();
        let flipped = std::fs::read(to.join(name(b"FOR1 lists"))).unwrap();
        assert_eq!(flipped.iter().zip(b"FOR1 lists").filter(|(a, b)| a != b).count(), 1);
        assert!(!to.join(name(b"FOR1 Enum")).exists());
        assert_eq!(std::fs::read(to.join(name(b"FOR1 embedded"))).unwrap(), b"FOR1 embedded");
        assert!(damaged(&stage, &names, Some("Elixir.Enum"), None, &to).is_err(), "a prefix is not a file");
        for d in [input, stage, to] {
            std::fs::remove_dir_all(d).unwrap();
        }
        std::fs::remove_file(index_path).unwrap();
    }
}
