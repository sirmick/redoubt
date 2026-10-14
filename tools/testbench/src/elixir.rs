//! The `elixir` kind: scripts that run Elixir oracles on beamlet under the pinned OTP and Elixir
//! (`userland/otp/tools/env.sh`), each judged by its exit status. The toolchain is checked first,
//! and a missing or wrong one fails the case even under --allow-skip: an oracle that does not run
//! catches nothing, and one on another version is not the one pinned.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::case::Elixir;
use crate::fanout::{KILL_AFTER, TIMED_OUT};

/// Lines of a failed script's output shown in the result; the whole of it is in the log.
const SHOWN: usize = 80;

/// The deadline of a case's builds together. A cold beamlet and the model's trace writer take
/// 29 s alone on four cores and up to 46 s beside a whole bench; this bounds only a hung build.
const BUILD_SECS: f64 = 600.0;

/// Run the case's builds, then its scripts: `Ok(None)` if every one exits 0, `Ok(Some(why))` for
/// the first script that fails, which `must_fail` may wait for. A failed build, a build past
/// [`BUILD_SECS`] and a script past the case's deadline, which starts when the builds end, are
/// `Err`: never what a `must_fail` waits for. The output goes to `log`, with what the builds and
/// the scripts each took.
pub fn run(workspace: &Path, case: &Elixir, log: &Path) -> Result<Option<String>> {
    run_within(workspace, case, log, BUILD_SECS)
}

/// `run`, with the builds' deadline `build_secs`.
fn run_within(workspace: &Path, case: &Elixir, log: &Path, build_secs: f64) -> Result<Option<String>> {
    let mut all = String::new();
    let result = builds(workspace, case, log, &mut all, build_secs).and_then(|()| {
        let started = Instant::now();
        let result = scripts(workspace, case, log, &mut all, started);
        all.push_str(&format!("# the scripts took {:.1} s\n", started.elapsed().as_secs_f64()));
        result
    });
    std::fs::write(log, &all).with_context(|| format!("writing {}", log.display()))?;
    result
}

/// The builds of `run`, under `secs` together; their output goes to `all`.
fn builds(workspace: &Path, case: &Elixir, log: &Path, all: &mut String, secs: f64) -> Result<()> {
    let started = Instant::now();
    for build in &case.build {
        let mut words = build.split_whitespace();
        let path = words.next().context("an empty build")?;
        let left = secs - started.elapsed().as_secs_f64();
        if left < 0.1 {
            bail!("build {build}: timed out: the builds' deadline is {secs} s");
        }
        let out = Command::new("timeout")
            .args(["-k", KILL_AFTER, &format!("{left:.1}")])
            .arg(workspace.join(path))
            .args(words)
            .current_dir(workspace)
            .output()
            .with_context(|| format!("running {path}"))?;
        all.push_str(&format!(
            "$ {build}\n{}{}\n",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
        if out.status.code().is_some_and(|c| TIMED_OUT.contains(&c)) {
            bail!("build {build}: timed out: the builds' deadline is {secs} s");
        }
        if !out.status.success() {
            bail!("build {build}: {} (the whole output: {})", out.status, log.display());
        }
    }
    all.push_str(&format!("# the builds took {:.1} s\n", started.elapsed().as_secs_f64()));
    Ok(())
}

/// The scripts of `run`, under the deadline counted from `started`; their output goes to `all`.
fn scripts(
    workspace: &Path,
    case: &Elixir,
    log: &Path,
    all: &mut String,
    started: Instant,
) -> Result<Option<String>> {
    for script in &case.scripts {
        let mut words = script.split_whitespace();
        let path = words.next().context("an empty script")?;
        // Each script under `timeout`, given what is left of the case's deadline.
        let mut command = match case.timeout_secs {
            Some(secs) => {
                let left = secs - started.elapsed().as_secs_f64();
                // `timeout 0` would be no deadline at all.
                if left < 0.1 {
                    bail!("{script}: ran past the case's deadline of {secs} s");
                }
                let mut timeout = Command::new("timeout");
                timeout.args(["-k", KILL_AFTER, &format!("{left:.1}")]).arg(workspace.join(path));
                timeout
            }
            None => Command::new(workspace.join(path)),
        };
        let out =
            command.args(words).current_dir(workspace).output().with_context(|| format!("running {path}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        all.push_str(&format!("$ {script}\n{stdout}{}\n", String::from_utf8_lossy(&out.stderr)));
        if let Some(secs) =
            case.timeout_secs.filter(|_| out.status.code().is_some_and(|c| TIMED_OUT.contains(&c)))
        {
            bail!("{script}: ran past the case's deadline of {secs} s");
        }
        if !out.status.success() {
            let shown: Vec<_> = stdout.lines().take(SHOWN).collect();
            return Ok(Some(format!(
                "{script}: {}\n      {}\n      (the whole output: {})",
                out.status,
                shown.join("\n      "),
                log.display()
            )));
        }
    }
    Ok(None)
}

/// `Err` unless `env.sh` puts on the path an `erl` whose release is `case.otp` and an `elixir`
/// that says it is `case.elixir`.
pub fn toolchain(workspace: &Path, case: &Elixir) -> Result<(), String> {
    let shell = |command: &str| -> Result<String, String> {
        let out = Command::new("bash")
            .current_dir(workspace)
            .args(["-c", &format!(". userland/otp/tools/env.sh && {command}")])
            .output()
            .map_err(|e| format!("running bash: {e}"))?;
        if !out.status.success() {
            return Err(format!("`{command}` failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let erl = shell("command -v erl")
        .map_err(|_| "no `erl` on the path userland/otp/tools/env.sh sets".to_string())?;
    // A release root: bin/erl, and releases/<major>/OTP_VERSION beside bin/.
    let major = case.otp.split('.').next().unwrap_or_default();
    let release = Path::new(&erl).parent().and_then(Path::parent).unwrap_or(Path::new("/"));
    let file = release.join("releases").join(major).join("OTP_VERSION");
    let otp = std::fs::read_to_string(&file).map_err(|e| format!("{erl}: no {}: {e}", file.display()))?;
    if otp.trim() != case.otp {
        return Err(format!("{erl} is OTP {}, not the pinned {}", otp.trim(), case.otp));
    }
    let version = shell("elixir --version")?;
    let pinned = format!("Elixir {} ", case.elixir);
    if !version.lines().any(|line| line.starts_with(&pinned)) {
        return Err(format!("`elixir --version` does not say {}: {version:?}", pinned.trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any OTP or Elixir but the pinned one fails the check, whatever is installed.
    #[test]
    fn another_version_is_refused() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pinned = |otp: &str, elixir: &str| Elixir {
            otp: otp.into(),
            elixir: elixir.into(),
            build: Vec::new(),
            scripts: Vec::new(),
            must_fail: None,
            timeout_secs: None,
        };
        assert!(toolchain(&workspace, &pinned("28.0.0.1", "1.20.4")).is_err());
        assert!(toolchain(&workspace, &pinned("28.5.0.6", "1.20.3")).is_err());
    }

    /// The deadline is the case's, over its scripts together: a second script gets what the first
    /// left, and one past it fails saying so.
    #[test]
    fn the_scripts_share_the_case_deadline() {
        let dir = std::env::temp_dir().join(format!("testbench-elixir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            std::fs::write(dir.join(name), format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(dir.join(name), std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        };
        script("quick", "sleep 0.3");
        let case = |scripts: &[&str], timeout_secs| Elixir {
            otp: String::new(),
            elixir: String::new(),
            build: Vec::new(),
            scripts: scripts.iter().map(|s| s.to_string()).collect(),
            must_fail: None,
            timeout_secs,
        };
        let log = dir.join("log");
        assert_eq!(run(&dir, &case(&["quick", "quick"], Some(5.0)), &log).unwrap(), None);
        let late = run(&dir, &case(&["quick", "quick"], Some(0.4)), &log).unwrap_err();
        assert_eq!(late.to_string(), "quick: ran past the case's deadline of 0.4 s");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The deadline starts when the builds end: a build longer than the deadline does not time the
    /// case out, a script past it still does, and a failed build fails as a build. The builds have
    /// a deadline of their own. A failed or late build and a late script are errors, which main
    /// reports failed without judging them: so even a `must_fail` that any text matches cannot
    /// take them for the failure it waits for.
    #[test]
    fn the_deadline_starts_after_the_build() {
        let dir = std::env::temp_dir().join(format!("testbench-elixir-build-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            std::fs::write(dir.join(name), format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(dir.join(name), std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        };
        script("slow-build", "sleep 0.8");
        script("broken-build", "exit 3");
        script("quick", "sleep 0.1");
        script("slow", "sleep 0.8");
        script("failing", "exit 1");
        let case = |build: &str, script: &str| Elixir {
            otp: String::new(),
            elixir: String::new(),
            build: vec![build.to_string()],
            scripts: vec![script.to_string()],
            must_fail: Some(String::new()),
            timeout_secs: Some(0.5),
        };
        let log = dir.join("log");
        assert_eq!(run(&dir, &case("slow-build", "quick"), &log).unwrap(), None);
        let failed = run(&dir, &case("slow-build", "failing"), &log).unwrap().unwrap();
        assert!(failed.starts_with("failing: exit status: 1"), "{failed}");
        let late = run(&dir, &case("slow-build", "slow"), &log).unwrap_err();
        assert_eq!(late.to_string(), "slow: ran past the case's deadline of 0.5 s");
        let broken = run(&dir, &case("broken-build", "quick"), &log).unwrap_err().to_string();
        assert!(broken.starts_with("build broken-build: exit status: 3"), "{broken}");
        let hung = run_within(&dir, &case("slow-build", "quick"), &log, 0.3).unwrap_err();
        assert_eq!(hung.to_string(), "build slow-build: timed out: the builds' deadline is 0.3 s");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
