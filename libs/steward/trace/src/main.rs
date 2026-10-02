//! `steward-trace run FILE` prints the core's output for one trace.
//!
//! `steward-trace check REFERENCE DIR` checks the Elixir reference's output (REFERENCE, its run
//! over every `*.trace` in DIR) against the core's, trace by trace, stopping each at its first
//! divergence. It fails on any divergence, on a trace with no events, on a trace the reference did
//! not run, and on a row of the tables that the hand-written traces (every trace not named
//! `model-*`) leave untaken; it lists the rows the model's recorded traces never reach.

use std::process::ExitCode;

use redoubt_steward_trace::{Rows, compare, events, no_events, reference, run, untaken};

fn read(path: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn check(reference_out: &str, dir: &str) -> Result<bool, String> {
    let r = reference(&read(reference_out.as_ref())?)?;
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "trace"))
        .collect();
    files.sort();
    let (mut ok, mut hand, mut model) = (true, Rows::new(), Rows::new());
    for path in &files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let input = read(path)?;
        let rust = run(&input).map_err(|e| format!("{name}: {e}"))?;
        let Some((elixir, rows)) = r.traces.get(&name) else {
            println!("MISSING {name}: the reference did not run it");
            ok = false;
            continue;
        };
        if name.starts_with("model-") { &mut model } else { &mut hand }.extend(rows.iter().cloned());
        match no_events(&name, &input).or_else(|| compare(&name, &input, &rust, elixir)) {
            None => println!("ok {name}: {} events", events(&input).len()),
            Some(report) => {
                print!("{report}");
                ok = false;
            }
        }
    }
    let machines = redoubt_steward_gen::machines(&redoubt_steward_gen::repo_root())?;
    let missed = untaken(&machines, &hand);
    for row in &missed {
        println!("UNTAKEN by the hand-written traces: {row}");
    }
    if !model.is_empty() {
        for row in untaken(&machines, &model) {
            println!("never reached by the model's traces: {row}");
        }
    }
    Ok(ok && missed.is_empty() && !files.is_empty())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["run", file] => read(file.as_ref()).and_then(|t| run(&t)).map(|out| {
            print!("{out}");
            true
        }),
        ["check", reference_out, dir] => check(reference_out, dir),
        _ => Err("usage: steward-trace run FILE | steward-trace check REFERENCE DIR".into()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("steward-trace: {e}");
            ExitCode::FAILURE
        }
    }
}
