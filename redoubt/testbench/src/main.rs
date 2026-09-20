//! The Redoubt test bench: build a kernel, inject programs, boot it under QEMU, and assert
//! on what appears on the console.
//!
//! Test cases are TOML files in `redoubt/tests/` (format: `case.rs`). Run with
//! `cargo testbench [FILTER]`. Console logs are kept in `target/testbench/`.

mod budget;
mod build;
mod case;
mod qemu;
mod ssh;
mod target;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::build::{Builder, Profile};
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
    /// With --run, start QEMU paused with a gdb stub on :1234 (see planning/redoubt/DEBUGGING.md).
    #[arg(long)]
    debug: bool,
    /// Report a case whose firmware or OpenSSH is missing as SKIP instead of FAIL.
    #[arg(long)]
    allow_skip: bool,
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
        let bundle = prepare(
            &builder,
            target,
            machine,
            &programs,
            &[],
            &[],
            "",
            false,
            case::Signing::Domain,
            Profile::Release,
            &logs.join("interactive.tar"),
        )?;
        let loader = builder.artifact(target, machine.loader_package, Profile::Release);
        let image = Image {
            machine,
            firmware: &args.firmware,
            loader: &loader,
            bundle: &bundle,
            smp: args.smp,
            memory_mib: target::DEFAULT_MEMORY_MIB,
            devices: &[],
        };
        return image.run_interactive(args.debug);
    }
    // Something the host lacks. A skip would make the run look greener than it is.
    let missing = |why: String| {
        if args.allow_skip { Outcome::Skip(why) } else { Outcome::Fail(format!("{why} (--allow-skip to skip)")) }
    };

    let mut paths: Vec<_> = std::fs::read_dir(workspace.join("redoubt/tests"))?
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
        if let Kind::SshLoopback(loopback) = &case.kind {
            let started = Instant::now();
            let outcome = match ssh_available(true) {
                Err(why) => missing(why),
                Ok(()) => match ssh_loopback(&workspace, case, loopback, &logs) {
                    Ok(outcome) => judge(loopback.must_fail.as_deref(), outcome)?,
                    // The bench's own trouble is never what a `must_fail` is waiting for.
                    Err(e) => Outcome::Fail(format!("bench error: {e:#}")),
                },
            };
            failures += report(&case.name, outcome, started.elapsed().as_secs_f32());
            continue;
        }
        for arch in case.arch.iter().filter(|a| args.arch.as_ref().is_none_or(|only| only == *a)) {
            let target = target::find(arch).with_context(|| format!("{}: unknown arch {arch:?}", case.name))?;
            for (variant, outcome, seconds) in run_case(&builder, case, target, &args.firmware, &logs, &missing)? {
                failures += report(&format!("{} [{}{}]", case.name, target.name, variant), outcome, seconds);
            }
        }
    }
    if failures > 0 {
        bail!("{failures} test(s) failed; console logs are in {}", logs.display());
    }
    Ok(())
}

/// Print one result line; returns 1 for a failure, to count them.
fn report(label: &str, outcome: Outcome, seconds: f32) -> usize {
    match outcome {
        Outcome::Pass => println!("PASS  {label:<32} {seconds:5.1}s"),
        Outcome::Skip(why) => println!("SKIP  {label:<32}        {why}"),
        Outcome::Fail(why) => {
            println!("FAIL  {label:<32} {seconds:5.1}s  {why}");
            return 1;
        }
    }
    0
}

/// The RustSBI Prototyper binary for `target`, or an error naming where it was looked for.
/// Overridable per width with RUSTSBI_PROTOTYPER (rv64) / RUSTSBI_PROTOTYPER_RV32 (rv32).
/// By default it is looked for in a `rustsbi` checkout beside this repository's main checkout,
/// found through git so that a worktree resolves to the same place.
fn rustsbi_prototyper(target: &Target) -> Result<String, String> {
    let (env, arch) = if target.triple.starts_with("riscv64") {
        ("RUSTSBI_PROTOTYPER", "riscv64gc-unknown-none-elf")
    } else {
        ("RUSTSBI_PROTOTYPER_RV32", "riscv32imac-unknown-none-elf")
    };
    let path = match std::env::var(env) {
        Ok(path) => path,
        Err(_) => {
            let common = Command::new("git")
                .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .output()
                .map_err(|e| format!("running git to find the RustSBI checkout: {e}"))?;
            // The common directory is the main checkout's `.git`; rustsbi sits beside the checkout.
            let common = PathBuf::from(String::from_utf8_lossy(&common.stdout).trim());
            let siblings = common.parent().and_then(Path::parent).ok_or("cannot locate the main checkout")?;
            format!("{}/rustsbi/target/{arch}/release/rustsbi-prototyper", siblings.display())
        }
    };
    if std::path::Path::new(&path).exists() {
        Ok(path)
    } else {
        Err(format!("RustSBI Prototyper not found ({path}); build it or set {env}"))
    }
}

/// Resolve a case's firmware choice to a `-bios` value. rv64 defaults to QEMU's bundled
/// OpenSBI; QEMU ships none for rv32, so rv32 always boots under RustSBI. A case may force
/// "rustsbi". A binary that is absent fails the case (or, with --allow-skip, skips it).
fn resolve_firmware(case_firmware: Option<&str>, cli_default: &str, target: &Target) -> Result<String, String> {
    let rv64 = target.triple.starts_with("riscv64");
    match case_firmware {
        Some("rustsbi") => rustsbi_prototyper(target),
        None | Some("opensbi") if rv64 => Ok(cli_default.to_string()),
        // rv32: no bundled OpenSBI, so the default firmware is RustSBI.
        None | Some("opensbi") => rustsbi_prototyper(target),
        Some(other) => Err(format!("unknown firmware {other:?}")),
    }
}

/// Build the kernel, the loader, `programs` and `files` for `target`, and pack them into `bundle`.
/// `profile` applies to the kernel and the loader (the trusted base); programs are always release.
#[allow(clippy::too_many_arguments)]
fn prepare(
    builder: &Builder,
    target: &Target,
    machine: &Machine,
    programs: &[Program],
    files: &[case::BundleFile],
    extra_kernel_features: &[String],
    manifest: &str,
    tamper: bool,
    signing: case::Signing,
    profile: Profile,
    bundle: &Path,
) -> Result<PathBuf> {
    let mut features: Vec<String> = machine.kernel_features.iter().map(|f| f.to_string()).collect();
    features.extend(extra_kernel_features.iter().cloned());
    builder.cargo_build(target, "xous-kernel", &features, profile)?;
    builder.cargo_build(target, machine.loader_package, &[], profile)?;
    let programs = programs.iter().map(|p| builder.program(target, p)).collect::<Result<Vec<_>>>()?;
    let files = files
        .iter()
        .map(|file| Ok((file.name.clone(), builder.program(target, &file.from)?.1)))
        .collect::<Result<Vec<_>>>()?;
    build::bundle(
        bundle,
        &builder.artifact(target, "xous-kernel", profile),
        &programs,
        &files,
        manifest,
        tamper,
        signing,
    )?;
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


/// Run one case on one target. A boot case yields one result per `smp` entry. `missing` turns
/// something the host lacks into a failure or, with --allow-skip, a skip.
fn run_case(
    builder: &Builder,
    case: &Case,
    target: &'static Target,
    firmware: &str,
    logs: &Path,
    missing: &dyn Fn(String) -> Outcome,
) -> Result<Vec<(String, Outcome, f32)>> {
    let started = Instant::now();
    let elapsed = |since: Instant| since.elapsed().as_secs_f32();

    let boot = match &case.kind {
        Kind::Build(build) => {
            let outcome = match builder.cargo_build(target, &build.package, &build.features, Profile::Release) {
                Ok(()) => Outcome::Pass,
                Err(e) => Outcome::Fail(format!("{e:#}")),
            };
            return Ok(vec![(String::new(), outcome, elapsed(started))]);
        }
        Kind::Boot(boot) => boot,
        Kind::UnsafeBudget(_) | Kind::SshLoopback(_) => unreachable!("handled before the per-target loop"),
    };
    let machine = match &target.machine {
        Ok(machine) => machine,
        Err(why) => return Ok(vec![(String::new(), Outcome::Skip(why.to_string()), 0.0)]),
    };
    let firmware = match resolve_firmware(boot.firmware.as_deref(), firmware, target) {
        Ok(firmware) => firmware,
        Err(why) => return Ok(vec![(String::new(), missing(why), 0.0)]),
    };
    if !boot.session.is_empty() {
        if let Err(why) = ssh_available(false) {
            return Ok(vec![(String::new(), missing(why), 0.0)]);
        }
    }

    // Build everything once, then boot it once per hart count. A build failure is the bench's
    // or the code's problem, never what a `must_fail` is waiting for, so it is not judged.
    let bundle = logs.join(format!("{}-{}.tar", case.name, target.name));
    let manifest = boot.grant.iter().flat_map(|g| g.manifest_lines()).collect::<Vec<_>>().join("\n");
    let profile = if boot.debug_assertions { Profile::Checked } else { Profile::Release };
    let bundle = match prepare(
        builder,
        target,
        machine,
        &boot.programs,
        &boot.file,
        &boot.kernel_features,
        &manifest,
        boot.tamper_bundle,
        boot.sign_bundle,
        profile,
        &bundle,
    ) {
        Ok(bundle) => bundle,
        Err(e) => return Ok(vec![(String::new(), Outcome::Fail(format!("{e:#}")), elapsed(started))]),
    };

    let loader = builder.artifact(target, machine.loader_package, profile);
    let mut results = Vec::new();
    for smp in &boot.smp {
        let run_started = Instant::now();
        let log = logs.join(format!("{}-{}-smp{}.log", case.name, target.name, smp));
        // Every boot gets fresh devices: a new disk, new host ports.
        let boot_once = |log: &Path| -> Result<Verdict> {
            let (devices, forwards) = qemu::virtio_devices(boot, &log.with_extension("img"))?;
            let image = Image {
                machine,
                firmware: &firmware,
                loader: &loader,
                bundle: &bundle,
                smp: *smp,
                memory_mib: boot.memory_mib.unwrap_or(target::DEFAULT_MEMORY_MIB),
                devices: &devices,
            };
            qemu::run(&image, boot, &builder.workspace, &forwards, log)
        };
        let outcome = match boot_once(&log)? {
            Verdict::Fail(why) => Outcome::Fail(why),
            Verdict::Pass(_) if boot.distinct_across_boots.is_empty() => Outcome::Pass,
            Verdict::Pass(first) => {
                let second_log = log.with_extension("second-boot.log");
                match boot_once(&second_log)? {
                    Verdict::Fail(why) => Outcome::Fail(format!("second boot: {why}")),
                    Verdict::Pass(second) => compare_boots(boot, &first, &second),
                }
            }
        };
        results.push((format!(", smp={smp}"), judge(boot.must_fail.as_deref(), outcome)?, elapsed(run_started)));
    }
    Ok(results)
}

/// Apply a case's `must_fail` to the verdict of its run: then the case passes only if the run
/// failed, and for the named reason, so a self-check cannot pass by failing for some other one.
fn judge(must_fail: Option<&str>, outcome: Outcome) -> Result<Outcome> {
    let Some(pattern) = must_fail else { return Ok(outcome) };
    let pattern = regex::Regex::new(pattern).with_context(|| format!("bad regular expression {pattern:?}"))?;
    Ok(match outcome {
        Outcome::Fail(why) if pattern.is_match(&why) => Outcome::Pass,
        Outcome::Fail(why) => Outcome::Fail(format!("failed, but not with /{pattern}/: {why}")),
        Outcome::Pass => Outcome::Fail(format!("passed, but must fail with /{pattern}/")),
        skip @ Outcome::Skip(_) => skip,
    })
}

/// SSH sessions need OpenSSH's client, and loopback cases its server, on the host.
fn ssh_available(server_too: bool) -> Result<(), String> {
    match Command::new(ssh::SSH).arg("-V").stderr(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        _ => return Err(format!("OpenSSH's `{}` is not installed", ssh::SSH)),
    }
    if server_too && !Path::new(ssh::SSHD).exists() {
        return Err(format!("OpenSSH's server {} is not installed", ssh::SSHD));
    }
    Ok(())
}

/// Run an `ssh-loopback` case: its sessions against a host sshd accepting its keys. An error
/// is the bench's own trouble; the outcome is the sessions' verdict.
fn ssh_loopback(workspace: &Path, case: &Case, loopback: &case::SshLoopback, logs: &Path) -> Result<Outcome> {
    let deadline = Instant::now() + std::time::Duration::from_secs_f64(loopback.timeout_secs);
    let server =
        ssh::loopback(workspace, &logs.join("ssh"), &case.name, &loopback.authorized, loopback.host_key.as_deref())?;
    let abort = std::sync::atomic::AtomicBool::new(false);
    Ok(match ssh::run(workspace, &loopback.session, &server, logs, &case.name, deadline, &abort)? {
        None => Outcome::Pass,
        Some(why) => Outcome::Fail(why),
    })
}
