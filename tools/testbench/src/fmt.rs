//! The `fmt` gate: every Rust source is formatted with the repository's `rustfmt.toml`, under
//! nightly (its options are unstable). Runs `cargo +nightly fmt --all --check` in each cargo
//! workspace root the case names, and fails if a workspace root git tracks is neither named nor
//! skipped with a reason, so a new crate outside the main workspace cannot drift unchecked.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::case::Fmt;

/// Directories never searched for workspace roots: build output and third-party code.
const SKIP_DIRS: &[&str] = &["target", "vendor"];

/// Whether nightly rustfmt can run; `Err` says what is missing.
pub fn available() -> Result<(), String> {
    match Command::new("cargo").args(["+nightly", "fmt", "--version"]).output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => Err("nightly rustfmt not installed (rustup component add rustfmt --toolchain nightly)".into()),
    }
}

/// Run the gate: `Ok(None)` if every root is formatted and covered, else what is wrong.
pub fn check(workspace: &Path, gate: &Fmt) -> Result<Option<String>> {
    let mut found = Vec::new();
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
        };
        let found = uncovered(&dir, &gate);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.unwrap(), ["tracked"]);
    }
}
