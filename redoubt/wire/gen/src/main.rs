//! `cargo run -p redoubt-wire-gen` regenerates the typed-message codecs from the tables in
//! the notes; `-- --check` only reports generated files that are out of date (exit 1).

use std::process::ExitCode;

fn main() -> ExitCode {
    let root = redoubt_wire_gen::repo_root();
    let check = std::env::args().skip(1).any(|a| a == "--check");
    let result = if check {
        redoubt_wire_gen::stale(&root).map(|stale| {
            for path in &stale {
                eprintln!("stale: {}", path.display());
            }
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
    for stale in redoubt_wire_gen::stale(root)? {
        let path = root.join(&stale);
        match generated.iter().find(|(p, _)| *p == stale) {
            Some((_, contents)) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("wrote {}", stale.display());
            }
            // No table produces it any more; deleting is left to a person (who can see why).
            None => println!("no table generates {}: delete it by hand", stale.display()),
        }
    }
    Ok(true)
}
