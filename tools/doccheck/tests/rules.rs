//! Each rule fires on its bad fixture and stays silent on the good tree (the harness can fail).

use std::collections::BTreeSet;
use std::path::PathBuf;

use redoubt_doccheck::{Finding, Scope, check, elasticlunr_version, search_index_within};

fn run(tree: &str) -> Vec<Finding> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(tree);
    check(&root, Scope { pages: None, code: true })
}

fn rules(tree: &str) -> BTreeSet<u8> { run(tree).iter().map(|f| f.rule).collect() }

#[test]
fn good_tree_is_clean() {
    let findings = run("good");
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn pages_scope_keeps_only_the_listed_pages() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/c4");
    let findings = check(&root, Scope { pages: Some(vec!["./docs/README.md".into()]), code: false });
    assert!(!findings.is_empty());
    assert!(findings.iter().all(|f| f.path == "docs/README.md"), "{findings:#?}");
}

#[test]
fn pages_scope_keeps_a_directory() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/c1");
    for dir in ["docs/kernel", "docs/kernel/", "./docs"] {
        let findings = check(&root, Scope { pages: Some(vec![dir.into()]), code: false });
        assert!(
            findings.iter().any(|f| f.path == "docs/kernel/bad.md" && f.rule == 1),
            "{dir}: {findings:#?}"
        );
    }
}

macro_rules! fires {
    ($($name:ident: $rule:literal, $tree:literal;)*) => {$(
        #[test]
        fn $name() {
            assert!(rules($tree).contains(&$rule), "C{} did not fire on {}", $rule, $tree);
            assert!(!rules("good").contains(&$rule), "C{} fired on the good tree", $rule);
        }
    )*};
}

fires! {
    c1_status_lines: 1, "c1";
    c2_tests_exist: 2, "c2";
    c3_milestones: 3, "c3";
    c4_process_references: 4, "c4";
    c5_rule_ids: 5, "c5";
    c6_links: 6, "c6";
    c7_security_register: 7, "c7";
    c8_no_binaries: 8, "c8";
    c9_templates: 9, "c9";
    c10_wire_tables: 10, "c10";
    c11_code: 11, "c11";
    c12_summary: 12, "c12";
    c13_pinned_unsafe: 13, "c13";
}

/// C1's failures on one page: no status, malformed, double, stray and misplaced lines, and a
/// collapsed status whose count does not match its list.
#[test]
fn c1_reports_each_failure() {
    let lines: BTreeSet<usize> = run("c1").iter().filter(|f| f.rule == 1).map(|f| f.line).collect();
    assert_eq!(lines, BTreeSet::from([3, 7, 9, 17, 23, 25, 27, 29]));
}

/// C13's failures on one page: a count the ceilings disagree with, a budget only partly under a
/// row's paths, a count that is not one, a budget named that does not exist, and a table that
/// names its rows by neither paths nor budgets.
#[test]
fn c13_reports_each_failure() {
    let lines: BTreeSet<usize> = run("c13").iter().filter(|f| f.rule == 13).map(|f| f.line).collect();
    assert_eq!(lines, BTreeSet::from([5, 6, 7, 11, 13]));
}

/// The search index over its ceiling fails naming the size, the ceiling and the three largest
/// pages, largest first; at or under it, the size is returned.
#[test]
fn index_over_its_ceiling_names_the_largest_pages() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/index");
    let book = root.join("book");
    let err = search_index_within(&root, &book, 1_000).unwrap_err();
    assert!(err.contains("1491 bytes") && err.contains("ceiling of 1000"), "{err}");
    let at = ["docs/big.md", "docs/mid.md", "docs/small.md"].map(|p| err.find(p).expect(p));
    assert!(at[0] < at[1] && at[1] < at[2] && !err.contains("tiny.md"), "{err}");
    assert_eq!(search_index_within(&root, &book, 1_491), Ok(1491));
    assert!(search_index_within(&root, &root.join("docs"), 1_000).unwrap_err().contains("no searchindex"));
}

/// The indexer's version is read from the crate path an mdbook binary embeds; a binary without
/// one, or with a malformed one, yields none.
#[test]
fn elasticlunr_version_is_read_from_the_binary() {
    let at = |b: &[u8]| elasticlunr_version(b);
    assert_eq!(at(b"\0/registry/src/index/elasticlunr-rs-3.1.0/src/lib.rs\0"), Some((3, 1, 0)));
    assert_eq!(at(b"xelasticlunr-rs-3.0.2/src/pipeline.rs"), Some((3, 0, 2)));
    assert_eq!(at(b"elasticlunr-rs-12.34.56."), Some((12, 34, 56)));
    assert_eq!(at(b"elasticlunr-rs-3.1/src"), None);
    assert_eq!(at(b"mdbook without the path"), None);
}

/// `beyond` excuses only M6 (C3); a part of a rule must repeat its name (C5); generated and
/// hand-written Elixir, module docs included, is code (C11); an anchor-only SUMMARY link names no
/// page (C12).
#[test]
fn narrow_cases_fire() {
    let at = |tree: &str, rule: u8, path: &str, line: usize| {
        run(tree).iter().any(|f| f.rule == rule && f.path == path && f.line == line)
    };
    assert!(at("c3", 3, "docs/README.md", 5));
    assert!(at("c5", 5, "docs/kernel/a.md", 7));
    assert!(at("c11", 11, "libs/wire/elixir/proto/ping.ex", 2));
    assert!(at("c11", 11, "libs/wire/elixir/wire.ex", 3));
    assert!(at("c12", 12, "docs/SUMMARY.md", 4));
    assert!(at("c8", 8, "docs/theme/logo.svg", 1));
}

/// C11 holds only what the repository tracks: a checkout's untracked files (reference sources,
/// scratch) are not Redoubt's code.
#[test]
fn untracked_code_is_not_checked() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/c11");
    let stray = root.join("src/untracked-scratch.rs");
    std::fs::write(&stray, "// see WP-K6 and answer 12\nfn f() {}\n").unwrap();
    let findings = check(&root, Scope { pages: None, code: true });
    std::fs::remove_file(&stray).unwrap();
    assert!(findings.iter().any(|f| f.rule == 11), "the tracked c11 fixture still fires");
    assert!(
        findings.iter().all(|f| f.path != "src/untracked-scratch.rs"),
        "an untracked file was checked: {findings:#?}"
    );
}

/// A tree that is no tracked part of a repository is checked whole: the c11 fixture copied under
/// the repository's own ignored `target/` (as a copy under `.tmp/` is, by the project's scratch
/// rule), where git answers and names nothing, fires C11 on the same lines as in place.
#[test]
fn a_copy_under_an_ignored_directory_is_checked_whole() {
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let copied = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c11-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copied);
    copy(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/c11"), &copied);
    let findings = check(&copied, Scope { pages: None, code: true });
    std::fs::remove_dir_all(&copied).unwrap();
    let at =
        |path: &str, line: usize| findings.iter().any(|f| f.rule == 11 && f.path == path && f.line == line);
    assert!(
        at("libs/wire/elixir/proto/ping.ex", 2) && at("libs/wire/elixir/wire.ex", 3) && at("src/lib.rs", 1),
        "{findings:#?}"
    );
}
