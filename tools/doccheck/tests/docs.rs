//! The repository's own docs: every rule holds, the code's comments included, and the book
//! renders cleanly. The bench runs both (`tests/docs.toml`).

use std::path::PathBuf;
use std::process::Command;

use redoubt_doccheck::{Scope, check, elasticlunr_version, search_index_within};

fn repo() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

/// The search index's ceiling in bytes of `target/book/searchindex*.js`: 30 % under mdbook's own
/// warning at 10,000,000, which `the_book_builds` fails on without saying what grew
/// ([the book's rules](../../../docs/README.md#the-search-index)). The dev host's build measured
/// 6,093,217 on 2026-10-16, with mdbook linking elasticlunr-rs 3.1.0.
const SEARCH_INDEX_CEILING: u64 = 7_000_000;

/// The first elasticlunr-rs that indexes each field with its own tokens only.
const ELASTICLUNR_MIN: (u64, u64, u64) = (3, 1, 0);

/// The `mdbook` the build above ran: the first on `PATH`.
fn mdbook_binary() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("mdbook"))
        .find(|p| p.is_file())
        .expect("mdbook on PATH")
}

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
    // The indexer: an mdbook locked to elasticlunr-rs 3.0.2 writes an index 60 % too large
    // (scripts/setup.sh installs mdbook resolved fresh for this reason).
    let mdbook = mdbook_binary();
    let version = elasticlunr_version(&std::fs::read(&mdbook).expect("mdbook is readable"))
        .unwrap_or_else(|| {
            panic!(
                "{}: no elasticlunr-rs version in the binary; the book needs an mdbook built against 3.1.0 or later: `cargo install --force mdbook@0.5.4`",
                mdbook.display()
            )
        });
    assert!(
        version >= ELASTICLUNR_MIN,
        "{}: built with elasticlunr-rs {}.{}.{}; the book needs {}.{}.{} or later: `cargo install --force mdbook@0.5.4`",
        mdbook.display(),
        version.0,
        version.1,
        version.2,
        ELASTICLUNR_MIN.0,
        ELASTICLUNR_MIN.1,
        ELASTICLUNR_MIN.2
    );
    if let Err(e) = search_index_within(&repo(), &repo().join("target/book"), SEARCH_INDEX_CEILING) {
        panic!("{e}");
    }
}
