//! The Xous test bench: build a kernel, inject programs, boot it under QEMU, and assert
//! on what appears on the console.
//!
//! Test cases are TOML files in `xous64/tests/` (format: `case.rs`). Run with
//! `cargo testbench [FILTER]`. Console logs are kept in `target/testbench/`.

mod build;
mod case;
mod qemu;
mod target;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::build::Builder;
use crate::case::{Case, Kind};
use crate::qemu::Verdict;
use crate::target::Target;

#[derive(Parser)]
#[command(about = "Boot Xous under QEMU with injected programs and assert on its console output")]
struct Args {
    /// Only run cases whose name contains this.
    filter: Option<String>,
    /// Only run on this target (rv64, rv32).
    #[arg(long)]
    arch: Option<String>,
    /// List the cases and exit.
    #[arg(long)]
    list: bool,
    /// Show cargo's output.
    #[arg(long, short)]
    verbose: bool,
}

enum Outcome {
    Pass,
    Fail(String),
    Skip(String),
}

fn main() -> Result<()> {
    let args = Args::parse();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let logs = workspace.join("target/testbench");
    std::fs::create_dir_all(&logs)?;
    let builder = Builder { workspace: workspace.clone(), verbose: args.verbose };

    let mut paths: Vec<_> = std::fs::read_dir(workspace.join("xous64/tests"))?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let cases = paths.iter().map(|p| Case::load(p)).collect::<Result<Vec<_>>>()?;

    let mut failures = 0;
    for case in cases.iter().filter(|c| args.filter.as_ref().is_none_or(|f| c.name.contains(f.as_str()))) {
        if args.list {
            println!("{:<16} [{}] {}", case.name, case.arch.join(", "), case.description);
            continue;
        }
        for arch in case.arch.iter().filter(|a| args.arch.as_ref().is_none_or(|only| only == *a)) {
            let target = target::find(arch).with_context(|| format!("{}: unknown arch {arch:?}", case.name))?;
            for (variant, outcome, seconds) in run_case(&builder, case, target, &logs)? {
                let label = format!("{} [{}{}]", case.name, target.name, variant);
                match outcome {
                    Outcome::Pass => println!("PASS  {label:<32} {seconds:5.1}s"),
                    Outcome::Skip(why) => println!("SKIP  {label:<32}        {why}"),
                    Outcome::Fail(why) => {
                        failures += 1;
                        println!("FAIL  {label:<32} {seconds:5.1}s  {why}");
                    }
                }
            }
        }
    }
    if failures > 0 {
        bail!("{failures} test(s) failed; console logs are in {}", logs.display());
    }
    Ok(())
}

/// Run one case on one target. A boot case yields one result per `smp` entry.
fn run_case(
    builder: &Builder,
    case: &Case,
    target: &'static Target,
    logs: &std::path::Path,
) -> Result<Vec<(String, Outcome, f32)>> {
    let started = Instant::now();
    let elapsed = |since: Instant| since.elapsed().as_secs_f32();

    let boot = match &case.kind {
        Kind::Build(build) => {
            let outcome = match builder.cargo_build(target, &build.package, &build.features) {
                Ok(()) => Outcome::Pass,
                Err(e) => Outcome::Fail(format!("{e:#}")),
            };
            return Ok(vec![(String::new(), outcome, elapsed(started))]);
        }
        Kind::Boot(boot) => boot,
    };
    let machine = match &target.machine {
        Ok(machine) => machine,
        Err(why) => return Ok(vec![(String::new(), Outcome::Skip(why.to_string()), 0.0)]),
    };

    // Build everything once, then boot it once per hart count.
    let prepared = (|| -> Result<_> {
        let mut features: Vec<String> = machine.kernel_features.iter().map(|f| f.to_string()).collect();
        features.extend(boot.kernel_features.iter().cloned());
        builder.cargo_build(target, "xous-kernel", &features)?;
        builder.cargo_build(target, machine.loader_package, &[])?;
        let programs = boot.programs.iter().map(|p| builder.program(target, p)).collect::<Result<Vec<_>>>()?;
        let bundle = logs.join(format!("{}-{}.tar", case.name, target.name));
        build::bundle(&bundle, &builder.artifact(target, "xous-kernel"), &programs)?;
        Ok(bundle)
    })();
    let bundle = match prepared {
        Ok(bundle) => bundle,
        Err(e) => return Ok(vec![(String::new(), Outcome::Fail(format!("{e:#}")), elapsed(started))]),
    };

    let loader = builder.artifact(target, machine.loader_package);
    let mut results = Vec::new();
    for smp in &boot.smp {
        let run_started = Instant::now();
        let log = logs.join(format!("{}-{}-smp{}.log", case.name, target.name, smp));
        let outcome = match qemu::run(machine, boot, *smp, &loader, &bundle, &log)? {
            Verdict::Pass => Outcome::Pass,
            Verdict::Fail(why) => Outcome::Fail(why),
        };
        results.push((format!(", smp={smp}"), outcome, elapsed(run_started)));
    }
    Ok(results)
}
