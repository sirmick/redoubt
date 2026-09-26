//! `doccheck [--pages <path>...] [--code]`, run from the repository root: prints each finding as
//! `path:line: C<n>: message` and exits 1 if there is any.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use redoubt_doccheck::{Scope, check};

fn main() -> ExitCode {
    let mut scope = Scope { pages: None, code: false };
    let mut in_pages = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--code" => (in_pages, scope.code) = (false, true),
            "--pages" => in_pages = true,
            _ if in_pages && !arg.starts_with("--") => {
                scope.pages.get_or_insert_with(Vec::new).push(PathBuf::from(arg))
            }
            _ => {
                eprintln!("usage: doccheck [--pages <path>...] [--code]");
                return ExitCode::from(2);
            }
        }
    }
    let root = match std::env::current_dir() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("doccheck: {e}");
            return ExitCode::from(2);
        }
    };
    let findings = check(&root, scope);
    for f in &findings {
        println!("{f}");
    }
    if findings.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
