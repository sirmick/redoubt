//! The `no-cruft` gate: the source must carry no leftover of an interface the tree has dropped,
//! no silenced dead code, no Cargo feature nothing reads, and one literal definition of each
//! shared constant. Not a boot; reads the sources. Every exemption is a case entry with a reason.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;

use crate::case::{Allow, NoCruft};

/// What source files look like. Everything else under a path (build output, binaries) is skipped.
const TEXT: &[&str] = &["rs", "toml", "x", "ld", "S"];
/// Directories never searched: build output and third-party code.
const SKIP_DIRS: &[&str] = &["target", "vendor", ".git"];

/// One broken rule: `rule` names it, as the case's `allow` entries do.
struct Finding {
    rule: String,
    file: String,
    line: usize,
    text: String,
}

fn files(workspace: &Path, paths: &[String]) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                if !SKIP_DIRS.iter().any(|s| path.file_name().is_some_and(|n| n == *s)) {
                    walk(&path, out)?;
                }
            } else if path.extension().is_some_and(|e| TEXT.iter().any(|t| e == *t)) {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for p in paths {
        let path = workspace.join(p);
        if path.is_dir() { walk(&path, &mut out)? } else { out.push(path) }
    }
    out.sort();
    Ok(out)
}

fn relative(workspace: &Path, path: &Path) -> String {
    path.strip_prefix(workspace).unwrap_or(path).to_string_lossy().into_owned()
}

/// Run the gate: `Ok(None)` if nothing is found, else the findings, one per line.
pub fn check(workspace: &Path, gate: &NoCruft) -> Result<Option<String>> {
    let mut found = Vec::new();
    let sources = files(workspace, &gate.paths)?;

    // Leftover names, each a pattern no line may match (unless it also matches `unless`).
    let rules = gate
        .forbidden
        .iter()
        .map(|f| Ok((f, Regex::new(&f.pattern)?, f.unless.as_deref().map(Regex::new).transpose()?)))
        .collect::<Result<Vec<_>>>()?;
    let allow_dead = Regex::new(r"allow\((dead_code|unused)")?;
    let dead_scope: Vec<PathBuf> = gate.no_allow_dead.iter().map(|p| workspace.join(p)).collect();
    for path in &sources {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file = relative(workspace, path);
        let is_rust = path.extension().is_some_and(|e| e == "rs");
        let dead_checked = is_rust && dead_scope.iter().any(|d| path.starts_with(d));
        for (number, line) in text.lines().enumerate() {
            for (rule, pattern, unless) in &rules {
                if pattern.is_match(line) && !unless.as_ref().is_some_and(|u| u.is_match(line)) {
                    found.push(Finding {
                        rule: rule.pattern.clone(),
                        file: file.clone(),
                        line: number + 1,
                        text: line.into(),
                    });
                }
            }
            if dead_checked && allow_dead.is_match(line) {
                found.push(Finding {
                    rule: "allow-dead".into(),
                    file: file.clone(),
                    line: number + 1,
                    text: line.into(),
                });
            }
        }
    }

    features(workspace, &sources, &mut found)?;
    one_definition(workspace, gate, &sources, &mut found)?;

    let mut report: Vec<String> = found
        .iter()
        .filter(|f| !allowed(gate, f))
        .map(|f| format!("{}:{}: [{}] {}", f.file, f.line, f.rule, f.text.trim()))
        .collect();
    // An exemption needs its reason, and one that covers nothing has outlived it.
    for a in &gate.allow {
        if a.reason.trim().is_empty() {
            report.push(format!("{}: [allow] an exemption without a reason", a.path));
        }
        if !found.iter().any(|f| covers(a, f)) {
            report.push(format!("{}: [allow] exempts `{}` but nothing there matches it", a.path, a.rule));
        }
    }
    Ok((!report.is_empty()).then(|| report.join("\n      ")))
}

/// Whether one of the case's `allow` entries covers `f`: its path is under the entry's, and the
/// entry names its rule (or `*`).
fn allowed(gate: &NoCruft, f: &Finding) -> bool { gate.allow.iter().any(|a| covers(a, f)) }

fn covers(a: &Allow, f: &Finding) -> bool {
    (a.rule == "*" || a.rule == f.rule) && f.file.starts_with(&a.path)
}

/// A Cargo feature nothing reads: no `feature = "name"` in its crate's sources. A feature that
/// only turns on other features of the same crate (a board alias such as `qemu-virt`) is exempt.
fn features(workspace: &Path, sources: &[PathBuf], found: &mut Vec<Finding>) -> Result<()> {
    for manifest in sources.iter().filter(|p| p.file_name().is_some_and(|n| n == "Cargo.toml")) {
        let text = std::fs::read_to_string(manifest)?;
        let parsed: toml::Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", manifest.display()))?;
        let Some(table) = parsed.get("features").and_then(|f| f.as_table()) else { continue };
        let crate_dir = manifest.parent().expect("a manifest is in a directory");
        let crate_sources: Vec<String> = sources
            .iter()
            .filter(|p| p.starts_with(crate_dir) && p.extension().is_some_and(|e| e == "rs"))
            .map(|p| std::fs::read_to_string(p))
            .collect::<Result<_, _>>()?;
        let names: BTreeSet<&str> = table.keys().map(String::as_str).collect();
        for (name, enables) in table {
            if name == "default" {
                continue;
            }
            let enables: Vec<&str> =
                enables.as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect();
            if !enables.is_empty() && enables.iter().all(|e| names.contains(e)) {
                continue;
            }
            let used = Regex::new(&format!(r#"feature\s*=\s*"{}""#, regex::escape(name)))?;
            if !crate_sources.iter().any(|s| used.is_match(s)) {
                let line =
                    text.lines().position(|l| l.trim_start().starts_with(name.as_str())).map_or(0, |n| n + 1);
                found.push(Finding {
                    rule: "unused-feature".into(),
                    file: relative(workspace, manifest),
                    line,
                    text: format!("feature `{name}` has no cfg(feature) user"),
                });
            }
        }
    }
    Ok(())
}

/// More than one file with a literal-valued definition of one of `gate.one_definition` (a
/// `target_pointer_width` pair in one file counts once), or any `const PAGE` alias.
fn one_definition(
    workspace: &Path,
    gate: &NoCruft,
    sources: &[PathBuf],
    found: &mut Vec<Finding>,
) -> Result<()> {
    let mut all = sources.to_vec();
    all.extend(files(workspace, &gate.definition_paths)?);
    let alias = Regex::new(r"\bconst\s+PAGE\s*:")?;
    for name in &gate.one_definition {
        let literal = Regex::new(&format!(r"\b(const|static)\s+{}\s*:[^=]*=\s*[0-9]", regex::escape(name)))?;
        let mut defining = Vec::new();
        for path in all.iter().filter(|p| p.extension().is_some_and(|e| e == "rs")) {
            let text = std::fs::read_to_string(path)?;
            if let Some(number) = text.lines().position(|l| literal.is_match(l)) {
                defining.push((path, number + 1));
            }
        }
        // An allowed second definition (the model's, which is independent on purpose) is not
        // counted, so the one left is not reported for it.
        let rule = format!("one-definition:{name}");
        let (exempt, counted): (Vec<Finding>, Vec<Finding>) = defining
            .into_iter()
            .map(|(path, line)| Finding {
                rule: rule.clone(),
                file: relative(workspace, path),
                line,
                text: format!("a literal definition of `{name}`"),
            })
            .partition(|f| allowed(gate, f));
        found.extend(exempt);
        if counted.len() > 1 {
            found.extend(counted);
        }
    }
    for path in all.iter().filter(|p| p.extension().is_some_and(|e| e == "rs")) {
        let text = std::fs::read_to_string(path)?;
        for (number, line) in text.lines().enumerate().filter(|(_, l)| alias.is_match(l)) {
            found.push(Finding {
                rule: "page-alias".into(),
                file: relative(workspace, path),
                line: number + 1,
                text: line.into(),
            });
        }
    }
    Ok(())
}
