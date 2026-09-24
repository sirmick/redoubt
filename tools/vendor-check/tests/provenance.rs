//! `provenance.sh` fails closed (QA D3-code-review-4): a table it cannot find, a row without a
//! vendored directory and a vendored directory without a row each fail it before anything is
//! downloaded, so it can never pass having checked nothing. The structure is checked offline on a
//! copy of the script, `vendor/README.md` and the vendored directories' names.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }

/// A scratch tree with the script, `readme` as `vendor/README.md` and an empty directory for each
/// name in `dirs`; the script's exit code and output, `args` given.
fn run(tag: &str, readme: &str, dirs: &[String], args: &[&str]) -> (i32, String) {
    let tree = std::env::temp_dir().join(format!("vendor-check-provenance-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tree);
    std::fs::create_dir_all(tree.join("tools/vendor-check")).unwrap();
    std::fs::copy(root().join("tools/vendor-check/provenance.sh"), tree.join("tools/vendor-check/provenance.sh"))
        .unwrap();
    for dir in dirs {
        std::fs::create_dir_all(tree.join("vendor").join(dir)).unwrap();
    }
    std::fs::write(tree.join("vendor/README.md"), readme).unwrap();
    let out = Command::new("bash").arg(tree.join("tools/vendor-check/provenance.sh")).args(args).output().unwrap();
    let _ = std::fs::remove_dir_all(&tree);
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

fn tree() -> (String, Vec<String>) {
    let readme = std::fs::read_to_string(root().join("vendor/README.md")).unwrap();
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(root().join("vendor")).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_dir() {
            dirs.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    (readme, dirs)
}

#[test]
fn the_real_structure_passes() {
    let (readme, dirs) = tree();
    let (code, out) = run("real", &readme, &dirs, &["--structure-only"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("every vendored crate (6)"), "{out}");
}

#[test]
fn a_renamed_header_fails_before_any_download() {
    let (readme, dirs) = tree();
    let renamed = readme.replacen("| License (ours", "| Licence (ours", 1);
    assert_ne!(renamed, readme);
    // Without --structure-only too: the full run must stop here, not check nothing and pass.
    for args in [&["--structure-only"][..], &[]] {
        let (code, out) = run("header", &renamed, &dirs, args);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("no vendored table"), "{out}");
    }
}

#[test]
fn a_missing_row_fails() {
    let (readme, dirs) = tree();
    let without: String = readme.lines().filter(|l| !l.starts_with("| `hash32`")).map(|l| format!("{l}\n")).collect();
    assert_ne!(without, readme);
    for args in [&["--structure-only"][..], &[]] {
        let (code, out) = run("row", &without, &dirs, args);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("vendor/hash32: not in the vendored table"), "{out}");
    }
}

#[test]
fn a_row_without_its_directory_and_a_stray_directory_fail() {
    let (readme, dirs) = tree();
    let fewer: Vec<String> = dirs.iter().filter(|d| *d != "managed").cloned().collect();
    let (code, out) = run("dir", &readme, &fewer, &["--structure-only"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("managed: listed but vendor/managed is missing"), "{out}");
    let mut more = dirs.clone();
    more.push("sneaky".into());
    let (code, out) = run("stray", &readme, &more, &["--structure-only"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("vendor/sneaky: not in the vendored table"), "{out}");
}
