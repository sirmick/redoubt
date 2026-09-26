//! The repository's own docs: every rule holds, and the book renders cleanly.
//!
//! Both run only with `DOCCHECK_REPO=1`: the switch-over to the new docs turns them on (with the
//! bench case that runs this crate's tests).

use std::path::PathBuf;
use std::process::Command;

use redoubt_doccheck::{Scope, check};

fn repo() -> Option<PathBuf> {
    let on = std::env::var("DOCCHECK_REPO").is_ok_and(|v| v == "1");
    on.then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

#[test]
fn docs_follow_the_rules() {
    let Some(root) = repo() else { return };
    let findings = check(&root, Scope { pages: None, code: true });
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(findings.is_empty(), "{}", report.join("\n"));
}

#[test]
fn the_book_builds() {
    let Some(root) = repo() else { return };
    let out =
        Command::new("mdbook").arg("build").arg("docs").current_dir(&root).output().expect("mdbook runs");
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{log}");
    assert!(!log.lines().any(|l| l.contains("WARN") || l.contains("ERROR")), "{log}");
}
