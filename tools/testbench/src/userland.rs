//! The userland disk's objects (image/userland.toml; docs/testbench.md, "Disks and network
//! cards"): every module beamlet loads by name after boot, stripped, staged as one plain file
//! under the name the VM asks for, `Elixir.Enum.beam` for a module and `elixir.app` for an
//! application's resource. The volume they are packed into is verified as a whole
//! (docs/servers/verityd.md), so no object carries a check of its own. The same inputs stage the
//! same bytes and tree.

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
}

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

/// Builds what `objects` names with the pinned toolchain, strips it, and stages it into `stage`
/// ([`write`]). Returns the objects' count and total bytes.
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
    let found = read_objects(&out, &objects.exclude)?;
    ensure!(!found.is_empty(), "no modules staged");
    write(&found, stage)?;
    Ok((found.len(), found.iter().map(|(_, b)| b.len()).sum()))
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
}
