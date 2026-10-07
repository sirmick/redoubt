//! A `host-tests` case as many jobs, one per `each` of its `fanout`: a value set in a variable
//! for the case's one test file, a test file, or a test. The tests are built once, first; then
//! each job runs through the machine's scheduler, `scripts/q`, on the cores it asks for, as many
//! at once as the machine has room for, or, where `q` does not answer, one after another. Each
//! job's output is a file of its own in the run's directory, and the case's log gives every value
//! its time; the case passes when every job does, and its failure names each value whose job
//! failed.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::build::{Builder, TestBinary, last_lines, libtest_args};
use crate::case::{Each, Fanout, HostTests};

/// The exit statuses of a command `timeout` ended, here and for an Elixir case: by TERM, or by
/// KILL after its grace.
pub const TIMED_OUT: [i32; 2] = [124, 137];

/// Seconds `timeout` waits after TERM before it kills.
pub const KILL_AFTER: &str = "10";

/// One job: run `argv` from `dir` with `vars` set.
struct Job {
    value: String,
    dir: PathBuf,
    vars: Vec<(String, String)>,
    argv: Vec<String>,
}

/// One job, as it ended.
struct Ended {
    value: String,
    failure: Option<String>,
    /// From its start, any wait for its core included, to its end.
    wall: f32,
    /// What its tests took, by libtest's `finished in`, over every binary it ran.
    tests: f32,
}

/// Run `host`'s jobs: `Ok(None)` if every one passed, else what failed.
pub fn run(
    builder: &Builder,
    name: &str,
    host: &HostTests,
    fanout: &Fanout,
    workspace: &Path,
) -> Result<Option<String>> {
    let jobs = match jobs(builder, host, fanout, workspace)? {
        Ok(jobs) if jobs.is_empty() => return Ok(Some("the fanout has no jobs".into())),
        Ok(jobs) => jobs,
        Err(why) => return Ok(Some(why)),
    };

    let dir = builder.run.join(name);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let q = scheduler(workspace);
    // Enough clients to fill the machine; more would only wait in q's queue.
    let at_once = if q.is_some() { (online_cpus() / fanout.cores as usize).max(1) } else { 1 };
    let count = jobs.len();
    let queue = Mutex::new(jobs.into_iter());
    let ended = Mutex::new(Vec::new());
    let started = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..at_once.min(count) {
            s.spawn(|| {
                loop {
                    // Taken apart from the loop's test, whose lock would last the whole job.
                    let next = queue.lock().unwrap().next();
                    let Some(job) = next else { return };
                    let one =
                        run_one(job, name, q.as_deref(), fanout.cores, host.timeout_secs, workspace, &dir);
                    ended.lock().unwrap().push(one);
                }
            });
        }
    });
    let mut ended = ended.into_inner().unwrap();
    ended.sort_by(|a, b| b.tests.total_cmp(&a.tests));

    let how = match &q {
        Some(_) => format!("{at_once} at once through scripts/q"),
        None => "one at a time".into(),
    };
    let mut log = format!("{count} jobs, {how}: {:.1}s\n", started.elapsed().as_secs_f32());
    for e in &ended {
        let verdict = if e.failure.is_some() { "FAIL" } else { "PASS" };
        log.push_str(&format!("{verdict}  {:<40} {:7.1}s  (wall {:.1}s)\n", e.value, e.tests, e.wall));
    }
    let path = builder.run.join(format!("{name}.log"));
    std::fs::write(&path, log).with_context(|| format!("writing {}", path.display()))?;

    let failed: Vec<_> = ended.iter().filter(|e| e.failure.is_some()).collect();
    let Some(first) = failed.first() else { return Ok(None) };
    let names: Vec<_> = failed.iter().map(|e| e.value.as_str()).collect();
    Ok(Some(format!(
        "{} of {count} jobs failed: {}\n      {}: {}\n      (each job's output: {})",
        failed.len(),
        names.join(", "),
        first.value,
        first.failure.as_deref().unwrap_or_default(),
        dir.display()
    )))
}

/// The case's jobs, after one build of its tests, or what failed.
fn jobs(
    builder: &Builder,
    host: &HostTests,
    fanout: &Fanout,
    workspace: &Path,
) -> Result<Result<Vec<Job>, String>> {
    let binaries = match builder.test_binaries(host, workspace, &host.tests)? {
        Ok(binaries) => binaries,
        Err(why) => return Ok(Err(format!("building the tests failed:\n      {why}"))),
    };
    let vars: Vec<(String, String)> =
        fanout.vars.iter().map(|(key, var)| (key.clone(), var.clone())).collect();
    let mut jobs = Vec::new();
    let values = match fanout.each {
        Each::Test => {
            for binary in &binaries {
                let tests = match list(binary)? {
                    Ok(tests) => tests,
                    Err(why) => return Ok(Err(why)),
                };
                let wanted = |t: &String| {
                    (host.filter.is_empty() || host.filter.iter().any(|f| t.contains(f.as_str())))
                        && !host.skip.iter().any(|skip| t.contains(skip.as_str()))
                };
                for test in tests.into_iter().filter(wanted) {
                    let value = format!("{}::{test}", binary.name);
                    jobs.push(native(binary, value, vec![test, "--exact".into()], vars.clone()));
                }
            }
            return Ok(Ok(jobs));
        }
        Each::File => host.tests.clone(),
        Each::Value => match self::values(fanout, workspace)? {
            Ok(values) => values,
            Err(why) => return Ok(Err(why)),
        },
    };
    let libtest = libtest_args(host);
    for value in values {
        let mut vars = vars.clone();
        vars.extend(fanout.env.iter().map(|env| (env.clone(), value.clone())));
        let file = if fanout.each == Each::Value { &host.tests[0] } else { &value };
        // Miri's test binaries run only under `cargo miri`, which shares the build.
        if host.miri {
            let cargo = builder.test_command(host, workspace, std::slice::from_ref(file), &["--quiet"]);
            let mut argv = vec![cargo.get_program().to_string_lossy().into_owned()];
            argv.extend(cargo.get_args().map(|a| a.to_string_lossy().into_owned()));
            for (key, var) in cargo.get_envs() {
                let var = var.map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
                vars.push((key.to_string_lossy().into_owned(), var));
            }
            jobs.push(Job { value, dir: workspace.to_path_buf(), vars, argv });
            continue;
        }
        let Some(binary) = binaries.iter().find(|b| &b.name == file) else {
            return Ok(Err(format!("cargo built no test binary {file}")));
        };
        jobs.push(native(binary, value, libtest.clone(), vars));
    }
    Ok(Ok(jobs))
}

/// The lines the fanout's `values` command prints, or why it failed.
fn values(fanout: &Fanout, workspace: &Path) -> Result<Result<Vec<String>, String>> {
    let command = fanout.values.as_deref().unwrap_or_default();
    let output = Command::new("sh")
        .args(["-c", command])
        .current_dir(workspace)
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running `{command}`"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Ok(Err(format!("`{command}` failed:\n      {}", last_lines(&err, 12))));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(Ok(text.lines().map(str::trim).filter(|line| !line.is_empty()).map(String::from).collect()))
}

/// The tests a binary holds, by libtest's own list, or why it would not say.
fn list(binary: &TestBinary) -> Result<Result<Vec<String>, String>> {
    let output = Command::new(&binary.path)
        .args(["--list", "--format", "terse"])
        .current_dir(&binary.dir)
        .output()
        .with_context(|| format!("listing {}", binary.path.display()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Ok(Err(format!("listing {}'s tests failed:\n      {}", binary.name, last_lines(&err, 12))));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(Ok(text.lines().filter_map(|line| line.strip_suffix(": test")).map(String::from).collect()))
}

/// A job running `binary` with `args`, as `cargo test` runs it: from its package's directory,
/// which it names.
fn native(binary: &TestBinary, value: String, args: Vec<String>, mut vars: Vec<(String, String)>) -> Job {
    vars.push(("CARGO_MANIFEST_DIR".into(), binary.dir.to_string_lossy().into_owned()));
    let mut argv = vec![binary.path.to_string_lossy().into_owned()];
    argv.extend(args);
    Job { value, dir: binary.dir.clone(), vars, argv }
}

/// Run one job, its output to its own file in `dir`: through `q` on `cores` cores when it is
/// given, under `timeout` when the case has a deadline.
fn run_one(
    job: Job,
    case: &str,
    q: Option<&Path>,
    cores: u32,
    timeout: Option<f64>,
    workspace: &Path,
    dir: &Path,
) -> Ended {
    let safe: String = job
        .value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' })
        .collect();
    let log = dir.join(format!("{safe}.log"));
    let mut argv: Vec<String> = Vec::new();
    if let Some(q) = q {
        let name = format!("{case}/{}", job.value);
        argv.extend([q.to_string_lossy().into_owned(), "run".into(), "--cores".into(), cores.to_string()]);
        argv.extend(["--name".into(), name, "--".into()]);
    }
    if let Some(secs) = timeout {
        argv.extend(["timeout".into(), "-k".into(), KILL_AFTER.into(), format!("{secs}")]);
    }
    // `env` moves into the job's directory and sets its variables under q and `timeout` alike;
    // `q` itself runs from the workspace, whose name is the tenant it bills.
    argv.extend(["env".into(), "-C".into(), job.dir.to_string_lossy().into_owned()]);
    argv.extend(job.vars.iter().map(|(key, var)| format!("{key}={var}")));
    argv.extend(job.argv);

    let started = Instant::now();
    let status = File::create(&log).and_then(|out| {
        Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(workspace)
            .stdout(out.try_clone()?)
            .stderr(out)
            .status()
    });
    let wall = started.elapsed().as_secs_f32();
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let tests = text
        .lines()
        .filter_map(|line| line.rsplit_once("finished in ")?.1.strip_suffix('s')?.parse::<f32>().ok())
        .sum();
    let failure = match status {
        Err(e) => Some(format!("running it: {e}")),
        Ok(status) if status.success() => None,
        Ok(status) if timeout.is_some() && status.code().is_some_and(|c| TIMED_OUT.contains(&c)) => {
            Some(format!("ran past its deadline of {} s", timeout.unwrap_or_default()))
        }
        Ok(status) => Some(format!("{status}:\n      {}", last_lines(&text, 12))),
    };
    Ended { value: job.value, failure, wall, tests }
}

/// The machine's scheduler, `scripts/q`, if the workspace has it and its daemon answers.
fn scheduler(workspace: &Path) -> Option<PathBuf> {
    let q = workspace.join("scripts/q");
    let answers = Command::new(&q)
        .arg("ping")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    answers.then_some(q)
}

/// The machine's online CPUs (`/sys/devices/system/cpu/online`, such as `0-23`): not this
/// process's affinity, which under `q` is its own lease.
fn online_cpus() -> usize {
    std::fs::read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .and_then(|list| cpus(&list))
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// How many CPUs a kernel CPU list (`0-3,8,10-11`) names.
fn cpus(list: &str) -> Option<usize> {
    list.trim()
        .split(',')
        .map(|range| match range.split_once('-') {
            Some((first, last)) => (last.parse::<usize>().ok()? + 1).checked_sub(first.parse().ok()?),
            None => range.parse::<usize>().ok().map(|_| 1),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cpu_list_counts_its_ranges() {
        assert_eq!(cpus("0-23\n"), Some(24));
        assert_eq!(cpus("0-3,8,10-11"), Some(7));
        assert_eq!(cpus("x"), None);
    }

    /// A job's verdict is its exit status, its deadline's expiry is said as such, and its time is
    /// what its tests said they took.
    #[test]
    fn a_job_passes_fails_or_runs_past_its_deadline() {
        let dir = std::env::temp_dir().join(format!("testbench-fanout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let job = |value: &str, script: &str| Job {
            value: value.into(),
            dir: dir.clone(),
            vars: vec![("V".into(), value.into())],
            argv: vec!["sh".into(), "-c".into(), script.into()],
        };
        let one = |job, timeout| run_one(job, "c", None, 1, timeout, &dir, &dir);

        let pass = one(job("a", "echo \"$V finished in 1.25s\"; echo finished in 0.5s"), None);
        assert!(pass.failure.is_none());
        assert_eq!(pass.tests, 1.75);
        assert!(std::fs::read_to_string(dir.join("a.log")).unwrap().starts_with("a finished"));

        let fail = one(job("b/c", "echo not caught; exit 3"), Some(30.0));
        let why = fail.failure.unwrap();
        assert!(why.contains("exit status: 3") && why.contains("not caught"), "{why}");
        assert!(dir.join("b_c.log").is_file(), "a value's file name is a safe one");

        let late = one(job("d", "sleep 5"), Some(0.2));
        assert_eq!(late.failure.as_deref(), Some("ran past its deadline of 0.2 s"));
        assert!(late.wall < 4.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
