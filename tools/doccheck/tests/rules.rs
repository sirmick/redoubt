//! Each rule fires on its bad fixture and stays silent on the good tree (the harness can fail).

use std::collections::BTreeSet;
use std::path::PathBuf;

use redoubt_doccheck::{Finding, Scope, check};

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
}

/// C1's four failures on one page: no status, malformed, double, stray and misplaced lines.
#[test]
fn c1_reports_each_failure() {
    let lines: BTreeSet<usize> = run("c1").iter().filter(|f| f.rule == 1).map(|f| f.line).collect();
    assert_eq!(lines, BTreeSet::from([3, 7, 9, 17, 23, 25]));
}

/// `beyond` excuses only M5 (C3); a part of a rule must repeat its name (C5); generated Elixir
/// comments are code (C11); an anchor-only SUMMARY link names no page (C12).
#[test]
fn narrow_cases_fire() {
    let at = |tree: &str, rule: u8, path: &str, line: usize| {
        run(tree).iter().any(|f| f.rule == rule && f.path == path && f.line == line)
    };
    assert!(at("c3", 3, "docs/README.md", 5));
    assert!(at("c5", 5, "docs/kernel/a.md", 7));
    assert!(at("c11", 11, "libs/wire/elixir/proto/ping.ex", 2));
    assert!(at("c12", 12, "docs/SUMMARY.md", 4));
}
