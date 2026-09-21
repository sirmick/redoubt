//! `cargo run -p redoubt-wire-gen` writes the typed-message codecs from the tables in the
//! notes; `-- --check` only lists generated files that are out of date (exit 1). The test
//! `generated_files_are_current` makes the same check on every `cargo test`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let root = redoubt_wire_gen::repo_root();
    let result = if std::env::args().skip(1).any(|a| a == "--check") {
        redoubt_wire_gen::stale(&root).map(|stale| {
            stale.iter().for_each(|path| eprintln!("stale: {}", path.display()));
            stale.is_empty()
        })
    } else {
        write_all(&root)
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("wire-gen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write_all(root: &std::path::Path) -> Result<bool, String> {
    let generated = redoubt_wire_gen::generate(root)?;
    for (rel, contents) in &generated {
        let path = root.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    // Deleting is left to a person, who can see why no table generates the file any more.
    let orphans = redoubt_wire_gen::orphans(root, &generated);
    orphans.iter().for_each(|path| eprintln!("no table generates {}: delete it by hand", path.display()));
    Ok(orphans.is_empty())
}
