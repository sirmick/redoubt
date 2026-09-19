//! The on-disk format of a test case (`redoubt/tests/*.toml`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
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
    /// SSH sessions against an OpenSSH server run on the host. Not a boot: it checks the
    /// bench's SSH client and session runner against a known-good server.
    SshLoopback(SshLoopback),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshLoopback {
    /// Test keys (`redoubt/tests/keys/`) the server accepts.
    pub authorized: Vec<String>,
    pub session: Vec<Session>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: f64,
    /// The host key the sessions expect, as an OpenSSH public key line. Defaults to the
    /// server's own (`loopback-host`); a self-check sets another to see the check fail.
    pub host_key: Option<String>,
    /// See `Boot::must_fail`.
    pub must_fail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Boot {
    /// Initial processes, in PID order starting at PID 2.
    pub programs: Vec<Program>,
    /// Hart counts to run with. One run per entry.
    #[serde(default = "default_smp")]
    pub smp: Vec<u32>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: f64,
    /// Guest RAM in MiB (QEMU `-m`); default `target::DEFAULT_MEMORY_MIB`. Small for cases
    /// that exhaust RAM on purpose, so they take milliseconds.
    pub memory_mib: Option<u32>,
    /// Regular expressions that must each match a console line, in this order.
    pub expect: Vec<String>,
    /// Regular expressions that must never match.
    #[serde(default)]
    pub forbid: Vec<String>,
    /// For cases that provoke a panic on purpose: drops `PANIC` from `ALWAYS_FORBIDDEN`.
    #[serde(default)]
    pub allow_panic: bool,
    /// The guest must power off after the last `expect` (and the sessions), with no forbidden
    /// line before it. Without this the bench watches the console for `GRACE` more, then stops.
    #[serde(default)]
    pub poweroff: bool,
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
    /// Firmware to boot under: "opensbi" (default) or "rustsbi".
    pub firmware: Option<String>,
    /// Data entries added to the bundle after the programs: a trace, a manifest, a hostile image.
    #[serde(default)]
    pub file: Vec<BundleFile>,
    /// A virtio-blk disk, created afresh for every boot.
    pub disk: Option<Disk>,
    /// A virtio-net device on QEMU's user-mode network.
    pub net: Option<Net>,
    /// SSH sessions to the guest's port 22, run once every `expect` has matched, while the
    /// console is still watched.
    #[serde(default)]
    pub session: Vec<Session>,
    /// The case passes only if the bench fails it for a reason matching this regular
    /// expression: self-checks proving that a bench feature can fail (TENETS.md 6).
    pub must_fail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleFile {
    /// The bundle entry's name. Must differ from every other entry's.
    pub name: String,
    /// Where the bytes come from: anything a `programs` entry can name. Injected as they are.
    pub from: Program,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disk {
    /// Size in KiB (a whole number of 512-byte sectors). The disk starts zeroed.
    pub size_kib: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    /// Guest TCP ports reachable from the host (default: none). Sessions need 22.
    #[serde(default)]
    pub forward: Vec<u16>,
    /// The guest's SSH host key, as an OpenSSH public key line. If set, sessions refuse any
    /// other; if not, they accept whatever key the guest presents.
    pub host_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// The login name, e.g. `alice+secrets`. Unique within a case; it also names the log.
    pub user: String,
    /// The test key to log in with. Defaults to `user` up to any `+`.
    pub key: Option<String>,
    /// Ask the server for a terminal, as an interactive user's ssh does.
    #[serde(default)]
    pub pty: bool,
    /// Regular expressions that must never match a line of this session's output.
    #[serde(default)]
    pub forbid: Vec<String>,
    pub steps: Vec<Step>,
}

impl Session {
    pub fn key(&self) -> &str { self.key.as_deref().unwrap_or_else(|| self.user.split('+').next().unwrap()) }
}

/// One step of a session. A session runs its steps in order; unless the last one is `exit`,
/// the bench then ends the session as `{ exit = 0 }` does.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Type this. Include the `\n`.
    Send(String),
    /// Wait until the output not consumed by an earlier `expect` matches this regular
    /// expression (multi-line mode), and consume it up to the end of the match. ssh's own
    /// messages count as output.
    Expect(String),
    /// Record that this session got here, for other sessions to `wait` for.
    Mark(String),
    /// Wait until some session has recorded this mark.
    Wait(String),
    /// Close the session's input, read its output until ssh exits, and require this status.
    Exit(i32),
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
#[serde(deny_unknown_fields)]
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
    /// Cut the file to this many bytes (fewer than it has): a malformed image.
    Truncate(usize),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Send once a console line matches this regular expression.
    pub after: String,
    pub send: String,
}

fn default_smp() -> Vec<u32> { vec![1] }

fn default_timeout() -> f64 { 60.0 }

impl Case {
    pub fn load(path: &Path) -> Result<Case> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut case: Case = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        case.name = path.file_stem().unwrap().to_string_lossy().into_owned();
        case.check().with_context(|| format!("in {}", path.display()))?;
        Ok(case)
    }

    /// Catch mistakes before booting anything, where they would otherwise show up only as a
    /// timeout or a confusing failure.
    fn check(&self) -> Result<()> {
        match &self.kind {
            Kind::Boot(boot) => {
                for pattern in &boot.distinct_across_boots {
                    let groups = regex::Regex::new(pattern)?.captures_len();
                    ensure!(groups >= 2, "distinct_across_boots /{pattern}/ needs a capture group");
                }
                if !boot.session.is_empty() {
                    ensure!(boot.net.as_ref().is_some_and(|n| n.forward.contains(&22)), "sessions need net.forward = [22]");
                }
                check_sessions(&boot.session)
            }
            Kind::SshLoopback(loopback) => check_sessions(&loopback.session),
            _ => Ok(()),
        }
    }
}

/// Session users name log files, so they are unique and plain; every mark waited for is set.
fn check_sessions(sessions: &[Session]) -> Result<()> {
    let mut users = HashSet::new();
    let marks: HashSet<&str> = sessions
        .iter()
        .flat_map(|s| &s.steps)
        .filter_map(|step| if let Step::Mark(mark) = step { Some(mark.as_str()) } else { None })
        .collect();
    for session in sessions {
        let user = session.user.as_str();
        ensure!(
            !user.is_empty() && user.bytes().all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b)),
            "session user {user:?}: use letters, digits, '_', '+' and '-'"
        );
        ensure!(users.insert(user), "two sessions log in as {user:?}");
        for (number, step) in session.steps.iter().enumerate() {
            match step {
                Step::Wait(mark) => {
                    ensure!(marks.contains(mark.as_str()), "session {user} waits for mark {mark:?}, which no session sets")
                }
                Step::Exit(_) => ensure!(number + 1 == session.steps.len(), "session {user}: `exit` must be the last step"),
                _ => {}
            }
        }
    }
    Ok(())
}
