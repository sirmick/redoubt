//! The repository's own docs: every rule holds, the code's comments included, and the book
//! renders cleanly. The bench runs both (`tests/docs.toml`).

use std::path::PathBuf;
use std::process::Command;

use redoubt_doccheck::{Scope, check};

fn repo() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

#[test]
fn docs_follow_the_rules() {
    let findings = check(&repo(), Scope { pages: None, code: true });
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(findings.is_empty(), "{}", report.join("\n"));
}

#[test]
fn the_book_builds() {
    let out =
        Command::new("mdbook").arg("build").arg("docs").current_dir(repo()).output().expect("mdbook runs");
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{log}");
    // The Mermaid preprocessor names the mdbook version it was built against on every build; that
    // notice is about the plugin, not the book.
    let notice = |l: &str| l.contains("mdbook-mermaid preprocessor was built against version");
    let bad = |l: &str| !notice(l) && ["WARN", "Warning", "ERROR"].iter().any(|w| l.contains(w));
    assert!(!log.lines().any(bad), "{log}");
}
