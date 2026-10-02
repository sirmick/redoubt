//! The `elixir` kind: scripts that run Elixir oracles on beamlet under the pinned OTP and Elixir
//! (`userland/otp/tools/env.sh`), each judged by its exit status. The toolchain is checked first,
//! and a missing or wrong one fails the case even under --allow-skip: an oracle that does not run
//! catches nothing, and one on another version is not the one pinned.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::case::Elixir;

/// Lines of a failed script's output shown in the result; the whole of it is in the log.
const SHOWN: usize = 80;

/// Run the case's scripts: `Ok(None)` if every one exits 0, else the first failure. Their output
/// goes to `log`.
pub fn run(workspace: &Path, case: &Elixir, log: &Path) -> Result<Option<String>> {
    let mut all = String::new();
    for script in &case.scripts {
        let mut words = script.split_whitespace();
        let path = words.next().context("an empty script")?;
        let out = Command::new(workspace.join(path))
            .args(words)
            .current_dir(workspace)
            .output()
            .with_context(|| format!("running {path}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        all.push_str(&format!("$ {script}\n{stdout}{}\n", String::from_utf8_lossy(&out.stderr)));
        if !out.status.success() {
            std::fs::write(log, &all).with_context(|| format!("writing {}", log.display()))?;
            let shown: Vec<_> = stdout.lines().take(SHOWN).collect();
            return Ok(Some(format!(
                "{script}: {}\n      {}\n      (the whole output: {})",
                out.status,
                shown.join("\n      "),
                log.display()
            )));
        }
    }
    std::fs::write(log, &all).with_context(|| format!("writing {}", log.display()))?;
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
            scripts: Vec::new(),
            must_fail: None,
        };
        assert!(toolchain(&workspace, &pinned("28.0.0.1", "1.20.4")).is_err());
        assert!(toolchain(&workspace, &pinned("28.5.0.6", "1.20.3")).is_err());
    }
}
