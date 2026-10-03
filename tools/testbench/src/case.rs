//! The on-disk format of a test case (`tests/*.toml`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

/// Output that fails any boot test, on top of the case's own `forbid` list.
pub const ALWAYS_FORBIDDEN: &[&str] = &["PANIC", "TEST FAILED", "WARNING: INSECURE"];

/// What a test program's verdict line says, after its name.
pub const PASSED: &str = "TEST PASSED";

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
    /// Each trusted crate's lines of Rust against a ceiling that only falls (`size.rs`). Not a
    /// boot; reads the sources and the case file's history.
    SizeBudget(SizeBudget),
    /// No leftover of a dropped interface, no silenced dead code, no unread Cargo feature, one
    /// literal definition of each shared constant (`cruft.rs`). Not a boot; reads the sources.
    NoCruft(NoCruft),
    /// Every Rust source formatted with the repository's `rustfmt.toml` under nightly
    /// (`fmt.rs`). Not a boot; runs `cargo +nightly fmt --check`.
    Fmt(Fmt),
    /// `cargo test` for host crates, so the suite runs the unit tests that no boot can reach:
    /// a constant both the loader and the bench agree on is right in the machine's eyes even
    /// when it is wrong (see `libs/signing`). Not a boot.
    HostTests(HostTests),
    /// SSH sessions against a server run on the host: Redoubt's `sshd` on its host platform, or
    /// OpenSSH's for the reference case. Not a boot.
    SshLoopback(SshLoopback),
    /// Scripts running Elixir oracles on beamlet, each judged by its exit status, after a check
    /// that the pinned OTP and Elixir are the ones on the path (`elixir.rs`). Not a boot.
    Elixir(Elixir),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Elixir {
    /// The OTP release `erl` must be (its `releases/<major>/OTP_VERSION`).
    pub otp: String,
    /// The version `elixir --version` must say.
    pub elixir: String,
    /// Scripts, relative to the workspace root, each with its arguments after it, run in order
    /// from the workspace root. Each must exit 0.
    pub scripts: Vec<String>,
    /// See `Boot::must_fail`. A toolchain that is not the pinned one is never what it waits for.
    pub must_fail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshLoopback {
    /// Which server `ssh` starts.
    #[serde(default)]
    pub server: LoopbackServer,
    /// Test keys (`tests/keys/`) the server accepts. On Redoubt's server each is a principal of
    /// the same name.
    pub authorized: Vec<String>,
    pub session: Vec<Session>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: f64,
    /// The host key the sessions expect, as an OpenSSH public key line. Defaults to the
    /// server's own (`loopback-host`); a self-check sets another to see the check fail.
    pub host_key: Option<String>,
    /// Regular expressions each of which must match a line of the server's own log once the
    /// sessions end: what the server saw, not only what the client says.
    #[serde(default)]
    pub server_log: Vec<String>,
    /// Regular expressions none of which may match a line of the server's log: what the server
    /// must not have done.
    #[serde(default)]
    pub server_log_forbid: Vec<String>,
    /// See `Boot::must_fail`.
    pub must_fail: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoopbackServer {
    /// Redoubt's `sshd` on its host platform, `redoubt-sshd-host`.
    #[default]
    Redoubt,
    /// The host's OpenSSH `sshd`: the session runner's independent witness.
    Openssh,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnsafeBudget {
    pub budget: Vec<Budget>,
    /// Workspace members built for the target whose sources no budget counts, each with its
    /// reason (test programs, host tools, vendored code). Every other such member's sources must
    /// all be in some budget.
    #[serde(default)]
    pub uncounted: Vec<Skip>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizeBudget {
    #[serde(rename = "crate")]
    pub crates: Vec<SizeCrate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizeCrate {
    pub name: String,
    /// Files or directories, relative to the workspace root.
    pub paths: Vec<String>,
    /// Most lines of code allowed across `paths`.
    pub max_lines: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoCruft {
    /// Files and directories searched, relative to the workspace root.
    pub paths: Vec<String>,
    /// Patterns no line may match.
    pub forbidden: Vec<Forbidden>,
    /// Where `allow(dead_code)` and `allow(unused...)` are refused.
    pub no_allow_dead: Vec<String>,
    /// Names with at most one literal-valued definition across `paths` and `definition_paths`.
    pub one_definition: Vec<String>,
    /// Searched for `one_definition` only.
    #[serde(default)]
    pub definition_paths: Vec<String>,
    /// The only exemptions, each with its reason.
    #[serde(default)]
    pub allow: Vec<Allow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fmt {
    /// Cargo workspace roots, relative to the workspace root (`.` for it), each checked with
    /// `cargo +nightly fmt --all --check`.
    pub roots: Vec<String>,
    /// Workspace roots left unformatted, each with its reason.
    #[serde(default)]
    pub skip: Vec<Skip>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skip {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forbidden {
    /// A regular expression; `(?i)` for one that ignores case.
    pub pattern: String,
    /// A line that also matches this is not a finding.
    pub unless: Option<String>,
    /// Only files under this path, relative to the workspace root, are held to the pattern.
    pub within: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    /// A file, or a directory prefix.
    pub path: String,
    /// The rule exempted there: a `forbidden` pattern, `allow-dead`, `unused-feature`,
    /// `one-definition:NAME`, `page-alias`, or `*` for every rule.
    pub rule: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTests {
    /// Workspace packages whose `cargo test` must pass, on the host.
    pub packages: Vec<String>,
    /// The integration test files to run (`--test NAME`); every test target when empty.
    #[serde(default)]
    pub tests: Vec<String>,
    /// Run under nightly Miri, which checks the `unsafe` a native run only executes.
    #[serde(default)]
    pub miri: bool,
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
    /// The case's programs: the first in `init`'s place, the loader's one process; the tester
    /// starts the others in this order (docs/testbench.md, "Starting a case's programs").
    #[serde(default)]
    pub programs: Vec<Program>,
    /// A bundle recipe, relative to the workspace root, whose entries are the case's programs and
    /// data entries instead of `programs` and `file`: `image/boot.toml`, so that the bundle
    /// `./mkimage` packs is the one a case boots ([`Recipe`]).
    pub recipe: Option<PathBuf>,
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
    /// QEMU status required when `poweroff` is true. Zero is a normal shutdown; RustSBI maps an
    /// SBI `SystemFailure` shutdown to 255, which rejection tests must request explicitly.
    #[serde(default)]
    pub poweroff_status: i32,
    /// The program whose `DONE` to `log-server` ends a `poweroff` case (docs/testbench.md, rule
    /// F). The case passes only if the one console line starting `[server] done:` names this
    /// program's PID. Any other such line fails it, as does one in a case with no reporter.
    ///
    /// Under the real `init` ([`Boot::under_init`]) it names a `servers` entry of the case's
    /// manifest instead, and the case passes only if exactly one line says `TEST PASSED`, and it
    /// starts with the `[con N] ` of the console connection `init` announced for that entry
    /// (docs/testbench.md, "The servers' cases under `init`").
    pub reporter: Option<String>,
    /// Regular expressions with one capture group. The case is booted twice, and what
    /// each captures must differ between the two boots (for randomness, ASLR, ...).
    #[serde(default)]
    pub distinct_across_boots: Vec<String>,
    /// Regular expressions with one capture group. Each must match at least two console lines
    /// of the boot, and no two of what it captures may be the same (a restarted program's new
    /// console, ...).
    #[serde(default)]
    pub distinct: Vec<String>,
    /// Console input to inject.
    #[serde(default)]
    pub input: Vec<Input>,
    /// Extra kernel features, e.g. `debug-print`.
    #[serde(default)]
    pub kernel_features: Vec<String>,
    /// Run the guest in virtual time: QEMU `-icount <this>` (e.g. `shift=3,sleep=off`, a fixed
    /// instruction rate that skips idle time to the next timer deadline), with the RTC on the
    /// same virtual clock (`-rtc clock=vm`). Timing a case asserts is then instruction time, the
    /// same on any host; without it, host load shows up as guest latency. None: real time, as
    /// every case ran before.
    #[serde(default)]
    pub icount: Option<String>,
    /// Pin the guest's randomness: QEMU `-seed <this>`, which fills the device tree's
    /// `/chosen/rng-seed` and so the kernel's RNG. With `icount` a run then repeats exactly. The
    /// seed is printed with the result, and TESTBENCH_QEMU_SEED replaces it to replay or sweep.
    /// None: QEMU draws it from host entropy on every boot.
    #[serde(default)]
    pub qemu_seed: Option<u64>,
    /// Build the kernel and the loader with debug assertions on, so `core`'s precondition
    /// checks on raw-pointer calls and every `debug_assert!` run (a failure is a `PANIC`).
    #[serde(default)]
    pub debug_assertions: bool,
    /// Corrupt the bundle after signing, to test that the loader rejects it.
    #[serde(default)]
    pub tamper_bundle: bool,
    /// Sign the bare archive, with no domain and no length, instead of the preimage
    /// kernel/boot.md states ("Verified boot"): a valid signature by the right key over untouched
    /// bytes, which the loader must still refuse. The container and the archive are otherwise normal.
    #[serde(default)]
    pub sign_bare_archive: bool,
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
    /// expression: self-checks proving that a bench feature can fail (TENETS.md, tenet 6).
    pub must_fail: Option<String>,
    /// A check the bench runs on the console log once everything else passed, by name, then its
    /// arguments. The one there is: `sched_oracle`, the stride queue's ranks, floor and lifts over
    /// a `sched-trace` kernel's trace (`sched_oracle.rs`); `r10_p99_us=N` bounds destructions.
    pub post_check: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleFile {
    /// The bundle entry's name. Must differ from every other entry's.
    pub name: String,
    /// Where the bytes come from: anything a `programs` entry can name. Injected as they are,
    /// unless `servers` is given.
    pub from: Program,
    /// For a manifest read from a path: entries merged into its `servers` by name, each replacing
    /// the members it gives in the entry of its name, or added after the rest if none has it.
    #[serde(default)]
    pub servers: Vec<toml::Table>,
}

impl BundleFile {
    /// The file's bytes, `base` with [`BundleFile::servers`] merged in, or `base` itself if there
    /// are none.
    pub fn merged(&self, base: &[u8]) -> Result<Vec<u8>> {
        use serde_json::Value;
        if self.servers.is_empty() {
            return Ok(base.to_vec());
        }
        let mut manifest: Value = serde_json::from_slice(base).context("a merged file is JSON")?;
        let servers = manifest
            .get_mut("servers")
            .and_then(Value::as_array_mut)
            .context("a merged file has a servers array")?;
        for entry in &self.servers {
            let name =
                entry.get("name").and_then(toml::Value::as_str).context("a merged entry has a name")?;
            let Value::Object(fields) = serde_json::to_value(entry)? else { unreachable!("a table") };
            match servers.iter_mut().find(|s| s["name"] == name) {
                Some(Value::Object(server)) => server.extend(fields),
                Some(_) => bail!("servers entry {name:?} is not an object"),
                None => servers.push(Value::Object(fields)),
            }
        }
        Ok(serde_json::to_vec_pretty(&manifest)?)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disk {
    /// Size in KiB (a whole number of 512-byte sectors). The disk starts zeroed. Not given with
    /// a `recipe`, which sizes the disk.
    #[serde(default)]
    pub size_kib: u64,
    /// Partitions in a GPT written on the disk before the boot, equal shares of the space after
    /// the table, by `blkd`'s own image builder; 0 leaves the disk zeroed, with no table.
    #[serde(default)]
    pub partitions: u64,
    /// A disk recipe (`image/disk.toml`) packed for every boot instead, as `./mkimage` packs it
    /// (`disk.rs`).
    pub recipe: Option<PathBuf>,
    /// With a `recipe`, the directory every partition holds instead of the recipe's own stage.
    pub stage: Option<PathBuf>,
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
    /// Hosts the guest may connect to, each counting its connections (`peer.rs`). A case with
    /// peers also gets the wider virtual network and a capture of the guest's frames.
    #[serde(default)]
    pub peer: Vec<Peer>,
    /// Connections the bench makes into the guest's forwarded ports while it boots (`peer.rs`).
    #[serde(default)]
    pub dial: Vec<Dial>,
    /// Prefixes (`A.B.C.D/len`) the guest must never send a SYN to: the box's own addresses.
    #[serde(default)]
    pub self_forbidden: Vec<String>,
    /// Cut the capture to this many bytes before judging it: self-checks that a cut or empty
    /// capture fails.
    pub truncate_capture: Option<u64>,
    /// One UDP datagram the bench sends into the guest, once, from outside (`peer.rs`).
    pub poke: Option<Poke>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Poke {
    /// The guest UDP port it goes to; QEMU forwards a host port of its own there.
    pub port: u16,
    /// Its whole payload.
    pub payload: String,
    /// It is sent when a console line matches this regular expression, the first time.
    pub after: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    /// `A.B.C.D:PORT`, inside 10.0.0.0/16 and outside slirp's own 10.0.2.0/24.
    pub addr: String,
    /// Exactly how many connections the guest must make to it.
    pub connections: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dial {
    /// The guest port, one of `forward`.
    pub port: u16,
    /// What the bench sends once connected.
    pub send: String,
    /// What must come back on the same connection.
    pub expect: String,
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
    /// More arguments for `ssh`, before the host: `-R`, `-W`, `-s` and the like.
    #[serde(default)]
    pub ssh_args: Vec<String>,
    /// A command to run, or with `-s` a subsystem, in place of a shell.
    pub command: Option<String>,
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
    /// Set a `pty = true` session's terminal to `[cols, rows]`, which makes ssh send a
    /// `window-change` if the size changed.
    Resize([u16; 2]),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub package: String,
    #[serde(default)]
    pub features: Vec<String>,
}

/// A program to inject into the boot bundle. The `bin` form takes `budgets`: the budgets the
/// tester gives the program, from `root`, `system` and `users` (docs/testbench.md, "Starting a
/// case's programs").
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Program {
    /// A binary of the `test-programs` package.
    TestProgram(String),
    /// A binary of any workspace package, built for the case's target, with `features` if any
    /// (`build.rs` keeps that build apart from the one without). With `workspace`, the package
    /// is in that workspace of its own, a directory relative to the root (`userland/otp`), and is
    /// built there.
    Package {
        package: String,
        bin: String,
        #[serde(default)]
        features: Vec<String>,
        workspace: Option<PathBuf>,
    },
    /// A binary of the `test-programs` package, with budgets.
    Bin {
        bin: String,
        #[serde(default)]
        budgets: Vec<String>,
    },
    /// A prebuilt ELF, relative to the workspace root.
    Path { path: PathBuf },
    /// An Erlang module's source, relative to the workspace root, compiled to its `.beam` by the
    /// pinned toolchain's `erlc` (`userland/otp/tools/env.sh`).
    Erlang { erlang: PathBuf },
    /// The `.beam` of a module of the pinned toolchain's OTP, by module name (`io`).
    Otp { otp: String },
    /// This many zero bytes, made in the run: an entry whose length is all that matters, such as
    /// a program `init` must refuse on its size before it reads a byte (init-refuses-bound).
    Zeros { zeros: u64 },
    /// A `test-programs` binary, corrupted before injection, for testing how the loader
    /// and kernel cope with hostile images.
    Corrupted { corrupt: String, with: Corruption },
}

/// A boot bundle's recipe, `image/boot.toml`: its entries in bundle order, the kernel first, then
/// `init` and the rest (docs/kernel/boot.md, "The boot bundle").
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub entry: Vec<RecipeEntry>,
}

/// One entry: a program, the binary of `package` named as the entry, or data read from `path`,
/// relative to the workspace root.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeEntry {
    pub name: String,
    pub package: Option<String>,
    pub path: Option<PathBuf>,
}

impl Recipe {
    pub fn load(path: &Path) -> Result<Recipe> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// The programs and the data entries after the kernel, which the builder always packs first
    /// from its own build.
    pub fn contents(&self) -> Result<(Vec<Program>, Vec<BundleFile>)> {
        let Some((kernel, rest)) = self.entry.split_first() else { bail!("a recipe with no entries") };
        ensure!(
            kernel.name == "kernel"
                && kernel.package.as_deref() == Some("redoubt-kernel")
                && kernel.path.is_none(),
            "a recipe's first entry is the kernel"
        );
        let (mut programs, mut files) = (Vec::new(), Vec::new());
        for entry in rest {
            match (&entry.package, &entry.path) {
                (Some(package), None) => programs.push(Program::Package {
                    package: package.clone(),
                    bin: entry.name.clone(),
                    features: Vec::new(),
                    workspace: None,
                }),
                (None, Some(path)) => files.push(BundleFile {
                    name: entry.name.clone(),
                    from: Program::Path { path: path.clone() },
                    servers: Vec::new(),
                }),
                _ => bail!("recipe entry {:?} needs a package or a path, not both", entry.name),
            }
        }
        ensure!(programs.first().is_some_and(Program::is_init), "a recipe's second entry is init");
        Ok((programs, files))
    }
}

impl Program {
    /// Whether it is the real `init`, which starts the other programs from its manifest.
    pub fn is_init(&self) -> bool {
        matches!(self, Program::Package { package, bin, .. } if package == "redoubt-init" && bin == "init")
    }

    /// The `test-programs` binary it names, if it names one.
    pub fn test_program(&self) -> Option<&str> {
        match self {
            Program::TestProgram(bin) | Program::Bin { bin, .. } => Some(bin),
            _ => None,
        }
    }

    /// The budgets the tester gives it.
    pub fn budgets(&self) -> &[String] {
        match self {
            Program::Bin { budgets, .. } => budgets,
            _ => &[],
        }
    }
}

/// The budgets a tester can give: `init`'s three.
pub const BUDGETS: [&str; 3] = ["root", "system", "users"];

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

impl Boot {
    /// Whether the real `init` is in the first program's place: then the other programs are only
    /// bundle entries, which `init` starts as its manifest says.
    pub fn under_init(&self) -> bool { self.programs.first().is_some_and(Program::is_init) }

    /// Under `init`, the line in which `init` announces the reporter's console connection, bare:
    /// its one capture is the connection's id.
    pub fn reporter_announced(&self) -> Option<String> {
        let reporter = self.reporter.as_ref().filter(|_| self.under_init())?;
        Some(format!(r"^init: started {}, console ([0-9a-f]{{16}})$", regex::escape(reporter)))
    }

    /// The reporter's place, which the tester prints as its PID: the case's programs are places
    /// 2 on, in order.
    pub fn reporter_pid(&self) -> Option<usize> {
        let reporter = self.reporter.as_ref().filter(|_| !self.under_init())?;
        let index = self.programs.iter().position(|p| p.test_program() == Some(reporter.as_str()))?;
        Some(index + 2)
    }
}

impl Case {
    pub fn load(path: &Path) -> Result<Case> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut case: Case = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        case.name = path.file_stem().unwrap().to_string_lossy().into_owned();
        if let Kind::Boot(boot) = &mut case.kind {
            if let Some(recipe) = &boot.recipe {
                ensure!(
                    boot.programs.is_empty() && boot.file.is_empty(),
                    "in {}: a case with a recipe takes its programs and files from it",
                    path.display()
                );
                // Cases live in `tests/`, one below the workspace root.
                let root = path.parent().and_then(Path::parent).context("a case outside tests/")?;
                (boot.programs, boot.file) = Recipe::load(&root.join(recipe))?.contents()?;
            }
        }
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
                for pattern in &boot.distinct {
                    let groups = regex::Regex::new(pattern)?.captures_len();
                    ensure!(groups >= 2, "distinct /{pattern}/ needs a capture group");
                }
                if !boot.session.is_empty() {
                    ensure!(
                        boot.net.as_ref().is_some_and(|n| n.forward.contains(&22)),
                        "sessions need net.forward = [22]"
                    );
                }
                if let Some(disk) = &boot.disk {
                    match &disk.recipe {
                        Some(_) => ensure!(
                            disk.size_kib == 0 && disk.partitions == 0,
                            "a disk recipe sizes and partitions the disk itself"
                        ),
                        None => {
                            ensure!(disk.size_kib > 0, "a disk needs size_kib or a recipe");
                            ensure!(disk.stage.is_none(), "a stage needs a disk recipe");
                        }
                    }
                }
                if let Some(net) = &boot.net {
                    check_net(net)?;
                    // The post-check judges one boot's peer files.
                    ensure!(
                        net.peer.is_empty() || boot.distinct_across_boots.is_empty(),
                        "peers need one boot"
                    );
                }
                for file in &boot.file {
                    ensure!(
                        file.servers.is_empty() || matches!(file.from, Program::Path { .. }),
                        "file {:?}: servers merge only into a manifest read from a path",
                        file.name
                    );
                }
                for (index, program) in boot.programs.iter().enumerate() {
                    let budgets = program.budgets();
                    ensure!(
                        index != 0 || budgets.is_empty(),
                        "the first program holds init's budgets already"
                    );
                    for (i, budget) in budgets.iter().enumerate() {
                        ensure!(BUDGETS.contains(&budget.as_str()), "unknown budget {budget:?}");
                        ensure!(!budgets[..i].contains(budget), "budget {budget:?} named twice");
                    }
                }
                match &boot.reporter {
                    // No program under `init` holds the Reset right, so the case ends at its last
                    // expect, which waits for the reporter's verdict.
                    Some(_) if boot.under_init() => {
                        ensure!(!boot.poweroff, "a reporter under init needs poweroff = false");
                        ensure!(
                            boot.expect.last().is_some_and(|e| e.contains(PASSED)),
                            "a reporter under init needs a last expect waiting for its {PASSED:?}"
                        );
                    }
                    Some(reporter) => {
                        ensure!(boot.poweroff, "a reporter needs poweroff = true");
                        ensure!(
                            boot.reporter_pid().is_some(),
                            "reporter {reporter:?} is not one of the programs"
                        );
                    }
                    None => {}
                }
                check_sessions(&boot.session)
            }
            Kind::SshLoopback(loopback) => check_sessions(&loopback.session),
            _ => Ok(()),
        }
    }
}

/// Peers are distinct and well-formed, and a case with peers has a positive control: one peer
/// the guest must reach, whose SYN proves the capture was live. Dials go to forwarded ports.
fn check_net(net: &Net) -> Result<()> {
    let mut seen = HashSet::new();
    for peer in &net.peer {
        ensure!(seen.insert(crate::peer::parse_peer(&peer.addr)?), "peer {} given twice", peer.addr);
    }
    if !net.peer.is_empty() {
        ensure!(net.peer.iter().any(|p| p.connections > 0), "peers need one with connections > 0");
    }
    ensure!(
        net.self_forbidden.is_empty() || !net.peer.is_empty(),
        "self_forbidden needs peers (the capture)"
    );
    ensure!(net.truncate_capture.is_none() || !net.peer.is_empty(), "truncate_capture needs peers");
    for prefix in &net.self_forbidden {
        crate::peer::parse_prefix(prefix)?;
    }
    for dial in &net.dial {
        ensure!(net.forward.contains(&dial.port), "dial to {}: not in net.forward", dial.port);
        ensure!(!dial.expect.is_empty(), "dial to {}: expect nothing", dial.port);
    }
    if let Some(poke) = &net.poke {
        // Its host port is found among the forwards by guest port, so it cannot share one.
        ensure!(
            poke.port != 0 && !net.forward.contains(&poke.port),
            "poke to {}: a TCP forward's port",
            poke.port
        );
        regex::Regex::new(&poke.after).with_context(|| format!("poke after {:?}", poke.after))?;
    }
    Ok(())
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
                    ensure!(
                        marks.contains(mark.as_str()),
                        "session {user} waits for mark {mark:?}, which no session sets"
                    )
                }
                Step::Exit(_) => {
                    ensure!(number + 1 == session.steps.len(), "session {user}: `exit` must be the last step")
                }
                Step::Resize(_) => ensure!(session.pty, "session {user}: `resize` needs `pty = true`"),
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The image's recipe is the kernel, `init`, the six servers and the manifest; a recipe that
    /// does not start with the kernel and `init`, or an entry that is neither a program nor data,
    /// is refused before anything is built.
    #[test]
    fn the_image_recipe_packs_init_the_servers_and_the_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (programs, files) = Recipe::load(&root.join("image/boot.toml")).unwrap().contents().unwrap();
        let bins: Vec<&str> = programs
            .iter()
            .map(|p| match p {
                Program::Package { bin, .. } => bin.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(bins, ["init", "keyd", "consoled", "bootfsd", "blkd", "netd", "ipd", "fsd"]);
        assert!(programs[0].is_init());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "manifest");
        assert!(matches!(&files[0].from, Program::Path { path } if path == Path::new("image/manifest.json")));

        let recipe = |text: &str| -> Result<(Vec<Program>, Vec<BundleFile>)> {
            toml::from_str::<Recipe>(text).unwrap().contents()
        };
        let kernel = "[[entry]]\nname = \"kernel\"\npackage = \"redoubt-kernel\"\n";
        let init = "[[entry]]\nname = \"init\"\npackage = \"redoubt-init\"\n";
        assert!(recipe(&format!("{kernel}{init}")).is_ok());
        assert!(recipe(init).is_err(), "no kernel first");
        assert!(recipe(kernel).is_err(), "no init second");
        assert!(recipe(&format!("{init}{kernel}")).is_err());
        let both = "[[entry]]\nname = \"x\"\npackage = \"p\"\npath = \"x\"\n";
        assert!(recipe(&format!("{kernel}{init}{both}")).is_err(), "a program and data at once");
        let neither = "[[entry]]\nname = \"x\"\n";
        assert!(recipe(&format!("{kernel}{init}{neither}")).is_err());
    }

    /// A manifest file's `servers` entries merge by name: one replaces the members it gives in the
    /// entry of its name and keeps the rest, one naming no entry is added after the others; a file
    /// not read from a path takes none.
    #[test]
    fn a_file_s_servers_merge_into_its_manifest_by_name() {
        let text = "name = \"manifest\"\nfrom = { path = \"m.json\" }\n\
                    servers = [{ name = \"client\", args = [\"read\", \"x\\n\"] }, { name = \"extra\" }]\n";
        let file: BundleFile = toml::from_str(text).unwrap();
        let base = br#"{"servers": [{"name": "fsd", "args": ["a"]}, {"name": "client", "program": "c", "args": ["boot"]}]}"#;
        let merged: serde_json::Value = serde_json::from_slice(&file.merged(base).unwrap()).unwrap();
        let expected = serde_json::json!({"servers": [
            {"name": "fsd", "args": ["a"]},
            {"name": "client", "program": "c", "args": ["read", "x\n"]},
            {"name": "extra"},
        ]});
        assert_eq!(merged, expected);
        assert!(file.merged(b"{}").is_err(), "no servers array");
        let case = |from: &str| -> Case {
            let text = format!(
                "description = \"d\"\nkind = \"boot\"\nprograms = [\"tester\"]\nexpect = []\n\
                 [[file]]\nname = \"manifest\"\nfrom = {from}\nservers = [{{ name = \"client\" }}]\n"
            );
            toml::from_str(&text).unwrap()
        };
        assert!(case("{ path = \"m.json\" }").check().is_ok());
        let err = case("\"tester\"").check().unwrap_err().to_string();
        assert!(err.contains("only into a manifest read from a path"), "{err}");
    }

    /// `resize` works only on a session's terminal, so a session without one is refused at load.
    #[test]
    fn resize_needs_a_pty() {
        let session = |pty: bool| -> Session {
            let text = format!("user = \"alice\"\npty = {pty}\nsteps = [{{ resize = [132, 43] }}]\n");
            toml::from_str(&text).unwrap()
        };
        assert!(check_sessions(&[session(true)]).is_ok());
        let err = check_sessions(&[session(false)]).unwrap_err().to_string();
        assert!(err.contains("`resize` needs `pty = true`"), "{err}");
    }

    /// A package's program takes `features`, none unless named.
    #[test]
    fn a_package_program_takes_features() {
        #[derive(Deserialize)]
        struct Programs {
            programs: Vec<Program>,
        }
        let text = "programs = [\n  { package = \"p\", bin = \"b\" },\n  \
                    { package = \"p\", bin = \"b\", features = [\"f\"] },\n]\n";
        let programs: Programs = toml::from_str(text).unwrap();
        let features: Vec<&[String]> = programs
            .programs
            .iter()
            .map(|p| match p {
                Program::Package { features, .. } => features.as_slice(),
                other => panic!("not a package's program: {other:?}"),
            })
            .collect();
        assert_eq!(features, [&[][..], &["f".to_string()][..]]);
    }

    /// A package's program may name a workspace of its own, none unless named; an Erlang
    /// module's source, an OTP module and a run of zeros are forms of their own.
    #[test]
    fn a_program_may_come_from_another_workspace_or_from_erlang() {
        #[derive(Deserialize)]
        struct Programs {
            programs: Vec<Program>,
        }
        let text = "programs = [\n  { package = \"p\", bin = \"b\" },\n  \
                    { package = \"p\", bin = \"b\", workspace = \"userland/otp\" },\n  \
                    { erlang = \"m.erl\" },\n  { otp = \"io\" },\n  { zeros = 4096 },\n]\n";
        let programs: Programs = toml::from_str(text).unwrap();
        let workspaces: Vec<Option<&Path>> = programs.programs[..2]
            .iter()
            .map(|p| match p {
                Program::Package { workspace, .. } => workspace.as_deref(),
                other => panic!("not a package's program: {other:?}"),
            })
            .collect();
        assert_eq!(workspaces, [None, Some(Path::new("userland/otp"))]);
        assert!(matches!(&programs.programs[2], Program::Erlang { erlang } if erlang == Path::new("m.erl")));
        assert!(matches!(&programs.programs[3], Program::Otp { otp } if otp == "io"));
        assert!(matches!(&programs.programs[4], Program::Zeros { zeros: 4096 }));
    }

    /// Every manifest that starts `beamlet`, the image's and the cases', gives it
    /// `budget_pages=N` with N its budget's pages: the argument sizes the VM's limits, and
    /// nothing else keeps it equal to the budget. The stopgap until the startup block carries
    /// the budget (docs/todo/beamlet-budget-from-startup.md).
    #[test]
    fn every_beamlet_is_told_its_own_budget() {
        fn manifests(dir: &Path, found: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    manifests(&path, found);
                } else if path.extension().is_some_and(|e| e == "json") {
                    found.push(path);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut found = vec![root.join("image/manifest.json")];
        manifests(&root.join("tests/data"), &mut found);
        let mut beamlets = 0;
        for path in found {
            // Not every file is a manifest, nor a well-formed one: refusal cases' are not.
            let Ok(json) = serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            let Some(servers) = json.get("servers").and_then(|s| s.as_array()) else { continue };
            for server in servers.iter().filter(|s| s["program"] == "beamlet") {
                let pages =
                    server["budget"]["pages"].as_str().unwrap_or_else(|| panic!("{path:?}: no budget"));
                let args: Vec<&str> =
                    server["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
                let given: Vec<&str> = args.iter().filter_map(|a| a.strip_prefix("budget_pages=")).collect();
                assert_eq!(given, [pages], "{path:?}: beamlet's budget_pages, against its budget");
                beamlets += 1;
            }
        }
        assert!(beamlets >= 4, "the beamlet cases' manifests were not found ({beamlets})");
    }
}
