//! The `unsafe` ratchet: counts uses of the keyword in the trusted computing base and how
//! many of them lack a `// SAFETY:` justification, and fails if either exceeds its budget.
//! Budgets only ever get lowered. Raising one needs a reason in the commit that does it.

use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::case::Budget;

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
        let documented = above
            .iter()
            .any(|l| l.trim_start().starts_with("//") && (l.contains(marker) || l.contains("SAFETY:")))
            || line.contains("SAFETY:");
        if !documented {
            count.undocumented += uses;
        }
    }
    Ok(())
}

/// Count source files separately: a real zero-unsafe crate is not missing coverage.
fn count_path(path: &Path, count: &mut Count) -> Result<usize> {
    let metadata = std::fs::metadata(path).with_context(|| format!("examining {}", path.display()))?;
    if metadata.is_dir() {
        let mut files = 0;
        for entry in std::fs::read_dir(path).with_context(|| format!("listing {}", path.display()))? {
            let entry = entry.with_context(|| format!("reading directory entry in {}", path.display()))?;
            files += count_path(&entry.path(), count)?;
        }
        Ok(files)
    } else if path.extension().is_some_and(|e| e == "rs") {
        ensure!(metadata.is_file(), "not a regular Rust source file: {}", path.display());
        count_file(path, count)?;
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Returns a description of the first budget that is exceeded, if any, and a summary line.
pub fn check(workspace: &Path, budgets: &[Budget]) -> Result<(Option<String>, String)> {
    ensure!(!budgets.is_empty(), "no unsafe budgets configured");
    let mut summary = Vec::new();
    let mut failure = None;
    for budget in budgets {
        ensure!(!budget.paths.is_empty(), "budget {}: no source paths configured", budget.name);
        let mut count = Count::default();
        for path in &budget.paths {
            let root = workspace.join(path);
            let files =
                count_path(&root, &mut count).with_context(|| format!("checking budget {}", budget.name))?;
            ensure!(files != 0, "budget {}: no Rust source files in {}", budget.name, root.display());
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
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
            check(
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
        assert_eq!(check(&fixture.0, &[]).unwrap_err().to_string(), "no unsafe budgets configured");
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
