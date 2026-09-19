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
    /// SSH sessions against an OpenSSH server the bench starts on the host. Not a boot: it
    /// checks the bench's SSH client and session runner against a known-good server.
    SshLoopback(SshLoopback),
}

#[derive(Debug, Deserialize)]
pub struct SshLoopback {
    /// Test keys (`ssh.rs`) the host server accepts.
    pub authorized: Vec<String>,
    pub session: Vec<Session>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// See `Boot::must_fail`.
    pub must_fail: Option<String>,
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
    /// Data entries added to the bundle after the programs: a trace, a manifest, a hostile image.
    #[serde(default)]
    pub file: Vec<BundleFile>,
    /// A virtio-blk disk, created afresh for every boot.
    pub disk: Option<Disk>,
    /// A virtio-net device on QEMU's user-mode network.
    pub net: Option<Net>,
    /// SSH sessions, run once every `expect` has matched, while the guest keeps running.
    #[serde(default)]
    pub session: Vec<Session>,
    /// Results the guest reports on the console, compared with a file.
    pub results: Option<Results>,
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
    /// Size in KiB (a whole number of 512-byte sectors).
    pub size_kib: u64,
    /// Initial contents, relative to the workspace root, copied to the start of the disk.
    pub image: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    /// Guest TCP ports reachable from the host. The bench picks a free host port for each.
    #[serde(default)]
    pub forward: Vec<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// Names the session in messages and in its log's file name. Defaults to `user`.
    pub name: Option<String>,
    /// The login name, e.g. `alice+secrets`.
    pub user: String,
    /// The test key (`ssh.rs`) to log in with. Defaults to `user` up to any `+`.
    pub key: Option<String>,
    /// The guest port to connect to; it must be in `net.forward`. (Unused by ssh-loopback.)
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Ask the server for a terminal, as an interactive user's ssh does.
    #[serde(default)]
    pub pty: bool,
    /// Regular expressions that must never match a line of this session's output.
    #[serde(default)]
    pub forbid: Vec<String>,
    pub steps: Vec<Step>,
}

impl Session {
    pub fn name(&self) -> &str { self.name.as_deref().unwrap_or(&self.user) }

    pub fn key(&self) -> &str { self.key.as_deref().unwrap_or_else(|| self.user.split('+').next().unwrap()) }
}

/// One step of a session. A session runs its steps in order.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Type this. Include the `\n`.
    Send(String),
    /// Wait until the output not consumed by an earlier `expect` matches this regular
    /// expression, and consume it up to the end of the match. ssh's own messages count as
    /// output, and when ssh exits the bench appends the line `[ssh exited: N]`.
    Expect(String),
    /// Record that this session got here, for other sessions to `wait` for.
    Mark(String),
    /// Wait until some session has recorded this mark.
    Wait(String),
    /// Close the session's input (end of file). The value must be `true`.
    Close(bool),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Results {
    /// A regular expression with one capture group. Each console line it matches reports
    /// one result: the captured text.
    pub pattern: String,
    /// The expected results, one per line, relative to the workspace root. Compared once
    /// every `expect` has matched (and every session finished): same count, text and order.
    pub expected: PathBuf,
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
    /// Cut the file to this many bytes: a malformed image.
    Truncate(usize),
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

fn default_ssh_port() -> u16 { 22 }

impl Case {
    pub fn load(path: &Path) -> Result<Case> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut case: Case = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        case.name = path.file_stem().unwrap().to_string_lossy().into_owned();
        match &case.kind {
            Kind::Boot(boot) => {
                let forwarded = boot.net.as_ref().map_or(&[][..], |net| &net.forward[..]);
                check_sessions(&boot.session, Some(forwarded))
            }
            Kind::SshLoopback(loopback) => check_sessions(&loopback.session, None),
            _ => Ok(()),
        }
        .with_context(|| format!("in {}", path.display()))?;
        Ok(case)
    }
}

/// Catch mistakes in a scenario before booting anything: unforwarded ports, duplicate
/// session names, and marks nobody sets (which would only show up as a timeout).
/// `forwarded` is None for a loopback case, which has no guest ports.
fn check_sessions(sessions: &[Session], forwarded: Option<&[u16]>) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    let marks: std::collections::HashSet<&str> = sessions
        .iter()
        .flat_map(|s| &s.steps)
        .filter_map(|step| if let Step::Mark(mark) = step { Some(mark.as_str()) } else { None })
        .collect();
    for session in sessions {
        let name = session.name();
        anyhow::ensure!(names.insert(name), "two sessions are named {name:?}");
        if let Some(forwarded) = forwarded {
            anyhow::ensure!(forwarded.contains(&session.port), "session {name}: port {} is not in net.forward", session.port);
        }
        for step in &session.steps {
            match step {
                Step::Wait(mark) => anyhow::ensure!(marks.contains(mark.as_str()), "session {name} waits for mark {mark:?}, which no session sets"),
                Step::Close(close) => anyhow::ensure!(*close, "session {name}: `close` must be true"),
                _ => {}
            }
        }
    }
    Ok(())
}
