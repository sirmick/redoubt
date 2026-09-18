//! The on-disk format of a test case (`xous64/tests/*.toml`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Output that fails any boot test, on top of the case's own `forbid` list.
pub const ALWAYS_FORBIDDEN: &[&str] = &["PANIC", "TEST FAILED", "loader64 PANIC"];

#[derive(Debug, Deserialize)]
pub struct Case {
    /// Taken from the file name.
    #[serde(skip)]
    pub name: String,
    pub description: String,
    /// Targets to run on, by name (see `target.rs`).
    pub arch: Vec<String>,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Kind {
    /// Boot the kernel with `programs` as the initial processes and watch the console.
    Boot(Boot),
    /// Only check that something compiles for the target. Coverage for configurations
    /// that cannot be booted under QEMU.
    Build(Build),
}

#[derive(Debug, Deserialize)]
pub struct Boot {
    /// Initial processes, in PID order starting at PID 2.
    pub programs: Vec<Program>,
    /// Hart counts to run with. One run per entry.
    #[serde(default = "default_smp")]
    pub smp: Vec<u32>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Regular expressions that must each match a console line, in this order.
    pub expect: Vec<String>,
    /// Regular expressions that must never match.
    #[serde(default)]
    pub forbid: Vec<String>,
    /// Console input to inject.
    #[serde(default)]
    pub input: Vec<Input>,
    /// Extra kernel features, e.g. `debug-print`.
    #[serde(default)]
    pub kernel_features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Build {
    pub package: String,
    #[serde(default)]
    pub features: Vec<String>,
}

/// A program to inject into the boot bundle.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Program {
    /// A binary of the `test-programs` package.
    TestProgram(String),
    /// A binary of any workspace package, built for the case's target.
    Package { package: String, bin: String },
    /// A prebuilt ELF, relative to the workspace root.
    Path { path: PathBuf },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Send once a console line matches this regular expression.
    pub after: String,
    pub send: String,
}

fn default_smp() -> Vec<u32> { vec![1] }

fn default_timeout() -> u64 { 60 }

impl Case {
    pub fn load(path: &Path) -> Result<Case> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut case: Case = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        case.name = path.file_stem().unwrap().to_string_lossy().into_owned();
        Ok(case)
    }
}
