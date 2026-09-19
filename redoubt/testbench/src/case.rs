//! The on-disk format of a test case (`redoubt/tests/*.toml`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Output that fails any boot test, on top of the case's own `forbid` list.
pub const ALWAYS_FORBIDDEN: &[&str] = &["PANIC", "TEST FAILED", "WARNING: INSECURE"];

#[derive(Debug, Deserialize)]
pub struct Case {
    /// Taken from the file name.
    #[serde(skip)]
    pub name: String,
    pub description: String,
    /// Targets to run on, by name (see `target.rs`). Empty for source-level checks.
    #[serde(default)]
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
    /// A ratchet on `unsafe` in the trusted computing base. Not a boot; reads the sources.
    UnsafeBudget(UnsafeBudget),
}

#[derive(Debug, Deserialize)]
pub struct UnsafeBudget {
    pub budget: Vec<Budget>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub name: String,
    /// Files or directories, relative to the workspace root.
    pub paths: Vec<String>,
    /// Most uses of the `unsafe` keyword allowed across `paths`.
    pub max_unsafe: usize,
    /// Most of those allowed to lack a `// SAFETY:` comment directly above them.
    pub max_undocumented: usize,
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
    /// Set to false for cases that provoke a panic on purpose. `ALWAYS_FORBIDDEN` is
    /// then not applied, only the case's own `forbid` list.
    #[serde(default = "default_true")]
    pub default_forbid: bool,
    /// Regular expressions with one capture group. The case is booted twice, and what
    /// each captures must differ between the two boots (for randomness, ASLR, ...).
    #[serde(default)]
    pub distinct_across_boots: Vec<String>,
    /// Console input to inject.
    #[serde(default)]
    pub input: Vec<Input>,
    /// Extra kernel features, e.g. `debug-print`.
    #[serde(default)]
    pub kernel_features: Vec<String>,
    /// Device grants written into the bundle's manifest (see DEVICE-GRANTS.md).
    #[serde(default)]
    pub grant: Vec<Grant>,
    /// Corrupt the bundle after signing, to test that the loader rejects it.
    #[serde(default)]
    pub tamper_bundle: bool,
    /// Firmware to boot under: "opensbi" (default) or "rustsbi". A case naming "rustsbi"
    /// is skipped where that firmware binary is not available.
    pub firmware: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The program (bundle file name) these grants apply to.
    pub program: String,
    /// MMIO regions as "hex-base:hex-len", e.g. "0x10000000:0x1000".
    #[serde(default)]
    pub mmio: Vec<String>,
    /// Interrupt numbers.
    #[serde(default)]
    pub irq: Vec<u32>,
}

impl Grant {
    /// The manifest lines for this grant (see DEVICE-GRANTS.md).
    pub fn manifest_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for region in &self.mmio {
            let (base, len) = region.split_once(':').unwrap_or((region, "0x1000"));
            lines.push(format!("{} mmio {} {}", self.program, base, len));
        }
        for irq in &self.irq {
            lines.push(format!("{} irq {}", self.program, irq));
        }
        lines
    }
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
    /// A `test-programs` binary, corrupted before injection, for testing how the loader
    /// and kernel cope with hostile images.
    Corrupted { corrupt: String, with: Corruption },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corruption {
    /// Move the first loadable segment to this virtual address (hex).
    SegmentVaddr(String),
    /// Set the entry point to this virtual address (hex).
    Entry(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Send once a console line matches this regular expression.
    pub after: String,
    pub send: String,
}

fn default_true() -> bool { true }

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
