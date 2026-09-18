//! The Xous test bench: build a kernel, inject programs, boot it under QEMU, and assert
//! on what appears on the console.
//!
//! Test cases are TOML files in `xous64/tests/` (format: `case.rs`). Run with
//! `cargo testbench [FILTER]`. Console logs are kept in `target/testbench/`.

mod budget;
mod build;
mod case;
mod qemu;
mod target;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::build::Builder;
use crate::case::{Case, Kind, Program};
use crate::qemu::{Image, Verdict};
use crate::target::{Machine, Target};

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
    /// Firmware image to pass as `-bios` instead of QEMU's bundled OpenSBI, e.g. a RustSBI build.
    #[arg(long, default_value = "default")]
    firmware: String,
    /// Instead of running tests, boot these programs with the console on this terminal
    /// (Ctrl-A X quits). Names of test-programs binaries, or paths to ELF files.
    #[arg(long, num_args = 1.., value_name = "PROGRAM")]
    run: Vec<String>,
    /// Hart count for --run.
    #[arg(long, default_value_t = 1)]
    smp: u32,
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

    if !args.run.is_empty() {
        let target = target::find(args.arch.as_deref().unwrap_or("rv64")).context("unknown arch")?;
        let machine = target.machine.as_ref().map_err(|why| anyhow::anyhow!("{} cannot boot: {why}", target.name))?;
        let programs: Vec<_> = args
            .run
            .iter()
            .map(|p| if p.contains('/') { Program::Path { path: p.into() } } else { Program::TestProgram(p.clone()) })
            .collect();
        let bundle = prepare(&builder, target, machine, &programs, &[], &logs.join("interactive.tar"))?;
        let loader = builder.artifact(target, machine.loader_package);
        let image = Image { machine, firmware: &args.firmware, loader: &loader, bundle: &bundle, smp: args.smp };
        return image.run_interactive();
    }

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
        if let Kind::UnsafeBudget(check) = &case.kind {
            let (failure, summary) = budget::check(&workspace, &check.budget)?;
            match failure {
                None => println!("PASS  {:<32}\n      {summary}", case.name),
                Some(why) => {
                    failures += 1;
                    println!("FAIL  {:<32}        {why}\n      {summary}", case.name);
                }
            }
            continue;
        }
        for arch in case.arch.iter().filter(|a| args.arch.as_ref().is_none_or(|only| only == *a)) {
            let target = target::find(arch).with_context(|| format!("{}: unknown arch {arch:?}", case.name))?;
            for (variant, outcome, seconds) in run_case(&builder, case, target, &args.firmware, &logs)? {
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

/// Build the kernel, the loader and `programs` for `target`, and pack them into `bundle`.
fn prepare(
    builder: &Builder,
    target: &Target,
    machine: &Machine,
    programs: &[Program],
    extra_kernel_features: &[String],
    bundle: &std::path::Path,
) -> Result<PathBuf> {
    let mut features: Vec<String> = machine.kernel_features.iter().map(|f| f.to_string()).collect();
    features.extend(extra_kernel_features.iter().cloned());
    builder.cargo_build(target, "xous-kernel", &features)?;
    builder.cargo_build(target, machine.loader_package, &[])?;
    let programs = programs.iter().map(|p| builder.program(target, p)).collect::<Result<Vec<_>>>()?;
    build::bundle(bundle, &builder.artifact(target, "xous-kernel"), &programs)?;
    Ok(bundle.to_path_buf())
}

/// Check that every `distinct_across_boots` pattern captured something on both boots,
/// and that the two captures differ.
fn compare_boots(boot: &case::Boot, first: &[Option<String>], second: &[Option<String>]) -> Outcome {
    for ((pattern, a), b) in boot.distinct_across_boots.iter().zip(first).zip(second) {
        match (a, b) {
            (Some(a), Some(b)) if a != b => {}
            (Some(a), Some(_)) => return Outcome::Fail(format!("/{pattern}/ captured {a:?} on both boots")),
            _ => return Outcome::Fail(format!("/{pattern}/ captured nothing")),
        }
    }
    Outcome::Pass
}

/// Run one case on one target. A boot case yields one result per `smp` entry.
fn run_case(
    builder: &Builder,
    case: &Case,
    target: &'static Target,
    firmware: &str,
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
        Kind::UnsafeBudget(_) => unreachable!("handled before the per-target loop"),
    };
    let machine = match &target.machine {
        Ok(machine) => machine,
        Err(why) => return Ok(vec![(String::new(), Outcome::Skip(why.to_string()), 0.0)]),
    };

    // Build everything once, then boot it once per hart count.
    let bundle = logs.join(format!("{}-{}.tar", case.name, target.name));
    let bundle = match prepare(builder, target, machine, &boot.programs, &boot.kernel_features, &bundle) {
        Ok(bundle) => bundle,
        Err(e) => return Ok(vec![(String::new(), Outcome::Fail(format!("{e:#}")), elapsed(started))]),
    };

    let loader = builder.artifact(target, machine.loader_package);
    let mut results = Vec::new();
    for smp in &boot.smp {
        let run_started = Instant::now();
        let log = logs.join(format!("{}-{}-smp{}.log", case.name, target.name, smp));
        let image = Image { machine, firmware, loader: &loader, bundle: &bundle, smp: *smp };
        let outcome = match qemu::run(&image, boot, &log)? {
            Verdict::Fail(why) => Outcome::Fail(why),
            Verdict::Pass(_) if boot.distinct_across_boots.is_empty() => Outcome::Pass,
            Verdict::Pass(first) => {
                let second_log = log.with_extension("second-boot.log");
                match qemu::run(&image, boot, &second_log)? {
                    Verdict::Fail(why) => Outcome::Fail(format!("second boot: {why}")),
                    Verdict::Pass(second) => compare_boots(boot, &first, &second),
                }
            }
        };
        results.push((format!(", smp={smp}"), outcome, elapsed(run_started)));
    }
    Ok(results)
}
