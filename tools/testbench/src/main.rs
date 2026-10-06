//! The Redoubt test bench: build a kernel, inject programs, boot it under QEMU, and assert
//! on what appears on the console.
//!
//! Test cases are TOML files in `tests/` (format: `case.rs`). Run with
//! `cargo testbench [FILTER]`. Each run keeps its console logs in a directory of its own,
//! `target/testbench/run-<pid>-<time>/`, and `target/testbench/last` names the latest (`run.rs`).

mod budget;
mod build;
mod case;
mod cruft;
mod disk;
mod elixir;
mod fmt;
mod memory;
mod peer;
mod pty;
mod qemu;
mod run;
mod sched_oracle;
mod size;
mod ssh;
mod ssh_guest;
mod target;
mod userland;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::build::{Builder, Profile};
use crate::case::{Case, Kind, LoopbackServer, Program};
use crate::qemu::{Image, Verdict};
use crate::target::{Machine, Target};

#[derive(Parser)]
#[command(about = "Boot Redoubt under QEMU with injected programs and assert on its console output")]
struct Args {
    /// Only run cases whose name contains this.
    filter: Option<String>,
    /// Only run on this target (rv64, rv32).
    #[arg(long)]
    arch: Option<String>,
    /// List the cases and exit.
    #[arg(long)]
    list: bool,
    /// Instead of running tests, boot the kernel with the console on this terminal
    /// (Ctrl-A X quits). Add `--program NAME` to start programs after it.
    #[arg(long)]
    run: bool,
    /// With --run, a program to start: a test-programs binary name or a path to an ELF file.
    #[arg(long = "program", value_name = "PROGRAM")]
    programs: Vec<String>,
    /// With --run, pack the bundle from this recipe instead (`image/boot.toml`, which `./mkimage`
    /// uses): the kernel, `init`, the servers and the manifest.
    #[arg(long, value_name = "RECIPE", conflicts_with = "programs")]
    recipe: Option<PathBuf>,
    /// Instead of running tests, pack the disk recipe RECIPE (`image/disk.toml` or
    /// `image/userland.toml`, which `./mkimage` uses) into the raw disk image OUT, and exit. The
    /// userland disk's objects are staged first, and each verified volume's root and data blocks
    /// are printed: the ones the bundle's manifest pins.
    #[arg(long, num_args = 2, value_names = ["RECIPE", "OUT"])]
    pack_disk: Option<Vec<PathBuf>>,
    /// Hart count for --run.
    #[arg(long, default_value_t = 1)]
    smp: u32,
    /// With --run, print the exact QEMU command line before booting.
    #[arg(long)]
    print_qemu: bool,
    /// With --run, print the QEMU command line and exit without booting.
    #[arg(long)]
    print_only: bool,
    /// With --run, start QEMU paused with a gdb stub on :1234.
    #[arg(long)]
    debug: bool,
    /// Report a case whose firmware, QEMU or OpenSSH is missing or too old as SKIP instead of FAIL.
    #[arg(long)]
    allow_skip: bool,
    /// Show cargo's output.
    #[arg(long, short)]
    verbose: bool,
    /// Run the one case the filter names once per guest seed, from one build: `A..B`
    /// (inclusive) or `A,B,C`, at most 10,000 seeds.
    #[arg(long, value_name = "SEEDS", value_parser = parse_seeds)]
    sweep: Option<Seeds>,
    /// With --sweep, boot up to J seeds at once (1 by default, at most the host's parallelism).
    /// A result under J above 1 is a sweep datum, never a verdict.
    #[arg(long, value_name = "J", requires = "sweep")]
    jobs: Option<usize>,
}

/// The seeds of a `--sweep`, each once.
#[derive(Clone, Debug, PartialEq)]
struct Seeds(Vec<u64>);

/// The most seeds one sweep boots: past it, a typo in a range, not a sweep.
const MAX_SEEDS: u64 = 10_000;

/// `A..B` (inclusive) or a comma list; empty, reversed or repeated seeds are refused, and so are
/// more than `MAX_SEEDS`.
fn parse_seeds(text: &str) -> Result<Seeds, String> {
    let seed = |text: &str| text.trim().parse::<u64>().map_err(|_| format!("{text:?} is not a seed"));
    let seeds: Vec<u64> = match text.split_once("..") {
        Some((first, last)) => {
            let (first, last) = (seed(first)?, seed(last)?);
            if first > last {
                return Err(format!("{first}..{last} is reversed"));
            }
            if last - first >= MAX_SEEDS {
                return Err(format!("{first}..{last} is more than {MAX_SEEDS} seeds"));
            }
            (first..=last).collect()
        }
        None => text.split(',').map(seed).collect::<Result<_, _>>()?,
    };
    if seeds.len() as u64 > MAX_SEEDS {
        return Err(format!("{} seeds is more than {MAX_SEEDS}", seeds.len()));
    }
    let mut seen = HashSet::new();
    if let Some(again) = seeds.iter().find(|seed| !seen.insert(**seed)) {
        return Err(format!("seed {again} is given twice"));
    }
    Ok(Seeds(seeds))
}

/// A `--sweep`: one case booted once per seed on each of its targets, `jobs` boots at a time.
struct Sweep<'a> {
    case: &'a Case,
    seeds: Vec<u64>,
    jobs: usize,
}

/// The sweep `args` asks for, if any, or why it is refused, before anything builds. `replay` is
/// TESTBENCH_QEMU_SEED, and `parallelism` the host's.
fn sweep<'a>(
    args: &Args,
    cases: &'a [Case],
    replay: Option<String>,
    parallelism: usize,
) -> Result<Option<Sweep<'a>>> {
    let Some(Seeds(seeds)) = &args.sweep else { return Ok(None) };
    if let Some(replay) = replay {
        bail!("TESTBENCH_QEMU_SEED={replay} replays one seed, and --sweep names its own: not both");
    }
    let jobs = args.jobs.unwrap_or(1);
    if jobs == 0 || jobs > parallelism {
        bail!("--jobs {jobs}: from 1 to this host's parallelism, {parallelism}");
    }
    let Some(filter) = args.filter.as_deref() else { bail!("--sweep runs one case: name it") };
    let named = Case::only(cases, filter);
    let [case] = named[..] else { bail!("--sweep runs one case, and {filter:?} names {}", named.len()) };
    let Kind::Boot(boot) = &case.kind else { bail!("{}: --sweep needs a boot case", case.name) };
    if boot.qemu_seed.is_none() {
        bail!("{}: --sweep needs a case that pins qemu_seed", case.name);
    }
    if let Some(only) = args.arch.as_deref().filter(|only| !case.arch.iter().any(|a| a == only)) {
        bail!("{}: --arch {only} is not one of its targets ({})", case.name, case.arch.join(", "));
    }
    Ok(Some(Sweep { case, seeds: seeds.clone(), jobs }))
}

enum Outcome {
    Pass,
    Fail(String),
    Skip(String),
}

fn main() -> Result<()> {
    // Not a command for people: what libslirp runs for each connection to a peer (`peer.rs`).
    if std::env::args().nth(1).as_deref() == Some(peer::HELPER) {
        peer::helper(std::env::args().skip(2));
    }
    let args = Args::parse();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    if let Some([recipe, out]) = args.pack_disk.as_deref() {
        let recipe = disk::Recipe::load(recipe)?;
        // The userland disk: its objects staged first.
        if let Some(objects) = &recipe.objects {
            let stage = recipe.partition[0]
                .stage
                .as_ref()
                .context("a userland recipe --pack-disk packs needs a stage")?;
            let (count, bytes) = userland::stage(&workspace, objects, &workspace.join(stage))?;
            println!("{count} objects, {bytes} bytes");
        }
        let (disk, verified) = disk::pack(&recipe, &workspace, None)?;
        std::fs::write(out, disk).with_context(|| format!("writing {}", out.display()))?;
        for v in &verified {
            let blocks = v.geometry.data_blocks();
            println!("verified volume {}: root {}, {blocks} blocks", v.name, v.root_hex());
        }
        return Ok(());
    }
    let run = run::Run::start(&workspace.join("target/testbench"))?;
    let logs = run.dir.clone();
    let builder = Builder {
        workspace: workspace.clone(),
        run: run.dir.clone(),
        verbose: args.verbose,
        staged: Default::default(),
    };

    if args.run {
        let target = target::find(args.arch.as_deref().unwrap_or("rv64")).context("unknown arch")?;
        let machine =
            target.machine.as_ref().map_err(|why| anyhow::anyhow!("{} cannot boot: {why}", target.name))?;
        let (programs, files) = match &args.recipe {
            Some(recipe) => case::Recipe::load(recipe)?.contents()?,
            None => {
                let programs = args
                    .programs
                    .iter()
                    .map(|p| {
                        if p.contains('/') {
                            Program::Path { path: p.into() }
                        } else {
                            Program::TestProgram(p.clone())
                        }
                    })
                    .collect();
                (programs, Vec::new())
            }
        };
        let (bundle, loader) = prepare(
            &builder,
            target,
            machine,
            &programs,
            &files,
            None,
            &[],
            false,
            false,
            Profile::Release,
            &logs.join("interactive.tar"),
        )?;
        let firmware = rustsbi_prototyper(target).map_err(|why| anyhow::anyhow!(why))?;
        let image = Image {
            machine,
            firmware: &firmware,
            loader: &loader,
            bundle: &bundle,
            smp: args.smp,
            memory_mib: target::DEFAULT_MEMORY_MIB,
            devices: &[],
        };
        return image.run_interactive(args.debug, args.print_qemu, args.print_only);
    }
    // Something the host lacks. A skip would make the run look greener than it is.
    let missing = |why: String| {
        if args.allow_skip {
            Outcome::Skip(why)
        } else {
            Outcome::Fail(format!("{why} (--allow-skip to skip)"))
        }
    };

    let mut paths: Vec<_> = std::fs::read_dir(workspace.join("tests"))?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let cases = paths.iter().map(|p| Case::load(p)).collect::<Result<Vec<_>>>()?;
    let parallelism = std::thread::available_parallelism().map_or(1, |n| n.get());
    let replay = std::env::var("TESTBENCH_QEMU_SEED").ok();
    if let Some(sweep) = sweep(&args, &cases, replay, parallelism)? {
        return run_sweep(&builder, &sweep, args.arch.as_deref(), &logs, &missing);
    }

    let mut failures = 0;
    // Probed once, at the first loopback case.
    let mut loopback_usable: Option<Result<(), ssh::Unusable>> = None;
    for case in cases.iter().filter(|c| c.matches(args.filter.as_deref())) {
        if args.list {
            let mark = if case.whole_run { "" } else { "(by name only) " };
            println!("{:<16} [{}] {mark}{}", case.name, case.arch.join(", "), case.description);
            continue;
        }
        if !case.chosen(args.filter.as_deref()) {
            continue;
        }
        if let Kind::UnsafeBudget(check) = &case.kind {
            let (failure, summary) =
                budget::check(&workspace, &format!("tests/{}.toml", case.name), &check.budget)?;
            let failure = match failure {
                None => budget::coverage(&workspace, &check.budget, &check.uncounted)?,
                failure => failure,
            };
            match failure {
                None => println!("PASS  {:<32}\n      {summary}", case.name),
                Some(why) => {
                    failures += 1;
                    println!("FAIL  {:<32}        {why}\n      {summary}", case.name);
                }
            }
            continue;
        }
        if let Kind::SizeBudget(budget) = &case.kind {
            let (failure, summary) = size::check(&workspace, &format!("tests/{}.toml", case.name), budget)?;
            match failure {
                None => println!("PASS  {:<32}\n      {summary}", case.name),
                Some(why) => {
                    failures += 1;
                    println!("FAIL  {:<32}        {why}\n      {summary}", case.name);
                }
            }
            continue;
        }
        if let Kind::NoCruft(gate) = &case.kind {
            match cruft::check(&workspace, gate)? {
                None => println!("PASS  {:<32}", case.name),
                Some(why) => {
                    failures += 1;
                    println!("FAIL  {:<32}\n      {why}", case.name);
                }
            }
            continue;
        }
        if let Kind::Fmt(gate) = &case.kind {
            let started = Instant::now();
            let outcome = match fmt::available() {
                Err(why) => missing(why),
                Ok(()) => match fmt::check(&workspace, gate)? {
                    None => Outcome::Pass,
                    Some(why) => Outcome::Fail(format!("\n      {why}")),
                },
            };
            failures += report(&case.name, outcome, started.elapsed().as_secs_f32());
            continue;
        }
        if let Kind::HostTests(host) = &case.kind {
            let started = Instant::now();
            let available = if host.miri { build::miri_available() } else { Ok(()) };
            let path = std::env::var_os("PATH");
            let available = available.and_then(|()| build::tools_available(&host.tools, path.as_deref()));
            let outcome = match available.map(|()| builder.cargo_test(host)) {
                Err(why) => missing(why),
                Ok(Ok(None)) => Outcome::Pass,
                Ok(Ok(Some(why))) => Outcome::Fail(format!("host tests failed:\n      {why}")),
                Ok(Err(e)) => Outcome::Fail(format!("bench error: {e:#}")),
            };
            failures += report(&case.name, outcome, started.elapsed().as_secs_f32());
            continue;
        }
        if let Kind::SshLoopback(loopback) = &case.kind {
            let started = Instant::now();
            // Only OpenSSH's server needs a guest.
            let usable = match loopback.server {
                LoopbackServer::Openssh => loopback_usable
                    .get_or_insert_with(|| {
                        ssh_available()
                            .map_err(ssh::Unusable::Host)
                            .and_then(|()| ssh::loopback_usable(&workspace, &logs.join("ssh")))
                    })
                    .clone(),
                LoopbackServer::Redoubt => ssh_available().map_err(ssh::Unusable::Host),
            };
            let outcome = match usable {
                Err(ssh::Unusable::Host(why)) => missing(why),
                Err(ssh::Unusable::Broken(why)) => Outcome::Fail(why),
                Ok(()) => match ssh_loopback(&workspace, case, loopback, &logs) {
                    Ok(outcome) => judge(loopback.must_fail.as_deref(), outcome)?,
                    // The bench's own trouble is never what a `must_fail` is waiting for.
                    Err(e) => Outcome::Fail(format!("bench error: {e:#}")),
                },
            };
            failures += report(&case.name, outcome, started.elapsed().as_secs_f32());
            continue;
        }
        if let Kind::Elixir(oracle) = &case.kind {
            let started = Instant::now();
            let log = logs.join(format!("{}.log", case.name));
            // Never a skip: an oracle that does not run catches nothing. Nor what a `must_fail` waits for.
            let outcome = match elixir::toolchain(&workspace, oracle) {
                Err(why) => Outcome::Fail(why),
                Ok(()) => match elixir::run(&workspace, oracle, &log) {
                    Ok(None) => judge(oracle.must_fail.as_deref(), Outcome::Pass)?,
                    Ok(Some(why)) => judge(oracle.must_fail.as_deref(), Outcome::Fail(why))?,
                    Err(e) => Outcome::Fail(format!("bench error: {e:#}")),
                },
            };
            failures += report(&case.name, outcome, started.elapsed().as_secs_f32());
            continue;
        }
        for arch in case.arch.iter().filter(|a| args.arch.as_ref().is_none_or(|only| only == *a)) {
            let target =
                target::find(arch).with_context(|| format!("{}: unknown arch {arch:?}", case.name))?;
            for (variant, outcome, seconds) in run_case(&builder, case, target, &logs, &missing)? {
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
    let (line, failed) = result_line(label, outcome, seconds);
    println!("{line}");
    failed
}

/// One result line, and 1 for a failure.
fn result_line(label: &str, outcome: Outcome, seconds: f32) -> (String, usize) {
    match outcome {
        Outcome::Pass => (format!("PASS  {label:<32} {seconds:5.1}s"), 0),
        Outcome::Skip(why) => (format!("SKIP  {label:<32}        {why}"), 0),
        Outcome::Fail(why) => (format!("FAIL  {label:<32} {seconds:5.1}s  {why}"), 1),
    }
}

/// One boot of a sweep, as it ended: what it printed before its results, and its results (one
/// per `smp` entry), or the bench's own error.
struct SeedRun {
    seed: u64,
    arch: &'static str,
    notes: Vec<String>,
    results: Result<Results>,
}

/// Where one boot of a sweep keeps its files: a directory of its own in the run's, so no two
/// boots share a file.
fn seed_dir(logs: &Path, seed: u64, arch: &str) -> PathBuf { logs.join(format!("seed-{seed}-{arch}")) }

/// A boot's console log in `logs`; its disk, transcripts and captures are named after it.
fn boot_log(logs: &Path, case: &str, arch: &str, smp: u32) -> PathBuf {
    logs.join(format!("{case}-{arch}-smp{smp}.log"))
}

/// Build `sweep`'s case once for each target, then boot it once per seed, `sweep.jobs` at a time,
/// and print the join.
fn run_sweep(
    builder: &Builder,
    sweep: &Sweep,
    only_arch: Option<&str>,
    logs: &Path,
    missing: &dyn Fn(String) -> Outcome,
) -> Result<()> {
    let case = sweep.case;
    let mut failures = 0;
    let mut built = Vec::new();
    for arch in case.arch.iter().filter(|a| only_arch.is_none_or(|only| only == *a)) {
        let target = target::find(arch).with_context(|| format!("{}: unknown arch {arch:?}", case.name))?;
        match build_case(builder, case, target, logs, missing)? {
            Ok(ready) => built.push(ready),
            Err(results) => {
                for (variant, outcome, seconds) in results {
                    failures +=
                        report(&format!("{} [{}{}]", case.name, target.name, variant), outcome, seconds);
                }
            }
        }
    }
    let boots: Vec<(u64, &Built)> =
        sweep.seeds.iter().flat_map(|seed| built.iter().map(move |ready| (*seed, ready))).collect();
    let runs = in_parallel(&boots, sweep.jobs, |&(seed, ready)| {
        let mut notes = Vec::new();
        let dir = seed_dir(logs, seed, ready.target.name);
        let results = std::fs::create_dir(&dir)
            .with_context(|| format!("creating {}", dir.display()))
            .and_then(|()| boot_case(builder, ready, Some(seed), &dir, &mut |line| notes.push(line)));
        SeedRun { seed, arch: ready.target.name, notes, results }
    });
    let arches: Vec<&str> = built.iter().map(|ready| ready.target.name).collect();
    let (text, failed) = join(&case.name, &arches, runs);
    print!("{text}");
    failures += failed;
    if failures > 0 {
        bail!("{failures} test(s) failed; console logs are in {}", logs.display());
    }
    Ok(())
}

/// Run `work` on every item, up to `jobs` at once; the results come in the order they finished.
fn in_parallel<T: Sync, R: Send>(items: &[T], jobs: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(items.len()) {
            scope.spawn(|| {
                while let Some(item) = items.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                    let result = work(item);
                    done.lock().unwrap().push(result);
                }
            });
        }
    });
    done.into_inner().unwrap()
}

/// The text a sweep prints when every boot has ended, and its failures: each seed's result
/// exactly as a single run prints it, in seed order and then in `arches`' order, whatever order
/// they finished in; then one summary line for each target.
fn join(case: &str, arches: &[&str], mut runs: Vec<SeedRun>) -> (String, usize) {
    let arch_index = |arch: &str| arches.iter().position(|a| *a == arch);
    runs.sort_by_key(|run| (run.seed, arch_index(run.arch)));
    let mut text = String::new();
    let mut failures = 0;
    // Each boot's target, seed and whether it failed.
    let mut ended: Vec<(&str, u64, bool)> = Vec::new();
    for run in runs {
        for note in run.notes {
            text += &format!("{note}\n");
        }
        // The bench's own trouble with one boot is that seed's failure, not the end of the sweep.
        let results = run
            .results
            .unwrap_or_else(|e| vec![(String::new(), Outcome::Fail(format!("bench error: {e:#}")), 0.0)]);
        let mut failed = 0;
        for (variant, outcome, seconds) in results {
            let (line, fail) = result_line(&format!("{case} [{}{variant}]", run.arch), outcome, seconds);
            text += &format!("{line}\n");
            failed += fail;
        }
        ended.push((run.arch, run.seed, failed > 0));
        failures += failed;
    }
    for arch in arches {
        let seeds = ended.iter().filter(|(a, _, _)| a == arch).count();
        let failed: Vec<String> = ended
            .iter()
            .filter(|(a, _, failed)| a == arch && *failed)
            .map(|(_, seed, _)| seed.to_string())
            .collect();
        let plural = if seeds == 1 { "" } else { "s" };
        text += &format!(
            "sweep {case} {arch}: {seeds} seed{plural}, {} passed, {} failed",
            seeds - failed.len(),
            failed.len()
        );
        if !failed.is_empty() {
            text += &format!(": {}", failed.join(","));
        }
        text += "\n";
    }
    (text, failures)
}

/// The RustSBI Prototyper binary for `target`, or an error naming where it was looked for.
/// Overridable per width with RUSTSBI_PROTOTYPER (rv64) / RUSTSBI_PROTOTYPER_RV32 (rv32).
/// By default it is looked for in the vendored `bios/` firmware tree at the repo root.
fn rustsbi_prototyper(target: &Target) -> Result<String, String> {
    let (env, arch) = if target.triple.starts_with("riscv64") {
        ("RUSTSBI_PROTOTYPER", "riscv64gc-unknown-none-elf")
    } else {
        ("RUSTSBI_PROTOTYPER_RV32", "riscv32imac-unknown-none-elf")
    };
    let path = match std::env::var(env) {
        Ok(path) => path,
        Err(_) => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            format!("{}/bios/target/{arch}/release/rustsbi-prototyper", root.display())
        }
    };
    if std::path::Path::new(&path).exists() {
        Ok(std::fs::canonicalize(&path).map(|p| p.to_string_lossy().into_owned()).unwrap_or(path))
    } else {
        Err(format!(
            "RustSBI Prototyper not found ({path}); build it with scripts/build-bios.sh or set {env}"
        ))
    }
}

/// Build the kernel, the loader, `programs` and `files` for `target`, pack them into `bundle`, and
/// return the bundle and the loader. A manifest file pins its userland disk's roots, one digit
/// off if the case's `userland` asks for a wrong root. `profile` applies to the kernel and the
/// loader (the trusted base); programs are always release.
#[allow(clippy::too_many_arguments)]
fn prepare(
    builder: &Builder,
    target: &Target,
    machine: &Machine,
    programs: &[Program],
    files: &[case::BundleFile],
    userland: Option<&case::Userland>,
    extra_kernel_features: &[String],
    tamper: bool,
    bare_archive: bool,
    profile: Profile,
    bundle: &Path,
) -> Result<(PathBuf, PathBuf)> {
    let mut features: Vec<String> = machine.kernel_features.iter().map(|f| f.to_string()).collect();
    features.extend(extra_kernel_features.iter().cloned());
    let kernel = builder.binary(target, "redoubt-kernel", None, &features, profile)?;
    let loader = builder.binary(target, machine.loader_package, None, &[], profile)?;
    let budgets: Vec<&[String]> = programs.iter().map(Program::budgets).collect();
    let under_init = programs.first().is_some_and(Program::is_init);
    let programs = programs.iter().map(|p| builder.program(target, p)).collect::<Result<Vec<_>>>()?;
    let files = files
        .iter()
        .map(|file| {
            let path = builder.program(target, &file.from)?.1;
            if file.servers.is_empty() && file.verity.is_none() {
                return Ok((file.name.clone(), path));
            }
            let mut bytes = file.merged(&std::fs::read(&path)?)?;
            if let Some(recipe) = &file.verity {
                let wrong = userland.is_some_and(|u| u.wrong_root && &u.recipe == recipe);
                bytes = build::pin_roots(&bytes, &builder.userland(recipe)?.verified, wrong)?;
            }
            // Beside this case's bundle: two cases may give one file name different bytes.
            let merged = bundle.with_extension(format!("{}.json", file.name));
            std::fs::write(&merged, bytes)?;
            Ok((file.name.clone(), merged))
        })
        .collect::<Result<Vec<_>>>()?;
    // A case may bring its own `programs` entry, a hostile one, as a file; the real `init` reads a
    // manifest instead, and is given none.
    let listing = build::programs_entry(&programs, &budgets);
    let listing =
        (!files.iter().any(|(name, _)| name == "programs") && !under_init).then_some(listing.as_slice());
    build::bundle(bundle, &kernel, &programs, listing, &files, tamper, bare_archive)?;
    Ok((bundle.to_path_buf(), loader))
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

/// Check that every `distinct` pattern matched at least two lines of a boot's console `log`,
/// and that no two of its captures are the same.
fn distinct_within(patterns: &[String], log: &str) -> Result<(), String> {
    for pattern in patterns {
        let re = regex::Regex::new(pattern).map_err(|e| format!("/{pattern}/: {e}"))?;
        let mut seen = HashSet::new();
        for captures in log.lines().filter_map(|line| re.captures(line)) {
            let got = captures.get(1).map_or("", |m| m.as_str());
            if !seen.insert(got) {
                return Err(format!("/{pattern}/ captured {got:?} twice"));
            }
        }
        if seen.len() < 2 {
            return Err(format!("/{pattern}/ matched {} line(s), not two", seen.len()));
        }
    }
    Ok(())
}

/// A case's results on one target: a variant (`, smp=N`, or none), its outcome and its seconds.
type Results = Vec<(String, Outcome, f32)>;

/// Run one case on one target. A boot case yields one result per `smp` entry. `missing` turns
/// something the host lacks into a failure or, with --allow-skip, a skip.
fn run_case(
    builder: &Builder,
    case: &Case,
    target: &'static Target,
    logs: &Path,
    missing: &dyn Fn(String) -> Outcome,
) -> Result<Results> {
    match build_case(builder, case, target, logs, missing)? {
        Ok(ready) => boot_case(builder, &ready, qemu_seed(ready.boot)?, logs, &mut |line| println!("{line}")),
        Err(results) => Ok(results),
    }
}

/// A boot case built and packed for one target, to boot any number of times.
struct Built<'a> {
    boot: &'a case::Boot,
    name: &'a str,
    target: &'static Target,
    machine: &'static Machine,
    firmware: String,
    loader: PathBuf,
    bundle: PathBuf,
    userland: Option<userland::Staged>,
}

/// Build a boot case for `target`, or the results of a case that ends here: a build case, a host
/// that cannot boot it, a build that fails.
fn build_case<'a>(
    builder: &Builder,
    case: &'a Case,
    target: &'static Target,
    logs: &Path,
    missing: &dyn Fn(String) -> Outcome,
) -> Result<Result<Built<'a>, Results>> {
    let started = Instant::now();
    let elapsed = |since: Instant| since.elapsed().as_secs_f32();

    let boot = match &case.kind {
        Kind::Build(build) => {
            let outcome = match builder.cargo_build(target, &build.package, &build.features, Profile::Release)
            {
                Ok(()) => Outcome::Pass,
                Err(e) => Outcome::Fail(format!("{e:#}")),
            };
            return Ok(Err(vec![(String::new(), outcome, elapsed(started))]));
        }
        Kind::Boot(boot) => boot,
        Kind::UnsafeBudget(_)
        | Kind::SizeBudget(_)
        | Kind::NoCruft(_)
        | Kind::Fmt(_)
        | Kind::SshLoopback(_)
        | Kind::Elixir(_)
        | Kind::HostTests(_) => {
            unreachable!("handled before the per-target loop")
        }
    };
    let machine = match &target.machine {
        Ok(machine) => machine,
        Err(why) => return Ok(Err(vec![(String::new(), Outcome::Skip(why.to_string()), 0.0)])),
    };
    let firmware = match rustsbi_prototyper(target) {
        Ok(firmware) => firmware,
        Err(why) => return Ok(Err(vec![(String::new(), missing(why), 0.0)])),
    };
    if let Err(why) = qemu::usable(machine.qemu) {
        return Ok(Err(vec![(String::new(), missing(why), 0.0)]));
    }
    if !boot.session.is_empty() {
        if let Err(why) = ssh_available() {
            return Ok(Err(vec![(String::new(), missing(why), 0.0)]));
        }
    }

    // Build everything once, then boot it once per hart count. A build failure is the bench's
    // or the code's problem, never what a `must_fail` is waiting for, so it is not judged.
    let bundle = logs.join(format!("{}-{}.tar", case.name, target.name));
    let profile = if boot.debug_assertions { Profile::Checked } else { Profile::Release };
    let (bundle, loader) = match prepare(
        builder,
        target,
        machine,
        &boot.programs,
        &boot.file,
        boot.userland.as_ref(),
        &boot.kernel_features,
        boot.tamper_bundle,
        boot.sign_bare_archive,
        profile,
        &bundle,
    ) {
        Ok(built) => built,
        Err(e) => return Ok(Err(vec![(String::new(), Outcome::Fail(format!("{e:#}")), elapsed(started))])),
    };
    let userland = match boot.userland.as_ref().map(|u| builder.userland(&u.recipe)).transpose() {
        Ok(staged) => staged,
        Err(e) => return Ok(Err(vec![(String::new(), Outcome::Fail(format!("{e:#}")), elapsed(started))])),
    };
    Ok(Ok(Built { boot, name: &case.name, target, machine, firmware, loader, bundle, userland }))
}

/// Boot a built case once per hart count with guest seed `seed`, keeping its files in `logs`.
/// `out` takes the lines it prints before its results.
fn boot_case(
    builder: &Builder,
    built: &Built,
    seed: Option<u64>,
    logs: &Path,
    out: &mut dyn FnMut(String),
) -> Result<Results> {
    let Built { boot, name, target, machine, firmware, loader, bundle, userland } = built;
    let elapsed = |since: Instant| since.elapsed().as_secs_f32();
    if let Some(seed) = seed {
        out(format!("      qemu seed {seed} (TESTBENCH_QEMU_SEED={seed} replays it)"));
    }
    let mut results = Vec::new();
    for smp in &boot.smp {
        let run_started = Instant::now();
        let log = boot_log(logs, name, target.name, *smp);
        // Every boot gets fresh devices: a new disk, new host ports.
        let boot_once = |log: &Path| -> Result<Verdict> {
            let disk = log.with_extension("img");
            let (mut devices, forwards) = qemu::virtio_devices(boot, &disk, userland.as_ref())?;
            if let Some(icount) = &boot.icount {
                devices.extend(["-icount".into(), icount.clone(), "-rtc".into(), "clock=vm".into()]);
            }
            if let Some(seed) = seed {
                devices.extend(["-seed".into(), seed.to_string()]);
            }
            let image = Image {
                machine,
                firmware,
                loader,
                bundle,
                smp: *smp,
                memory_mib: boot.memory_mib.unwrap_or(target::DEFAULT_MEMORY_MIB),
                devices: &devices,
            };
            // The network's far side dials in while it boots (`peer.rs`); its peers and capture are
            // judged by the post-check below.
            let deadline = Instant::now() + std::time::Duration::from_secs_f64(boot.timeout_secs);
            let dials =
                boot.net.as_ref().map(|net| peer::Dials::start(net, &forwards, deadline)).transpose()?;
            let verdict = qemu::run(&image, boot, &builder.workspace, &forwards, log)?;
            Ok(peer::finish_dials(verdict, dials))
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
        let outcome = match outcome {
            Outcome::Pass if !boot.distinct.is_empty() => {
                let text = std::fs::read(&log).with_context(|| format!("reading {}", log.display()))?;
                match distinct_within(&boot.distinct, &String::from_utf8_lossy(&text)) {
                    Ok(()) => Outcome::Pass,
                    Err(why) => Outcome::Fail(why),
                }
            }
            other => other,
        };
        // Cases without a `post_check` or peers are judged exactly as before.
        let peers = boot.net.as_ref().is_some_and(|net| !net.peer.is_empty());
        let outcome = match outcome {
            Outcome::Pass if boot.post_check.is_some() || peers => post_check(boot, &log, out)?,
            other => other,
        };
        results.push((
            format!(", smp={smp}"),
            judge(boot.must_fail.as_deref(), outcome)?,
            elapsed(run_started),
        ));
    }
    Ok(results)
}

/// The guest seed of a case that pins one (`qemu_seed`), replaced by TESTBENCH_QEMU_SEED when set.
fn qemu_seed(boot: &case::Boot) -> Result<Option<u64>> {
    let Some(pinned) = boot.qemu_seed else { return Ok(None) };
    match std::env::var("TESTBENCH_QEMU_SEED") {
        Ok(seed) => Ok(Some(seed.parse().with_context(|| format!("TESTBENCH_QEMU_SEED={seed:?}"))?)),
        Err(_) => Ok(Some(pinned)),
    }
}

/// Run a case's `post_check` (a name, then its arguments) over the console log of a boot that
/// passed. `out` takes the line a check prints.
fn post_check(boot: &case::Boot, log: &Path, out: &mut dyn FnMut(String)) -> Result<Outcome> {
    // A network with peers: the peers' counts and the capture of this boot (`peer.rs`).
    if let Some(net) = boot.net.as_ref().filter(|net| !net.peer.is_empty()) {
        if let Err(why) = peer::judge_peers(net, &peer::Files::beside(&log.with_extension("img"))) {
            return Ok(Outcome::Fail(why));
        }
    }
    let text = std::fs::read(log).with_context(|| format!("reading {}", log.display()))?;
    let text = String::from_utf8_lossy(&text);
    let check = boot.post_check.as_deref().unwrap_or("");
    let (name, args) = check.split_once(' ').unwrap_or((check, ""));
    Ok(match (!check.is_empty()).then_some(name) {
        Some("sched_oracle") => match sched_oracle::run(&text, args) {
            Ok(summary) => {
                out(format!("      {summary}"));
                Outcome::Pass
            }
            Err(why) => Outcome::Fail(format!("sched_oracle: {why}")),
        },
        Some(other) => bail!("unknown post_check {other:?}"),
        None => Outcome::Pass,
    })
}

/// Apply a case's `must_fail` to the verdict of its run: then the case passes only if the run
/// failed, and for the named reason, so a self-check cannot pass by failing for some other one.
fn judge(must_fail: Option<&str>, outcome: Outcome) -> Result<Outcome> {
    let Some(pattern) = must_fail else { return Ok(outcome) };
    let pattern =
        regex::Regex::new(pattern).with_context(|| format!("bad regular expression {pattern:?}"))?;
    Ok(match outcome {
        Outcome::Fail(why) if pattern.is_match(&why) => Outcome::Pass,
        Outcome::Fail(why) => Outcome::Fail(format!("failed, but not with /{pattern}/: {why}")),
        Outcome::Pass => Outcome::Fail(format!("passed, but must fail with /{pattern}/")),
        skip @ Outcome::Skip(_) => skip,
    })
}

/// SSH sessions need OpenSSH's client on the host.
fn ssh_available() -> Result<(), String> {
    match Command::new(ssh::SSH).arg("-V").stderr(Stdio::null()).status() {
        Ok(status) if status.success() => Ok(()),
        _ => Err(format!("OpenSSH's `{}` is not installed", ssh::SSH)),
    }
}

/// Run an `ssh-loopback` case: its sessions against a host server accepting its keys. An error
/// is the bench's own trouble; the outcome is the sessions' verdict.
fn ssh_loopback(workspace: &Path, case: &Case, loopback: &case::SshLoopback, logs: &Path) -> Result<Outcome> {
    let deadline = Instant::now() + std::time::Duration::from_secs_f64(loopback.timeout_secs);
    let serve = match loopback.server {
        LoopbackServer::Redoubt => ssh::redoubt,
        LoopbackServer::Openssh => ssh::loopback,
    };
    let server =
        serve(workspace, &logs.join("ssh"), &case.name, &loopback.authorized, loopback.host_key.as_deref())?;
    let abort = std::sync::atomic::AtomicBool::new(false);
    let sessions = ssh::run(workspace, &loopback.session, &server, logs, &case.name, deadline, &abort)?;
    // What the server saw comes first: a case that fails as its `must_fail` expects still fails if
    // the server did not see it that way.
    let server_log = ssh::loopback_log(&logs.join("ssh"), &case.name);
    let server_log = std::fs::read_to_string(&server_log)
        .with_context(|| format!("reading the server's log {}", server_log.display()))?;
    for pattern in &loopback.server_log {
        let re = regex::Regex::new(pattern)?;
        if !server_log.lines().any(|line| re.is_match(line)) {
            return Ok(Outcome::Fail(format!("the server's log has no line matching /{pattern}/")));
        }
    }
    for pattern in &loopback.server_log_forbid {
        let re = regex::Regex::new(pattern)?;
        if let Some(line) = server_log.lines().find(|line| re.is_match(line)) {
            return Ok(Outcome::Fail(format!("the server's log has a forbidden line /{pattern}/: {line}")));
        }
    }
    Ok(match sessions {
        None => Outcome::Pass,
        Some(why) => Outcome::Fail(why),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `distinct` pattern passes only on two or more captures, all different.
    #[test]
    fn distinct_captures_differ_within_a_boot() {
        let pattern = ["^init: (?:started|restarted) b, console ([0-9a-f]+)$".to_string()];
        let log = |lines: &[&str]| lines.join("\n");
        let two = log(&["init: started b, console 1a", "x", "init: restarted b, console 2b"]);
        assert_eq!(distinct_within(&pattern, &two), Ok(()));
        let same = log(&["init: started b, console 1a", "init: restarted b, console 1a"]);
        assert!(distinct_within(&pattern, &same).unwrap_err().contains("twice"));
        let one = log(&["init: started b, console 1a"]);
        assert!(distinct_within(&pattern, &one).unwrap_err().contains("not two"));
        assert!(distinct_within(&pattern, "").is_err());
    }

    /// SEEDS is an inclusive range or a comma list; empty, reversed and repeated seeds are refused.
    #[test]
    fn sweep_seeds_are_ranges_or_lists() {
        assert_eq!(parse_seeds("1..4"), Ok(Seeds(vec![1, 2, 3, 4])));
        assert_eq!(parse_seeds("7..7"), Ok(Seeds(vec![7])));
        assert_eq!(parse_seeds("3,5,9"), Ok(Seeds(vec![3, 5, 9])));
        assert_eq!(parse_seeds("12"), Ok(Seeds(vec![12])));
        assert_eq!(parse_seeds("1..10000").map(|Seeds(s)| s.len()), Ok(10_000));
        for (refused, why) in [
            ("", "is not a seed"),
            ("..", "is not a seed"),
            ("3,", "is not a seed"),
            ("4..1", "reversed"),
            ("3,5,3", "twice"),
            ("x..2", "is not a seed"),
            ("0..18446744073709551615", "more than 10000 seeds"),
            ("1..10001", "more than 10000 seeds"),
        ] {
            let err = parse_seeds(refused).unwrap_err();
            assert!(err.contains(why), "{refused:?}: {err}");
        }
    }

    fn case(name: &str, fields: &str) -> Case {
        let mut case: Case = toml::from_str(&format!("description = 'x'\narch = ['rv64', 'rv32']\n{fields}"))
            .expect("a test case's TOML");
        case.name = name.into();
        case
    }

    fn args(argv: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("testbench").chain(argv.iter().copied()))
    }

    /// A sweep names one case that pins its seed; `--jobs` comes only with it, from 1 to the
    /// host's parallelism; TESTBENCH_QEMU_SEED is refused beside it, and so is an `--arch` the
    /// case lacks; a run without it is no sweep.
    #[test]
    fn a_sweep_is_refused_before_anything_builds() {
        let seeded = "kind = 'boot'\nprograms = []\nexpect = []\nqemu_seed = 1\n";
        let cases = [
            case("sched", seeded),
            case("sched-ties", seeded),
            case("timer", "kind = 'boot'\nprograms = []\nexpect = []\n"),
        ];
        let plan = |argv: &[&str], replay: Option<&str>| {
            sweep(&args(argv).unwrap(), &cases, replay.map(String::from), 4)
                .map(|s| s.map(|s| (&s.case.name, s.jobs)))
        };
        let refused = |argv: &[&str], replay: Option<&str>, why: &str| {
            let err = format!("{:#}", plan(argv, replay).err().unwrap_or_else(|| panic!("{argv:?} ran")));
            assert!(err.contains(why), "{argv:?}: {err}");
        };

        assert_eq!(plan(&["sched"], None).unwrap(), None);
        assert_eq!(plan(&["sched", "--sweep", "1..4"], None).unwrap(), Some((&"sched".to_string(), 1)));
        assert_eq!(
            plan(&["ties", "--sweep", "1..4", "--jobs", "4"], None).unwrap(),
            Some((&"sched-ties".to_string(), 4))
        );
        assert!(args(&["sched", "--jobs", "2"]).is_err(), "--jobs without --sweep");
        refused(&["sched", "--sweep", "1..4"], Some("3"), "TESTBENCH_QEMU_SEED");
        refused(&["sched", "--sweep", "1..4", "--jobs", "5"], None, "parallelism, 4");
        refused(&["sched", "--sweep", "1..4", "--jobs", "0"], None, "parallelism");
        refused(&["--sweep", "1..4"], None, "name it");
        refused(&["sch", "--sweep", "1..4"], None, "names 2");
        refused(&["nothing", "--sweep", "1..4"], None, "names 0");
        refused(&["timer", "--sweep", "1..4"], None, "qemu_seed");
        refused(&["sched", "--sweep", "1..4", "--arch", "x86"], None, "not one of its targets (rv64, rv32)");
        assert_eq!(
            plan(&["sched", "--sweep", "1..4", "--arch", "rv32"], None).unwrap(),
            Some((&"sched".to_string(), 1))
        );
    }

    /// No two boots of a sweep share a file: each seed and target has a directory of its own, and
    /// within it each hart count a log, a disk and captures of its own.
    #[test]
    fn sweep_boots_have_their_own_files() {
        let logs = Path::new("/run");
        let mut seen = HashSet::new();
        for seed in [1, 2, 10, 11, 21, 111] {
            for arch in ["rv64", "rv32"] {
                let dir = seed_dir(logs, seed, arch);
                assert_eq!(dir.parent(), Some(logs));
                assert!(seen.insert(dir.clone()), "{} twice", dir.display());
                for smp in [1, 2, 4] {
                    let log = boot_log(&dir, "sched", arch, smp);
                    assert_eq!(log.parent(), Some(dir.as_path()));
                    for file in
                        [log.clone(), log.with_extension("img"), log.with_extension("second-boot.log")]
                    {
                        assert!(seen.insert(file.clone()), "{} twice", file.display());
                    }
                }
            }
        }
        assert_eq!(seed_dir(logs, 3, "rv64"), Path::new("/run/seed-3-rv64"));
    }

    fn seed_run(seed: u64, arch: &'static str, outcome: Outcome) -> SeedRun {
        let notes = vec![format!("      qemu seed {seed} (TESTBENCH_QEMU_SEED={seed} replays it)")];
        SeedRun { seed, arch, notes, results: Ok(vec![(", smp=1".into(), outcome, 1.0)]) }
    }

    /// The join prints each seed as a single run would, in seed order then by target, whatever
    /// order they finished in; then each target's summary, naming the failed seeds, and counts
    /// the failures, which make the run's exit status non-zero.
    #[test]
    fn the_join_prints_in_seed_order_and_counts_failures() {
        let runs = vec![
            seed_run(3, "rv32", Outcome::Pass),
            seed_run(10, "rv64", Outcome::Pass),
            seed_run(3, "rv64", Outcome::Fail("timed out".into())),
            seed_run(1, "rv32", Outcome::Pass),
            seed_run(1, "rv64", Outcome::Pass),
            SeedRun { seed: 10, arch: "rv32", notes: vec![], results: Err(anyhow::anyhow!("no disk")) },
        ];
        let (text, failures) = join("sched", &["rv64", "rv32"], runs);
        let lines: Vec<&str> = text.lines().collect();
        let seed_lines: Vec<&str> =
            lines.iter().filter(|l| l.contains("qemu seed")).map(|l| l.trim()).collect();
        assert_eq!(
            seed_lines,
            [1, 1, 3, 3, 10].map(|s| format!("qemu seed {s} (TESTBENCH_QEMU_SEED={s} replays it)")),
        );
        let results: Vec<&str> =
            lines.iter().filter_map(|l| l.split_once("sched [").map(|(_, r)| r)).collect();
        assert_eq!(results.len(), 6);
        assert!(results[0].starts_with("rv64, smp=1]") && lines[1].starts_with("PASS"), "{text}");
        assert!(results[1].starts_with("rv32, smp=1]"), "{text}");
        assert!(lines[5].starts_with("FAIL") && lines[5].ends_with("timed out"), "{text}");
        assert!(lines[10].starts_with("FAIL") && lines[10].contains("bench error: no disk"), "{text}");
        assert_eq!(lines[1], format!("PASS  {:<32}   1.0s", "sched [rv64, smp=1]"));
        assert_eq!(
            &lines[11..],
            [
                "sweep sched rv64: 3 seeds, 2 passed, 1 failed: 3",
                "sweep sched rv32: 3 seeds, 2 passed, 1 failed: 10"
            ]
        );
        assert_eq!(failures, 2);

        let (text, failures) = join("sched", &["rv64"], vec![seed_run(2, "rv64", Outcome::Pass)]);
        assert_eq!(failures, 0);
        assert!(text.ends_with("sweep sched rv64: 1 seed, 1 passed, 0 failed\n"), "{text}");
    }
}
