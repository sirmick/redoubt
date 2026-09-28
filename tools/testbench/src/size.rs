//! The size budget: each trusted crate's lines of Rust against a ceiling that only falls
//! (tenet 1: the trusted computing base is budgeted, not observed).
//!
//! A line counts when it holds code: blank lines, `//` comments (doc comments included) and lines
//! wholly inside a `/* */` comment do not. Every `.rs` file under a crate's paths counts, its
//! in-file tests included, the same way every time.
//!
//! Raising a ceiling needs a reason in the commit that does it. The case reads every commit that
//! changed the case file, merges included; where one raised a ceiling over the file in its first
//! parent, dropped a crate (a rename drops the old name) or narrowed a crate's paths, a line
//! `Size budget: <crate>: <reason>` must name each such crate, in its message or, for a merge, in
//! a commit it brings in. A raise not yet committed fails outright.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::case::{SizeBudget, SizeCrate};

/// Lines of code in one file's text.
fn count_text(text: &str) -> usize {
    let mut in_block = false;
    let mut lines = 0;
    for line in text.lines() {
        let mut rest = line.trim();
        let mut code = false;
        while !rest.is_empty() {
            if in_block {
                match rest.find("*/") {
                    Some(at) => {
                        in_block = false;
                        rest = rest[at + 2..].trim_start();
                    }
                    None => break,
                }
            } else if rest.starts_with("//") {
                break;
            } else if let Some(after) = rest.strip_prefix("/*") {
                in_block = true;
                rest = after;
            } else {
                code = true;
                break;
            }
        }
        lines += usize::from(code);
    }
    lines
}

fn count_path(path: &Path) -> Result<usize> {
    let files = crate::budget::rust_files(path)?;
    ensure!(!files.is_empty(), "no Rust source files in {}", path.display());
    let mut lines = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        lines += count_text(&text);
    }
    Ok(lines)
}

#[derive(Deserialize)]
struct File {
    #[serde(rename = "crate")]
    crates: Vec<SizeCrate>,
}

/// The crates in one version of the case file.
fn ceilings(text: &str) -> Result<Vec<SizeCrate>> {
    let file: File = toml::from_str(text).context("parsing the size budget")?;
    Ok(file.crates)
}

/// The crates `now` raises over `before`, drops, or counts fewer paths of, each with what changed.
fn raised(before: &[SizeCrate], now: &[SizeCrate]) -> Vec<(String, String)> {
    before
        .iter()
        .filter_map(|old| {
            let name = old.name.clone();
            let Some(new) = now.iter().find(|c| c.name == old.name) else {
                return Some((name, format!("dropped (its ceiling was {})", old.max_lines)));
            };
            if new.max_lines > old.max_lines {
                return Some((name, format!("ceiling raised from {} to {}", old.max_lines, new.max_lines)));
            }
            let gone: Vec<_> =
                old.paths.iter().filter(|p| !new.paths.contains(p)).map(String::as_str).collect();
            (!gone.is_empty())
                .then(|| (name, format!("paths narrowed (no longer counts {})", gone.join(", "))))
        })
        .collect()
}

/// The reasons a commit message gives, `Size budget: <crate>: <reason>`, by crate.
fn reasons(message: &str) -> Vec<&str> {
    message
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Size budget: "))
        .filter_map(|l| l.split_once(": ").filter(|(_, why)| !why.trim().is_empty()).map(|(name, _)| name))
        .collect()
}

fn git(workspace: &Path, args: &[&str]) -> Result<Option<String>> {
    let out = Command::new("git").current_dir(workspace).args(args).output().context("running git")?;
    Ok(out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// Whether the ceilings only fell, or each raise carries its reason. `Some` says what is wrong.
fn ratchet(workspace: &Path, file: &str, now_text: &str) -> Result<Option<String>> {
    let committed = git(workspace, &["show", &format!("HEAD:{file}")])?;
    if let Some(committed) = committed.filter(|c| c != now_text) {
        return Ok(raised(&ceilings(&committed)?, &ceilings(now_text)?).into_iter().next().map(
            |(name, what)| {
                format!("{name}: {what} and not committed; commit it with `Size budget: {name}: <reason>`")
            },
        ));
    }
    let Some(commits) = git(workspace, &["log", "--format=%H", "--", file])? else {
        bail!("git log failed for {file}");
    };
    for commit in commits.lines() {
        let Some(parent) = git(workspace, &["show", &format!("{commit}^:{file}")])? else {
            continue;
        };
        let Some(text) = git(workspace, &["show", &format!("{commit}:{file}")])? else {
            continue;
        };
        // A merge is judged against its first parent, with the reasons of every commit it brings in.
        let message =
            git(workspace, &["log", "--format=%B", &format!("{commit}^..{commit}")])?.unwrap_or_default();
        let given = reasons(&message);
        let unexplained = raised(&ceilings(&parent)?, &ceilings(&text)?)
            .into_iter()
            .find(|(name, _)| !given.contains(&name.as_str()));
        if let Some((name, what)) = unexplained {
            return Ok(Some(format!(
                "{name}: {what} in {commit} without a `Size budget: {name}: <reason>` line"
            )));
        }
    }
    Ok(None)
}

/// Count every crate against its ceiling, then check the ceilings only fell. Returns the first
/// failure, if any, and a summary.
pub fn check(workspace: &Path, file: &str, budget: &SizeBudget) -> Result<(Option<String>, String)> {
    ensure!(!budget.crates.is_empty(), "no crates in the size budget");
    let mut summary = Vec::new();
    let mut failure = None;
    for c in &budget.crates {
        ensure!(!c.paths.is_empty(), "size budget {}: no paths", c.name);
        let mut lines = 0;
        for path in &c.paths {
            lines += count_path(&workspace.join(path)).with_context(|| format!("size budget {}", c.name))?;
        }
        summary.push(format!("{}: {lines} of {} lines", c.name, c.max_lines));
        if lines > c.max_lines {
            failure.get_or_insert(format!("{}: {lines} lines, ceiling is {}", c.name, c.max_lines));
        }
    }
    if failure.is_none() {
        let text =
            std::fs::read_to_string(workspace.join(file)).with_context(|| format!("reading {file}"))?;
        failure = ratchet(workspace, file, &text)?;
    }
    Ok((failure, summary.join("\n      ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_code_lines_count() {
        let text = "//! Doc.\n\n/// Item doc.\nfn f() {} // trailing\n/* one\n   two */\n/* a */ let x = 1;\n  // indented\n";
        assert_eq!(count_text(text), 2);
    }

    fn crates(list: &[(&str, &[&str], usize)]) -> Vec<SizeCrate> {
        list.iter()
            .map(|(name, paths, max)| SizeCrate {
                name: name.to_string(),
                paths: paths.iter().map(|p| p.to_string()).collect(),
                max_lines: *max,
            })
            .collect()
    }

    /// A raise, a dropped crate and a narrowed path set each need a reason; a fall, a new crate
    /// and a widened path set do not.
    #[test]
    fn a_raise_needs_its_reason() {
        let before = crates(&[("kernel", &["k"], 100), ("loader", &["l"], 50)]);
        let now = crates(&[("kernel", &["k"], 120), ("loader", &["l", "l2"], 40), ("new", &["n"], 9)]);
        let one = |name: &str, what: &str| vec![(name.to_string(), what.to_string())];
        assert_eq!(raised(&before, &now), one("kernel", "ceiling raised from 100 to 120"));
        let renamed = crates(&[("kernel", &["k"], 100), ("boot", &["l"], 50)]);
        assert_eq!(raised(&before, &renamed), one("loader", "dropped (its ceiling was 50)"));
        let narrowed = crates(&[("kernel", &["k/src"], 100), ("loader", &["l"], 50)]);
        assert_eq!(raised(&before, &narrowed), one("kernel", "paths narrowed (no longer counts k)"));
        assert_eq!(
            reasons("x\nSize budget: kernel: the timer wheel\nSize budget: loader:\n"),
            vec!["kernel"]
        );
    }

    /// A throwaway git repository holding one case file, `b.toml`.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("redoubt-size-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let scratch = Self(dir);
            scratch.run(&["init", "-q", "-b", "main"]);
            scratch.run(&["config", "user.email", "t@t"]);
            scratch.run(&["config", "user.name", "t"]);
            scratch
        }

        fn run(&self, args: &[&str]) {
            assert!(
                Command::new("git").current_dir(&self.0).args(args).status().unwrap().success(),
                "{args:?}"
            )
        }

        fn write(&self, max: usize) -> String {
            let text = format!("[[crate]]\nname = \"k\"\npaths = [\"k\"]\nmax_lines = {max}\n");
            std::fs::write(self.0.join("b.toml"), &text).unwrap();
            text
        }

        fn ratchet(&self, text: &str) -> Option<String> { ratchet(&self.0, "b.toml", text).unwrap() }
    }

    impl Drop for Scratch {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }

    /// In a scratch repository: a committed raise without its reason fails, with it passes, an
    /// uncommitted raise fails, and a raise without its reason fails behind a later commit too.
    #[test]
    fn the_ratchet_reads_the_commit_that_raised() {
        let repo = Scratch::new("commits");
        let text = repo.write(10);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        assert_eq!(repo.ratchet(&text), None);
        let text = repo.write(12);
        assert!(repo.ratchet(&text).is_some_and(|w| w.contains("not committed")));
        repo.run(&["commit", "-qam", "grow"]);
        assert!(repo.ratchet(&text).is_some_and(|w| w.contains("without")));
        repo.run(&["commit", "-q", "--amend", "-m", "grow\n\nSize budget: k: it needs it\n"]);
        assert_eq!(repo.ratchet(&text), None);
        let text = repo.write(8);
        assert_eq!(repo.ratchet(&text), None);
        repo.run(&["commit", "-qam", "shrink"]);
        repo.write(9);
        repo.run(&["commit", "-qam", "grow again"]);
        let text = repo.write(7);
        repo.run(&["commit", "-qam", "shrink again"]);
        assert!(repo.ratchet(&text).is_some_and(|w| w.contains("from 8 to 9")));
    }

    /// A merge that brings in a raise with its reason passes; a raise made in the merge itself
    /// needs the reason in the merge's message.
    #[test]
    fn a_merge_is_judged_against_its_first_parent() {
        let repo = Scratch::new("merges");
        repo.write(10);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.run(&["checkout", "-qb", "side"]);
        let text = repo.write(12);
        repo.run(&["commit", "-qam", "grow\n\nSize budget: k: it needs it\n"]);
        repo.run(&["checkout", "-q", "main"]);
        repo.run(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
        assert_eq!(repo.ratchet(&text), None);
        repo.run(&["checkout", "-qb", "other", "HEAD~1"]);
        std::fs::write(repo.0.join("x"), "x").unwrap();
        repo.run(&["add", "x"]);
        repo.run(&["commit", "-qm", "unrelated"]);
        repo.run(&["checkout", "-q", "main"]);
        repo.run(&["merge", "-q", "--no-ff", "--no-commit", "other"]);
        let text = repo.write(14);
        repo.run(&["commit", "-qam", "merge other"]);
        assert!(repo.ratchet(&text).is_some_and(|w| w.contains("from 12 to 14")));
    }
}
