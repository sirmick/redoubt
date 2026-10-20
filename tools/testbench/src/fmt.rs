//! The `fmt` gate: every Rust source is formatted with the repository's `rustfmt.toml`, under
//! nightly (its options are unstable), and every Elixir source with its project's
//! `.formatter.exs`. Runs `cargo +nightly fmt --all --check` in each cargo workspace root the
//! case names, and fails if a workspace root git tracks is neither named nor skipped with a
//! reason, so a new crate outside the main workspace cannot drift unchecked. Then it lists every
//! `.ex` and `.exs` git tracks or has not ignored and runs `mix format --check-formatted` on them
//! from the deepest Elixir root above each: a file is checked wherever it is put, as the agents'
//! modules were not when only one project's `mix format` ran.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::case::{ElixirFmt, Fmt};

/// Directories never searched for workspace roots: build output and third-party code.
const SKIP_DIRS: &[&str] = &["target", "vendor"];

/// Puts the pinned OTP and Elixir on the path, from the workspace root.
const ENV: &str = "userland/otp/tools/env.sh";

/// Whether nightly rustfmt and the pinned `mix` can run; `Err` says what is missing.
pub fn available(workspace: &Path) -> Result<(), String> {
    match Command::new("cargo").args(["+nightly", "fmt", "--version"]).output() {
        Ok(out) if out.status.success() => {}
        _ => {
            return Err(
                "nightly rustfmt not installed (rustup component add rustfmt --toolchain nightly)".into()
            );
        }
    }
    match mix(workspace, Path::new("."), &["--version"]) {
        Ok(out) if out.status.success() => Ok(()),
        _ => Err(format!("no pinned `mix` on the path {ENV} sets (see GETTING-STARTED.md, \"beamlet\")")),
    }
}

/// `mix args` in `dir`, under the pinned toolchain.
fn mix(workspace: &Path, dir: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("bash")
        .current_dir(workspace.join(dir))
        .args(["-c", &format!(". \"$0/{ENV}\" && exec mix \"$@\""), &workspace.display().to_string()])
        .args(args)
        .output()
}

/// Run the gate: `Ok(None)` if every root is formatted and covered, else what is wrong.
pub fn check(workspace: &Path, gate: &Fmt) -> Result<Option<String>> {
    let mut found = Vec::new();
    found.extend(elixir(workspace, &gate.elixir)?);
    // A skip needs its reason, and one that names no workspace has outlived it.
    for skip in &gate.skip {
        if skip.reason.trim().is_empty() {
            found.push(format!("{}: [skip] without a reason", skip.path));
        }
        if !workspace.join(&skip.path).join("Cargo.toml").is_file() {
            found.push(format!("{}: skipped, but no Cargo.toml is there", skip.path));
        }
    }
    for root in uncovered(workspace, gate)? {
        found.push(format!("{root}: a cargo workspace in neither `roots` nor `skip`"));
    }
    for root in &gate.roots {
        let dir = workspace.join(root);
        if !dir.join("Cargo.toml").is_file() {
            found.push(format!("{root}: no Cargo.toml"));
            continue;
        }
        let out = Command::new("cargo")
            .current_dir(&dir)
            .args(["+nightly", "fmt", "--all", "--check"])
            .output()
            .with_context(|| format!("running cargo fmt in {root}"))?;
        if out.status.success() {
            continue;
        }
        // A `Diff in` line heads each hunk; one line per file is enough to act on.
        let stdout = String::from_utf8_lossy(&out.stdout);
        let files: BTreeSet<_> = stdout
            .lines()
            .filter_map(diff_file)
            .map(|f| Path::new(f).strip_prefix(workspace).unwrap_or(Path::new(f)).display().to_string())
            .collect();
        if files.is_empty() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            found.push(format!("{root}: cargo fmt failed: {}", stderr.trim()));
        }
        found.extend(files.into_iter().map(|f| format!("{f}: not formatted (cargo +nightly fmt in {root})")));
    }
    Ok((!found.is_empty()).then(|| found.join("\n      ")))
}

/// The Elixir side: `mix format --check-formatted` over every file `elixir_files` lists, from
/// its root, and a line for each that is not formatted; the walk's own findings first.
fn elixir(workspace: &Path, gate: &ElixirFmt) -> Result<Vec<String>> {
    let (by_root, mut found) = elixir_files(workspace, gate)?;
    for (root, files) in by_root {
        let dir = Path::new(&root);
        let mut args = vec!["format", "--check-formatted"];
        let rel: Vec<String> = files
            .iter()
            .map(|f| if root == "." { f.clone() } else { f[root.len() + 1..].to_string() })
            .collect();
        args.extend(rel.iter().map(String::as_str));
        let out = mix(workspace, dir, &args).with_context(|| format!("running mix format in {root}"))?;
        if out.status.success() {
            continue;
        }
        // Each unformatted file is named on a line of its own, as an absolute path or as given,
        // in colour, on the error stream; its diff follows. One line per file is enough to act on.
        let stderr = String::from_utf8_lossy(&out.stderr);
        let text = plain(&format!("{}{stderr}", String::from_utf8_lossy(&out.stdout)));
        let unformatted: BTreeSet<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.ends_with(".ex") || l.ends_with(".exs"))
            .map(|l| {
                let path = Path::new(l);
                let path =
                    if path.is_absolute() { path.to_path_buf() } else { workspace.join(dir).join(path) };
                path.strip_prefix(workspace).unwrap_or(&path).display().to_string()
            })
            .collect();
        if unformatted.is_empty() {
            found.push(format!("{root}: mix format failed: {}", stderr.trim()));
        }
        found.extend(unformatted.into_iter().map(|f| format!("{f}: not formatted (mix format in {root})")));
    }
    Ok(found)
}

/// Every `.ex` and `.exs` git tracks or has not ignored, each under the deepest of `gate.roots`
/// above it, and the walk's findings: a skip with no reason or no Elixir under it, a
/// `.formatter.exs` whose directory is no root (its configuration would go unused), a file under
/// no root at all. A skipped file is left out; so is `.worktrees/`, which git ignores.
fn elixir_files(workspace: &Path, gate: &ElixirFmt) -> Result<(BTreeMap<String, Vec<String>>, Vec<String>)> {
    let out = Command::new("git")
        .current_dir(workspace)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"])
        .args(["*.ex", "*.exs", ".formatter.exs", "*/.formatter.exs"])
        .output()
        .context("running git ls-files")?;
    anyhow::ensure!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let under = |file: &str, dir: &str| dir == "." || file == dir || file.starts_with(&format!("{dir}/"));
    let mut found = Vec::new();
    for skip in &gate.skip {
        if skip.reason.trim().is_empty() {
            found.push(format!("{}: [elixir.skip] without a reason", skip.path));
        }
    }
    let mut skipped_hit: BTreeSet<&str> = BTreeSet::new();
    let mut by_root: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in String::from_utf8_lossy(&out.stdout).split('\0').filter(|f| !f.is_empty()) {
        // `--cached` still lists a tracked file deleted in the tree.
        if !workspace.join(file).is_file() {
            continue;
        }
        if let Some(skip) = gate.skip.iter().find(|s| under(file, &s.path)) {
            skipped_hit.insert(&skip.path);
            continue;
        }
        let dir = Path::new(file).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        if file.ends_with("/.formatter.exs") || file == ".formatter.exs" {
            let dir = if dir.is_empty() { ".".to_string() } else { dir };
            if !gate.roots.contains(&dir) {
                found.push(format!("{file}: a .formatter.exs in neither `elixir.roots` nor `elixir.skip`"));
            }
            continue;
        }
        match gate.roots.iter().filter(|r| under(file, r)).max_by_key(|r| if *r == "." { 0 } else { r.len() })
        {
            Some(root) => by_root.entry(root.clone()).or_default().push(file.to_string()),
            None => found.push(format!("{file}: under no root in `elixir.roots`")),
        }
    }
    // Tracked files come before untracked ones: sorted, the same tree lists the same way.
    by_root.values_mut().for_each(|files| files.sort());
    for skip in &gate.skip {
        if !skipped_hit.contains(skip.path.as_str()) {
            found.push(format!("{}: skipped, but no Elixir is there", skip.path));
        }
    }
    for root in &gate.roots {
        if !workspace.join(root).join(".formatter.exs").is_file() {
            found.push(format!("{root}: no .formatter.exs"));
        }
    }
    Ok((by_root, found))
}

/// `text` without its colour codes.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        out.push_str(&rest[..at]);
        rest = &rest[at + 2..];
        rest = rest.find('m').map_or("", |m| &rest[m + 1..]);
    }
    out.push_str(rest);
    out
}

/// The file a rustfmt hunk header names: `Diff in <file>:<line>:` or
/// `Diff in <file> at line <n>:`.
fn diff_file(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("Diff in ")?;
    if let Some((file, _)) = rest.rsplit_once(" at line ") {
        return Some(file);
    }
    rest.rsplitn(3, ':').nth(2)
}

/// Workspace roots (a tracked `Cargo.toml` with a `[workspace]` table) under neither `roots` nor
/// `skip`. Only files git tracks count, so `.worktrees/` checkouts and any untracked nested
/// workspace are never reported.
fn uncovered(workspace: &Path, gate: &Fmt) -> Result<Vec<String>> {
    let out = Command::new("git")
        .current_dir(workspace)
        .args(["ls-files", "-z", "--", "Cargo.toml", "*/Cargo.toml"])
        .output()
        .context("running git ls-files")?;
    anyhow::ensure!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let mut found = Vec::new();
    for manifest in String::from_utf8_lossy(&out.stdout).split('\0').filter(|m| !m.is_empty()) {
        let dir = Path::new(manifest).parent().unwrap_or(Path::new(""));
        let rel =
            if dir.as_os_str().is_empty() { ".".to_string() } else { dir.to_string_lossy().into_owned() };
        let skipped = gate.skip.iter().any(|s| rel == s.path || rel.starts_with(&format!("{}/", s.path)));
        let excluded = dir.components().any(|c| SKIP_DIRS.iter().any(|s| c.as_os_str() == *s));
        if skipped || excluded || gate.roots.contains(&rel) {
            continue;
        }
        let text = std::fs::read_to_string(workspace.join(manifest))
            .with_context(|| format!("reading {manifest}"))?;
        if text.lines().any(|l| l.trim() == "[workspace]") {
            found.push(rel);
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::case::Skip;

    #[test]
    fn diff_headers_of_both_rustfmt_forms_name_the_file() {
        assert_eq!(diff_file("Diff in /w/src/a.rs:12:"), Some("/w/src/a.rs"));
        assert_eq!(diff_file("Diff in /w/src/a.rs at line 12:"), Some("/w/src/a.rs"));
        assert_eq!(diff_file(" fn main() {}"), None);
    }

    #[test]
    fn only_tracked_workspaces_are_reported() {
        let dir: PathBuf = std::env::temp_dir().join(format!("redoubt-fmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ws = "[workspace]\n";
        for (path, text) in [
            ("Cargo.toml", ws),
            ("tracked/Cargo.toml", ws),
            ("covered/Cargo.toml", ws),
            ("member/Cargo.toml", "[package]\n"),
            ("skipped/inner/Cargo.toml", ws),
            (".worktrees/pkg/Cargo.toml", ws),
            ("untracked/Cargo.toml", ws),
        ] {
            std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
            std::fs::write(dir.join(path), text).unwrap();
        }
        let git = |args: &[&str]| {
            assert!(Command::new("git").current_dir(&dir).args(args).status().unwrap().success())
        };
        git(&["init", "-q"]);
        git(&["add", "Cargo.toml", "tracked", "covered", "member", "skipped"]);
        let gate = Fmt {
            roots: vec![".".into(), "covered".into()],
            skip: vec![Skip { path: "skipped".into(), reason: "test".into() }],
            elixir: ElixirFmt { roots: vec![".".into()], skip: Vec::new() },
        };
        let found = uncovered(&dir, &gate);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.unwrap(), ["tracked"]);
    }

    /// Every Elixir file git tracks or has not ignored goes to the deepest root above it, an
    /// untracked one too (it is what a change is about to add); a skipped or ignored one goes
    /// nowhere; a `.formatter.exs` outside the roots, a skip over no Elixir and a root with no
    /// `.formatter.exs` are findings.
    #[test]
    fn every_elixir_file_is_walked_from_its_deepest_root() {
        let dir: PathBuf = std::env::temp_dir().join(format!("redoubt-fmt-ex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in [
            (".formatter.exs", "[inputs: []]\n"),
            (".gitignore", "/ignored/\n"),
            ("a.ex", "a\n"),
            ("lib/b.exs", "b\n"),
            ("sub/.formatter.exs", "[inputs: []]\n"),
            ("sub/lib/c.ex", "c\n"),
            ("sub/lib/agents/new.ex", "new\n"),
            ("gen/g.ex", "g\n"),
            ("stray/.formatter.exs", "[inputs: []]\n"),
            ("stray/s.ex", "s\n"),
            ("ignored/i.ex", "i\n"),
            ("gone.ex", "gone\n"),
        ] {
            std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
            std::fs::write(dir.join(path), text).unwrap();
        }
        let git = |args: &[&str]| {
            assert!(Command::new("git").current_dir(&dir).args(args).status().unwrap().success())
        };
        git(&["init", "-q"]);
        git(&[
            "add",
            ".formatter.exs",
            ".gitignore",
            "a.ex",
            "lib",
            "sub/.formatter.exs",
            "sub/lib/c.ex",
            "gen",
            "stray",
            "gone.ex",
        ]);
        std::fs::remove_file(dir.join("gone.ex")).unwrap();
        let gate = ElixirFmt {
            roots: vec![".".into(), "sub".into(), "noconf".into()],
            skip: vec![
                Skip { path: "gen".into(), reason: "test".into() },
                Skip { path: "empty".into(), reason: "test".into() },
            ],
        };
        let walked = elixir_files(&dir, &gate);
        std::fs::remove_dir_all(&dir).unwrap();
        let (by_root, found) = walked.unwrap();
        assert_eq!(
            by_root,
            BTreeMap::from([
                (".".to_string(), vec!["a.ex".to_string(), "lib/b.exs".into(), "stray/s.ex".into()]),
                ("sub".to_string(), vec!["sub/lib/agents/new.ex".to_string(), "sub/lib/c.ex".into()]),
            ])
        );
        assert_eq!(
            found,
            [
                "stray/.formatter.exs: a .formatter.exs in neither `elixir.roots` nor `elixir.skip`",
                "empty: skipped, but no Elixir is there",
                "noconf: no .formatter.exs",
            ]
        );
    }

    #[test]
    fn colour_codes_are_stripped() {
        assert_eq!(plain("\x1b[1m\x1b[31m/w/a.ex\n\x1b[0m"), "/w/a.ex\n");
    }
}
