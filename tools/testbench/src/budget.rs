//! The `unsafe` ratchet: counts uses of the keyword in the trusted computing base and how
//! many of them lack a `// SAFETY:` justification, and fails if either exceeds its budget.
//! Budgets only ever get lowered. Raising one needs a line `Unsafe budget: <name>: <reason>` in
//! the commit that does it, which the size budget's history check reads (`size::ratchet`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::case::{Budget, Skip};
use crate::size::{self, Limit};

impl Limit for Budget {
    const TABLE: &'static str = "budget";

    fn name(&self) -> &str { &self.name }

    fn paths(&self) -> &[String] { &self.paths }

    fn limits(&self) -> Vec<(&'static str, usize)> {
        vec![("max_unsafe", self.max_unsafe), ("max_undocumented", self.max_undocumented)]
    }
}

#[derive(Default)]
pub struct Count {
    pub total: usize,
    pub undocumented: usize,
}

/// How far above an `unsafe` block a `// SAFETY:` comment may start and still count.
const SAFETY_COMMENT_REACH: usize = 6;
/// How far above an `unsafe fn` / `unsafe impl` its `# Safety` doc section may start.
const SAFETY_DOC_REACH: usize = 16;

fn count_file(path: &Path, count: &mut Count) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    for (number, line) in lines.iter().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let uses =
            code.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|word| *word == "unsafe").count();
        if uses == 0 {
            continue;
        }
        count.total += uses;
        // A block is justified by a `// SAFETY:` comment; a declaration states its contract
        // in a `# Safety` doc section instead.
        let is_declaration =
            ["unsafe fn", "unsafe impl", "unsafe extern", "unsafe trait"].iter().any(|d| code.contains(d));
        let (reach, marker) =
            if is_declaration { (SAFETY_DOC_REACH, "# Safety") } else { (SAFETY_COMMENT_REACH, "SAFETY:") };
        let above = &lines[number.saturating_sub(reach)..number];
        let justifies = |l: &&str| l.contains(marker) || l.contains("SAFETY:");
        // The comment block directly above counts however long it is; otherwise one within reach.
        let block = lines[..number].iter().rev().take_while(|l| l.trim_start().starts_with("//"));
        let documented = block.clone().any(|l| justifies(&l))
            || above.iter().any(|l| l.trim_start().starts_with("//") && justifies(l))
            || line.contains("SAFETY:");
        if !documented {
            count.undocumented += uses;
        }
    }
    Ok(())
}

/// Every Rust source under `path`, a file or a directory. A path that is not there fails, and so
/// does a broken link under it.
pub fn rust_files(path: &Path) -> Result<Vec<PathBuf>> {
    let metadata = std::fs::metadata(path).with_context(|| format!("examining {}", path.display()))?;
    if metadata.is_dir() {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(path).with_context(|| format!("listing {}", path.display()))? {
            let entry = entry.with_context(|| format!("reading directory entry in {}", path.display()))?;
            files.extend(rust_files(&entry.path())?);
        }
        Ok(files)
    } else if path.extension().is_some_and(|e| e == "rs") {
        ensure!(metadata.is_file(), "not a regular Rust source file: {}", path.display());
        Ok(vec![path.to_path_buf()])
    } else {
        Ok(Vec::new())
    }
}

/// Count every budget, then check the budgets in `file` only fell. Returns the first failure, if
/// any, and a summary.
pub fn check(workspace: &Path, file: &str, budgets: &[Budget]) -> Result<(Option<String>, String)> {
    let (failure, summary) = counts(workspace, budgets)?;
    let failure = match failure {
        None => size::ratchet::<Budget>(workspace, file, "Unsafe budget")?,
        failure => failure,
    };
    Ok((failure, summary))
}

/// Returns a description of the first budget that is exceeded, if any, and a summary line.
fn counts(workspace: &Path, budgets: &[Budget]) -> Result<(Option<String>, String)> {
    ensure!(!budgets.is_empty(), "no unsafe budgets configured");
    let mut summary = Vec::new();
    let mut failure = None;
    for budget in budgets {
        ensure!(!budget.paths.is_empty(), "budget {}: no source paths configured", budget.name);
        let mut count = Count::default();
        for path in &budget.paths {
            let root = workspace.join(path);
            // A real zero-unsafe crate is not missing coverage; a path with no source is.
            let files = rust_files(&root).with_context(|| format!("checking budget {}", budget.name))?;
            ensure!(!files.is_empty(), "budget {}: no Rust source files in {}", budget.name, root.display());
            for file in files {
                count_file(&file, &mut count).with_context(|| format!("checking budget {}", budget.name))?;
            }
        }
        let name = &budget.name;
        summary.push(format!("{name}: {} unsafe, {} undocumented", count.total, count.undocumented));
        if count.total > budget.max_unsafe {
            failure.get_or_insert(format!(
                "{name}: {} uses of unsafe, budget is {}",
                count.total, budget.max_unsafe
            ));
        }
        if count.undocumented > budget.max_undocumented {
            failure.get_or_insert(format!(
                "{name}: {} unsafe without a SAFETY comment, budget is {}",
                count.undocumented, budget.max_undocumented
            ));
        }
        if count.total < budget.max_unsafe || count.undocumented < budget.max_undocumented {
            summary.push(format!("  (budget can be lowered to {} / {})", count.total, count.undocumented));
        }
    }
    Ok((failure, summary.join("\n      ")))
}

/// Whether a crate root declares itself `no_std` (outright, or under a `cfg_attr`): the crates
/// that can be built for the target.
fn is_no_std(root: &str) -> bool {
    root.lines().map(str::trim_start).any(|l| {
        l.starts_with("#![")
            && l.contains("no_std")
            && (l.starts_with("#![no_std") || l.starts_with("#![cfg_attr"))
    })
}

/// The coverage the budgets alone cannot prove: every Rust source of every workspace member that
/// can be built for the target (`no_std`) is in some budget, unless the member is `uncounted`;
/// and every `uncounted` entry names such a member. `Some` names what is wrong.
pub fn coverage(workspace: &Path, budgets: &[Budget], uncounted: &[Skip]) -> Result<Option<String>> {
    let manifest = std::fs::read_to_string(workspace.join("Cargo.toml")).context("reading Cargo.toml")?;
    let manifest: toml::Table = toml::from_str(&manifest).context("parsing Cargo.toml")?;
    let members = manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .context("Cargo.toml has no workspace.members")?;
    let counted = |file: &str| {
        budgets.iter().flat_map(|b| &b.paths).any(|p| file == p || file.starts_with(&format!("{p}/")))
    };
    let mut on_target = Vec::new();
    let mut missing = Vec::new();
    for member in members.iter().filter_map(|m| m.as_str()) {
        let roots = ["src/lib.rs", "src/main.rs"].map(|r| workspace.join(member).join(r));
        let no_std = roots.iter().any(|r| std::fs::read_to_string(r).is_ok_and(|text| is_no_std(&text)));
        if !no_std {
            continue;
        }
        on_target.push(member);
        if uncounted.iter().any(|u| u.path == member) {
            continue;
        }
        for file in rust_files(&workspace.join(member).join("src"))? {
            let file = file.strip_prefix(workspace).unwrap_or(&file).to_string_lossy().into_owned();
            if !counted(&file) {
                missing.push(file);
            }
        }
    }
    if let Some(stale) = uncounted.iter().find(|u| !on_target.contains(&u.path.as_str())) {
        return Ok(Some(format!("uncounted {} is not a workspace member built for the target", stale.path)));
    }
    missing.sort();
    Ok((!missing.is_empty()).then(|| format!("on-target sources in no budget: {}", missing.join(", "))))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "redoubt-budget-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn check(&self, paths: &[&str]) -> Result<(Option<String>, String)> {
            counts(
                &self.0,
                &[Budget {
                    name: "fixture".into(),
                    paths: paths.iter().map(|path| (*path).into()).collect(),
                    max_unsafe: 0,
                    max_undocumented: 0,
                }],
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }

    fn budget(paths: &[&str]) -> Budget {
        Budget {
            name: "b".into(),
            paths: paths.iter().map(|p| p.to_string()).collect(),
            max_unsafe: 0,
            max_undocumented: 0,
        }
    }

    /// A source of an on-target crate that no budget counts fails; a host crate, and a member
    /// left uncounted with a reason, do not; an uncounted entry that names no such member fails.
    #[test]
    fn every_on_target_source_is_in_a_budget() {
        let fixture = Fixture::new();
        let w = &fixture.0;
        std::fs::write(w.join("Cargo.toml"), "[workspace]\nmembers = [\"a\", \"host\"]\n").unwrap();
        for dir in ["a/src/nested", "host/src"] {
            std::fs::create_dir_all(w.join(dir)).unwrap();
        }
        std::fs::write(w.join("a/src/lib.rs"), "//! A.\n#![cfg_attr(not(test), no_std)]\n").unwrap();
        std::fs::write(w.join("a/src/nested/x.rs"), "fn x() {}\n").unwrap();
        std::fs::write(w.join("host/src/main.rs"), "fn main() {}\n").unwrap();
        let uncounted = |path: &str| vec![Skip { path: path.into(), reason: "r".into() }];
        let missed = coverage(w, &[budget(&["a/src/lib.rs"])], &[]).unwrap();
        assert_eq!(missed.as_deref(), Some("on-target sources in no budget: a/src/nested/x.rs"));
        assert_eq!(coverage(w, &[budget(&["a/src"])], &[]).unwrap(), None);
        assert_eq!(coverage(w, &[budget(&["a/src/lib.rs"])], &uncounted("a")).unwrap(), None);
        let stale = coverage(w, &[budget(&["a/src"])], &uncounted("host")).unwrap();
        assert!(stale.is_some_and(|s| s.contains("uncounted host")));
    }

    /// A justification is the comment block directly above, however long, or a comment within
    /// reach; one farther up, cut off by code, does not count.
    #[test]
    fn a_long_safety_block_directly_above_justifies() {
        let fixture = Fixture::new();
        let long = format!("// SAFETY: why.\n{}fn f() {{ unsafe {{}} }}\n", "// more.\n".repeat(10));
        std::fs::write(fixture.0.join("long.rs"), long).unwrap();
        let far = format!("// SAFETY: why.\nfn g() {{}}\n{}fn f() {{ unsafe {{}} }}\n", "\n".repeat(10));
        std::fs::write(fixture.0.join("far.rs"), far).unwrap();
        let mut count = Count::default();
        count_file(&fixture.0.join("long.rs"), &mut count).unwrap();
        assert_eq!((count.total, count.undocumented), (1, 0));
        count_file(&fixture.0.join("far.rs"), &mut count).unwrap();
        assert_eq!((count.total, count.undocumented), (2, 1));
    }

    #[test]
    fn missing_paths_fail_regardless_of_extension() {
        let fixture = Fixture::new();
        for path in ["missing", "missing.rs", "missing.txt"] {
            let error = format!("{:#}", fixture.check(&[path]).unwrap_err());
            assert!(error.contains("checking budget fixture"), "{error}");
            assert!(error.contains(&fixture.0.join(path).display().to_string()), "{error}");
        }
    }

    #[test]
    fn empty_configuration_is_not_coverage() {
        let fixture = Fixture::new();
        assert_eq!(counts(&fixture.0, &[]).unwrap_err().to_string(), "no unsafe budgets configured");
        assert_eq!(fixture.check(&[]).unwrap_err().to_string(), "budget fixture: no source paths configured");
    }

    #[test]
    fn every_configured_root_must_contain_rust_source() {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("lib.rs"), "fn safe() {}\n").unwrap();
        std::fs::create_dir(fixture.0.join("empty")).unwrap();
        std::fs::create_dir(fixture.0.join("nonrust")).unwrap();
        std::fs::write(fixture.0.join("nonrust/README.md"), "not source").unwrap();
        for path in ["empty", "nonrust", "nonrust/README.md"] {
            let error = fixture.check(&["lib.rs", path]).unwrap_err().to_string();
            assert!(error.contains("no Rust source files"), "{error}");
            assert!(error.contains(&fixture.0.join(path).display().to_string()), "{error}");
        }
    }

    #[test]
    fn zero_unsafe_source_is_valid_as_a_file_or_nested_directory() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.0.join("src/nested")).unwrap();
        std::fs::create_dir(fixture.0.join("src/empty")).unwrap();
        std::fs::write(fixture.0.join("src/nested/lib.rs"), "fn safe() {}\n").unwrap();
        std::fs::write(fixture.0.join("src/README.md"), "unsafe: not Rust source").unwrap();
        for path in ["src", "src/nested/lib.rs"] {
            let (failure, summary) = fixture.check(&[path]).unwrap();
            assert_eq!(failure, None);
            assert_eq!(summary, "fixture: 0 unsafe, 0 undocumented");
        }
    }

    #[test]
    fn actual_source_counts_still_enforce_the_budget() {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.0.join("src")).unwrap();
        std::fs::write(fixture.0.join("src/lib.rs"), "fn bad() { unsafe {} }\n").unwrap();
        let (failure, summary) = fixture.check(&["src"]).unwrap();
        assert_eq!(failure.as_deref(), Some("fixture: 1 uses of unsafe, budget is 0"));
        assert_eq!(summary, "fixture: 1 unsafe, 1 undocumented");
    }

    #[test]
    fn unreadable_source_reports_its_path() {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("invalid.rs"), [0xff]).unwrap();
        let error = format!("{:#}", fixture.check(&["invalid.rs"]).unwrap_err());
        assert!(error.contains("checking budget fixture"), "{error}");
        assert!(error.contains(&format!("reading {}", fixture.0.join("invalid.rs").display())), "{error}");
    }

    /// A raise of either limit needs its `Unsafe budget:` line; a `Size budget:` line is not one.
    #[test]
    fn a_raise_needs_its_unsafe_budget_line() {
        let repo = size::tests::Scratch::new("unsafe");
        let limits = |unsafe_uses: usize, undocumented: usize| {
            repo.write(&format!(
                "[[budget]]\nname = \"b\"\npaths = [\"b\"]\nmax_unsafe = {unsafe_uses}\nmax_undocumented = {undocumented}\n"
            ))
        };
        let ratchet = || size::ratchet::<Budget>(&repo.0, "b.toml", "Unsafe budget").unwrap();
        limits(2, 0);
        repo.run(&["add", "b.toml"]);
        repo.run(&["commit", "-qm", "first"]);
        repo.run(&["checkout", "-qb", "wp"]);
        limits(2, 1);
        repo.run(&["commit", "-qam", "grow\n\nSize budget: b: it needs it\n"]);
        assert!(ratchet().is_some_and(|w| w.contains("max_undocumented raised from 0 to 1")));
        repo.run(&["commit", "-q", "--amend", "-m", "grow\n\nUnsafe budget: b: it needs it\n"]);
        assert_eq!(ratchet(), None);
        limits(3, 1);
        assert!(ratchet().is_some_and(|w| w.contains("max_unsafe raised from 2 to 3 and not committed")));
    }

    #[cfg(unix)]
    #[test]
    fn broken_nested_symlink_is_not_silently_skipped() {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("lib.rs"), "fn safe() {}\n").unwrap();
        std::os::unix::fs::symlink("absent", fixture.0.join("broken")).unwrap();
        let error = format!("{:#}", fixture.check(&["."]).unwrap_err());
        assert!(error.contains("examining"), "{error}");
        assert!(error.contains("broken"), "{error}");
    }
}
