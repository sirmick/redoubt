//! Building the pieces of a boot test with cargo, and packing the boot bundle.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use anyhow::{Context, Result, bail, ensure};
use ed25519_compact::{KeyPair, Seed};
use serde::Deserialize;

use crate::case::{Corruption, HostTests, Program};
use crate::disk::Verified;
use crate::target::Target;
use crate::userland::Staged;

pub struct Builder {
    pub workspace: PathBuf,
    /// This run's own directory (`run.rs`): the binaries it packs and boots, and every file it
    /// makes from them, are copied or written here and nowhere another run writes.
    pub run: PathBuf,
    pub verbose: bool,
    /// Each userland disk staged in this run, by its recipe ([`Builder::userland`]).
    pub staged: Mutex<Vec<(PathBuf, Staged)>>,
}

/// Runs `command` with the pinned Erlang toolchain on the path (`userland/otp/tools/env.sh`),
/// from `workspace`, and returns what it printed. A VM that crashes writes no `erl_crash.dump`
/// into the tree.
pub fn erlang(workspace: &Path, command: &[&str]) -> Result<String> {
    let output = Command::new("bash")
        .current_dir(workspace)
        .env("ERL_CRASH_DUMP", "/dev/null")
        .args(["-c", ". userland/otp/tools/env.sh && exec \"$@\"", "_"])
        .args(command)
        .output()
        .with_context(|| format!("running {}", command[0]))?;
    ensure!(
        output.status.success(),
        "{} failed: {}",
        command.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Which cargo profile builds a package: the workspace's `release`, or `checked` (Cargo.toml):
/// release with debug assertions and overflow checks on, so every precondition check in
/// `core` (`slice::from_raw_parts`, `ptr::read`, ...), every `debug_assert!` and every
/// arithmetic overflow becomes a panic instead of silence. Cargo keeps each profile's
/// artifacts apart, so switching between the two rebuilds neither.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Release,
    Checked,
}

impl Profile {
    fn name(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Checked => "checked",
        }
    }
}

/// What cargo's `--message-format=json` says about one unit it built.
#[derive(Deserialize)]
struct Message {
    reason: String,
    target: Option<MessageTarget>,
    executable: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
    profile: Option<MessageProfile>,
}

#[derive(Deserialize)]
struct MessageTarget {
    name: String,
}

#[derive(Deserialize)]
struct MessageProfile {
    test: bool,
}

/// A test binary `cargo test --no-run` built: its target's name, its path, and its package's
/// directory, where `cargo test` would run it.
pub struct TestBinary {
    pub name: String,
    pub path: PathBuf,
    pub dir: PathBuf,
}

impl Builder {
    /// `cargo build` one package for `target` with `profile`.
    pub fn cargo_build(
        &self,
        target: &Target,
        package: &str,
        features: &[String],
        profile: Profile,
    ) -> Result<()> {
        self.cargo(target, None, package, None, features, profile).map(drop)
    }

    /// Build `package` (only its binary `bin`, if given) and return the path cargo reports for
    /// that binary, `bin` or else the package's name. The build cache is the workspace's one
    /// `target/`, as for any cargo build there (`build.build-dir`); but cargo copies the finished
    /// binaries into this run's own target directory, under its own lock, so another run building
    /// other features in the same cache never replaces the file this run packs. The path cargo
    /// reports is that copy: the hashed file in the cache's `deps/` is never reported. A build
    /// with features copies into a directory of its own within the run, so a program built with
    /// them never replaces the same program built without.
    ///
    /// A package of a workspace of its own (`workspace`, relative to the root) is built the same
    /// way from that workspace, with its own cache, `target/` in it, and its own directory of
    /// copies in the run.
    fn cargo(
        &self,
        target: &Target,
        workspace: Option<&Path>,
        package: &str,
        bin: Option<&str>,
        features: &[String],
        profile: Profile,
    ) -> Result<Option<PathBuf>> {
        let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        // On the command line, both win over a user's cargo config and environment, so a run's
        // copies cannot land in a shared path; and, unlike the environment, they do not reach
        // the cargo a build script runs (`tests/programs/build.rs`), which keeps a target
        // directory of its own and would wait forever on this build's lock.
        let root = workspace.map_or_else(|| self.workspace.clone(), |w| self.workspace.join(w));
        let cache = root.join("target").to_string_lossy().into_owned();
        let build_dir = format!("build.build-dir={}", toml::Value::String(cache));
        cargo.current_dir(&root).args([
            "build",
            "--message-format=json-render-diagnostics",
            "--config",
            &build_dir,
            "--target-dir",
        ]);
        let mut copies = match features {
            [] => String::from("cargo"),
            _ => format!("cargo-{}", features.join("+")),
        };
        if let Some(workspace) = workspace {
            copies = format!("{copies}@{}", workspace.to_string_lossy().replace('/', "_"));
        }
        cargo.arg(self.run.join(copies)).args([
            "--profile",
            profile.name(),
            "--target",
            target.triple,
            "-p",
            package,
        ]);
        if let Some(bin) = bin {
            cargo.args(["--bin", bin]);
        }
        if !features.is_empty() {
            cargo.args(["--features", &features.join(",")]);
        }
        if !self.verbose {
            cargo.arg("--quiet").stderr(Stdio::piped());
        }
        let output = cargo.output().context("running cargo")?;
        if !output.status.success() {
            let what = bin.map_or(package.to_string(), |bin| format!("{package}:{bin}"));
            bail!(
                "building {what} for {} failed: {}",
                target.name,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let want = bin.unwrap_or(package);
        let mut executable = None;
        for line in output.stdout.split(|&b| b == b'\n').filter(|line| line.starts_with(b"{")) {
            let message: Message = serde_json::from_slice(line).context("reading cargo's report")?;
            if message.reason == "compiler-artifact" && message.target.is_some_and(|t| t.name == want) {
                executable = message.executable.or(executable);
            }
        }
        Ok(executable)
    }

    /// `cargo` for a binary the run packs or boots: an error if cargo reports none.
    pub fn binary(
        &self,
        target: &Target,
        package: &str,
        bin: Option<&str>,
        features: &[String],
        profile: Profile,
    ) -> Result<PathBuf> {
        let what = bin.map_or(package.to_string(), |bin| format!("{package}:{bin}"));
        self.cargo(target, None, package, bin, features, profile)?
            .with_context(|| format!("cargo reported no binary for {what} on {}", target.name))
    }

    /// Build `program` if it comes from the workspace, and return the path of its ELF.
    pub fn program(&self, target: &Target, program: &Program) -> Result<(String, PathBuf)> {
        let (package, bin, features, workspace): (&str, &str, &[String], Option<&Path>) = match program {
            Program::Path { path } => {
                let name =
                    path.file_name().context("program path has no file name")?.to_string_lossy().into_owned();
                return Ok((name, self.workspace.join(path)));
            }
            Program::Corrupted { corrupt, with } => {
                let built = self.binary(target, "test-programs", Some(corrupt), &[], Profile::Release)?;
                let mut elf = std::fs::read(built)?;
                corrupt_elf(&mut elf, with)?;
                let name = format!("{corrupt}-corrupted");
                let path = self.run.join(format!("{name}-{}.elf", target.name));
                std::fs::write(&path, elf)?;
                return Ok((name, path));
            }
            Program::Erlang { erlang } => return self.erlc(erlang),
            Program::Otp { otp } => return self.otp_module(otp),
            Program::Zeros { zeros } => return self.zeros(*zeros),
            Program::TestProgram(bin) | Program::Bin { bin, .. } => {
                ("test-programs", bin.as_str(), &[], None)
            }
            Program::Package { package, bin, features, workspace } => {
                (package.as_str(), bin.as_str(), features, workspace.as_deref())
            }
        };
        let built = self
            .cargo(target, workspace, package, Some(bin), features, Profile::Release)?
            .with_context(|| format!("cargo reported no binary for {package}:{bin} on {}", target.name))?;
        Ok((bin.to_string(), built))
    }

    /// The userland disk `recipe` packs, staged and packed once in this run: its objects'
    /// directory, the disk, and its verified volumes' roots (`userland.rs`, `disk.rs`). The
    /// bundle's manifest and every boot take this one pack, so the root the manifest pins is the
    /// disk's.
    pub fn userland(&self, recipe: &Path) -> Result<Staged> {
        let mut staged = self.staged.lock().unwrap();
        if let Some((_, done)) = staged.iter().find(|(r, _)| r == recipe) {
            return Ok(done.clone());
        }
        let dir = self.userland_dir(recipe);
        let (objects, image) = (dir.join("objects"), dir.join("userland.img"));
        let loaded = crate::disk::Recipe::load(&self.workspace.join(recipe))?;
        let wanted = loaded.objects.as_ref().with_context(|| format!("{}: no objects", recipe.display()))?;
        crate::userland::stage(&self.workspace, wanted, &objects)?;
        let (disk, verified) = crate::disk::pack(&loaded, &self.workspace, Some(&objects))?;
        std::fs::write(&image, disk).with_context(|| format!("writing {}", image.display()))?;
        let done = Staged { objects, image, verified };
        staged.push((recipe.to_path_buf(), done.clone()));
        Ok(done)
    }

    /// Where this run stages the userland disk of `recipe`: named for the recipe's whole path, as
    /// two recipes may share a file name (`image/userland.toml`, `tests/data/pack/userland.toml`)
    /// and a run that stages both keeps each apart.
    fn userland_dir(&self, recipe: &Path) -> PathBuf {
        self.run.join("userland").join(recipe.with_extension("").to_string_lossy().replace('/', "_"))
    }

    /// A file of `len` zero bytes in this run, named for its length.
    fn zeros(&self, len: u64) -> Result<(String, PathBuf)> {
        let name = format!("zeros-{len}");
        let path = self.run.join(&name);
        let file = std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        file.set_len(len).with_context(|| format!("sizing {}", path.display()))?;
        Ok((name, path))
    }

    fn erlang(&self, command: &[&str]) -> Result<String> { erlang(&self.workspace, command) }

    /// Compiles the Erlang module at `source` into this run's `erlang/`, and returns the
    /// module's file name and its `.beam`.
    fn erlc(&self, source: &Path) -> Result<(String, PathBuf)> {
        let module = source.file_stem().context("an Erlang source with no name")?.to_string_lossy();
        let out = self.run.join("erlang");
        std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
        self.erlang(&["erlc", "-o", &out.to_string_lossy(), &source.to_string_lossy()])?;
        let beam = format!("{module}.beam");
        Ok((beam.clone(), out.join(beam)))
    }

    /// The `.beam` of OTP's module `module` in the pinned toolchain, by the path its code server
    /// gives.
    fn otp_module(&self, module: &str) -> Result<(String, PathBuf)> {
        ensure!(
            !module.is_empty()
                && module.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{module:?} is not an OTP module's name"
        );
        // `code:which/1` answers an atom for a module it lacks: printed as nothing.
        let eval = format!(
            "case code:which({module}) of P when is_list(P) -> io:put_chars(P); _ -> ok end, halt()."
        );
        let path = PathBuf::from(self.erlang(&["erl", "-noshell", "-eval", &eval])?.trim());
        ensure!(path.is_file(), "the pinned OTP has no module {module}");
        Ok((format!("{module}.beam"), path))
    }

    /// Resolve a host-tests workspace within the repository. A requested workspace must be a
    /// Cargo workspace directory; a bad request is an error, never a reason to run at the root.
    fn host_test_workspace(&self, requested: Option<&Path>) -> Result<PathBuf> {
        let root = self.workspace.canonicalize().context("resolving the testbench workspace")?;
        let Some(requested) = requested else { return Ok(root) };
        let parts = requested.to_str().context("host-tests workspace is not UTF-8")?;
        ensure!(
            !requested.is_absolute()
                && parts.split('/').all(|part| !part.is_empty() && part != "." && part != ".."),
            "host-tests workspace must be a relative directory below the repository root"
        );
        let workspace = root
            .join(requested)
            .canonicalize()
            .with_context(|| format!("resolving host-tests workspace {}", requested.display()))?;
        ensure!(
            workspace.starts_with(&root) && workspace != root,
            "host-tests workspace {} leaves the repository root",
            requested.display()
        );
        ensure!(
            workspace.join("Cargo.toml").is_file(),
            "host-tests workspace {} has no Cargo.toml",
            requested.display()
        );
        Ok(workspace)
    }

    /// The `cargo test` a host-tests case runs, of the test files `tests` (every target when
    /// empty), with `args` for Cargo. Under Miri it goes through rustup's `cargo`, which alone
    /// takes `+nightly`, with isolation off: some tests read files or the clock.
    pub fn test_command(
        &self,
        host: &HostTests,
        workspace: &Path,
        tests: &[String],
        args: &[&str],
    ) -> Command {
        let mut cargo = if host.miri {
            let mut cargo = Command::new("cargo");
            cargo.args(["+nightly", "miri"]).env("MIRIFLAGS", "-Zmiri-disable-isolation");
            cargo
        } else {
            Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        };
        cargo.current_dir(workspace);
        cargo.arg("test");
        for package in &host.packages {
            cargo.args(["-p", package]);
        }
        for test in tests {
            cargo.args(["--test", test]);
        }
        if !host.features.is_empty() {
            cargo.args(["--features", &host.features.join(",")]);
        }
        if let Some(profile) = &host.profile {
            cargo.args(["--profile", profile]);
        }
        cargo.args(args);
        let libtest = libtest_args(host);
        if !libtest.is_empty() {
            cargo.arg("--").args(libtest);
        }
        cargo
    }

    /// Build the test files `tests` of a host-tests case without running them, and return the
    /// test binaries Cargo reports, or, if the build failed, the end of what it said.
    pub fn test_binaries(
        &self,
        host: &HostTests,
        workspace: &Path,
        tests: &[String],
    ) -> Result<Result<Vec<TestBinary>, String>> {
        let args = ["--no-run", "--message-format=json-render-diagnostics"];
        let output = self
            .test_command(host, workspace, tests, &args)
            .stderr(Stdio::piped())
            .output()
            .context("running cargo test --no-run")?;
        if !output.status.success() {
            return Ok(Err(last_lines(&String::from_utf8_lossy(&output.stderr), 12)));
        }
        let mut binaries = Vec::new();
        for line in output.stdout.split(|&b| b == b'\n').filter(|line| line.starts_with(b"{")) {
            let message: Message = serde_json::from_slice(line).context("reading cargo's report")?;
            let (Some(target), Some(path), Some(manifest), Some(profile)) =
                (message.target, message.executable, message.manifest_path, message.profile)
            else {
                continue;
            };
            if message.reason == "compiler-artifact" && profile.test {
                let dir = manifest.parent().context("a manifest path with no directory")?.to_path_buf();
                binaries.push(TestBinary { name: target.name, path, dir });
            }
        }
        Ok(Ok(binaries))
    }

    /// `cargo test` for host packages, for the unit tests a boot cannot reach; with a fanout, as
    /// its jobs (`fanout.rs`). Returns what failed, or `None` if every test passed.
    pub fn cargo_test(&self, name: &str, host: &HostTests) -> Result<Option<String>> {
        let workspace = self.host_test_workspace(host.workspace.as_deref())?;
        if let Some(fanout) = &host.fanout {
            return crate::fanout::run(self, name, host, fanout, &workspace);
        }
        let quiet: &[&str] = if self.verbose { &[] } else { &["--quiet"] };
        let mut cargo = self.test_command(host, &workspace, &host.tests, quiet);
        if !self.verbose {
            cargo.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let output = cargo.output().context("running cargo test")?;
        if self.verbose {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if output.status.success() {
            return Ok(None);
        }
        // The last lines carry the failed assertion and the test's name; the rest is noise.
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        Ok(Some(last_lines(&[out.trim(), err.trim()].join("\n"), 12)))
    }
}

/// What a host-tests case tells its test binaries: its `filter`s, then a `--skip` for each of its
/// `skip`s.
pub fn libtest_args(host: &HostTests) -> Vec<String> {
    let skips = host.skip.iter().flat_map(|skip| ["--skip".to_string(), skip.clone()]);
    host.filter.iter().cloned().chain(skips).collect()
}

/// The last `n` lines of `text`, indented as a result's continuation lines.
pub fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<_> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n      ")
}

/// Rewrite one field of a little-endian ELF in place, or cut it short. Handles ELF32 and ELF64.
fn corrupt_elf(elf: &mut Vec<u8>, corruption: &Corruption) -> Result<()> {
    const PT_LOAD: u32 = 1;
    let is_64 = match elf.get(..5) {
        Some([0x7f, b'E', b'L', b'F', class]) => *class == 2,
        _ => bail!("not an ELF file"),
    };
    let parse = |hex: &str| u64::from_str_radix(hex.trim_start_matches("0x"), 16).context("bad hex address");
    // Addresses are 8 bytes in ELF64 and 4 in ELF32.
    let put = |elf: &mut [u8], at: usize, value: u64| {
        let width = if is_64 { 8 } else { 4 };
        elf[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
    };
    let u16_at = |elf: &[u8], at: usize| u16::from_le_bytes([elf[at], elf[at + 1]]) as usize;

    match corruption {
        Corruption::Truncate(length) => {
            ensure!(*length < elf.len(), "truncate = {length} does not shorten a {}-byte file", elf.len());
            elf.truncate(*length)
        }
        Corruption::Entry(address) => put(elf, 0x18, parse(address)?),
        Corruption::SegmentVaddr(address) => {
            // (e_phoff, e_phentsize, e_phnum, p_vaddr within a program header)
            let (phoff, phentsize, phnum, vaddr_at) = if is_64 {
                (
                    u64::from_le_bytes(elf[0x20..0x28].try_into()?) as usize,
                    u16_at(elf, 0x36),
                    u16_at(elf, 0x38),
                    0x10,
                )
            } else {
                (
                    u32::from_le_bytes(elf[0x1c..0x20].try_into()?) as usize,
                    u16_at(elf, 0x2a),
                    u16_at(elf, 0x2c),
                    0x08,
                )
            };
            let header = (0..phnum)
                .map(|i| phoff + i * phentsize)
                .find(|at| u32::from_le_bytes(elf[*at..*at + 4].try_into().unwrap()) == PT_LOAD)
                .context("ELF has no loadable segment")?;
            put(elf, header + vaddr_at, parse(address)?);
        }
    }
    Ok(())
}

/// The development signing seed (public: see kernel/boot.md). NOT FOR PRODUCTION.
const DEV_SEED: [u8; 32] = [0x42; 32];

/// The `programs` data entry (docs/testbench.md, "Starting a case's programs"): one ASCII line
/// per program after the first, in the case's order, its entry name and then the budgets the
/// tester gives it, separated by spaces.
pub fn programs_entry(programs: &[(String, PathBuf)], budgets: &[&[String]]) -> Vec<u8> {
    let mut text = String::new();
    for ((name, _), budgets) in programs.iter().zip(budgets).skip(1) {
        text.push_str(name);
        for budget in budgets.iter() {
            text.push(' ');
            text.push_str(budget);
        }
        text.push('\n');
    }
    text.into_bytes()
}

/// `manifest` with each of `verified`'s roots and data blocks written into the `verity` of the
/// `volumes` entry of its name (servers/init.md, "Verified volumes"): the pack's own, so the
/// signed manifest pins the disk the case boots. A verified volume the manifest does not verify is
/// refused. With `wrong_root`, the root's first digit is changed: a manifest pinning another root.
pub fn pin_roots(manifest: &[u8], verified: &[Verified], wrong_root: bool) -> Result<Vec<u8>> {
    use serde_json::Value;
    let mut m: Value = serde_json::from_slice(manifest).context("a manifest is JSON")?;
    let volumes = m.get_mut("volumes").and_then(Value::as_array_mut).context("a manifest with volumes")?;
    for v in verified {
        let verity = volumes
            .iter_mut()
            .find(|e| e["name"] == v.name.as_str())
            .and_then(|e| e.get_mut("verity"))
            .and_then(Value::as_object_mut)
            .with_context(|| format!("the manifest does not verify volume {}", v.name))?;
        let mut root = v.root_hex();
        if wrong_root {
            let other = if root.starts_with('0') { "1" } else { "0" };
            root.replace_range(..1, other);
        }
        verity.insert("root".into(), root.into());
        verity.insert("blocks".into(), v.geometry.data_blocks().to_string().into());
    }
    Ok(serde_json::to_vec_pretty(&m)?)
}

/// Build the boot bundle, sign it, and write `signature || tar` to `path`: the kernel, the first
/// program in `init`'s place, the `programs` entry the tester reads (`listing`; a case that
/// brings its own as a file has none here), the other programs, then `files`, the data entries.
/// If `tamper`, flip one payload byte after signing, so the loader must reject it. If
/// `bare_archive`, sign the archive alone instead of the preimage kernel/boot.md states, which
/// the loader must reject too.
pub fn bundle(
    path: &Path,
    kernel: &Path,
    programs: &[(String, PathBuf)],
    listing: Option<&[u8]>,
    files: &[(String, PathBuf)],
    tamper: bool,
    bare_archive: bool,
) -> Result<()> {
    let mut archive = tar::Builder::new(Vec::new());
    let read = |(name, path): &(String, PathBuf)| -> Result<(String, Vec<u8>)> {
        Ok((name.clone(), std::fs::read(path).with_context(|| format!("reading {}", path.display()))?))
    };
    let mut entries = vec![read(&("kernel".to_string(), kernel.to_path_buf()))?];
    let (first, rest) =
        programs.split_first().map_or((&[][..], &[][..]), |(f, r)| (std::slice::from_ref(f), r));
    for program in first {
        entries.push(read(program)?);
    }
    if let Some(listing) = listing {
        entries.push(("programs".to_string(), listing.to_vec()));
    }
    for entry in rest.iter().chain(files) {
        entries.push(read(entry)?);
    }
    let mut names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    names.sort();
    if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
        bail!("two bundle entries are named {:?}", pair[0]);
    }
    for (name, data) in entries {
        append(&mut archive, &name, &data)?;
    }
    let mut tar = archive.into_inner()?;

    let keypair = KeyPair::from_seed(Seed::new(DEV_SEED));
    let signature = keypair.sk.sign(preimage(&tar, bare_archive), None);
    if tamper {
        // Corrupt a payload byte so verification fails, without touching the signature.
        let mid = tar.len() / 2;
        tar[mid] ^= 0xff;
    }

    let mut out = signature.as_ref().to_vec();
    out.extend_from_slice(&tar);
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The bytes a case's signature covers: the preimage `redoubt_signing` defines — the same
/// construction the loader verifies with, so the signer and the verifier cannot drift apart —
/// or, for the case that proves the loader refuses it, the bare archive with no domain and no
/// length, which is what a signer that predates the bundle domain produces.
fn preimage(tar: &[u8], bare_archive: bool) -> Vec<u8> {
    if bare_archive {
        return tar.to_vec();
    }
    let mut preimage = redoubt_signing::bundle_preamble(tar.len() as u64).to_vec();
    preimage.extend_from_slice(tar);
    preimage
}

fn append<W: std::io::Write>(archive: &mut tar::Builder<W>, name: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive.append_data(&mut header, name, data)?;
    Ok(())
}

/// Whether nightly Miri can run; `Err` says what is missing.
pub fn miri_available() -> Result<(), String> {
    match Command::new("cargo").args(["+nightly", "miri", "--version"]).output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => Err("nightly Miri not installed (rustup component add miri --toolchain nightly)".into()),
    }
}

/// Whether every one of `tools` is an executable file in a directory of `path` (`$PATH`'s
/// form); `Err` names the first that is not.
pub fn tools_available(tools: &[String], path: Option<&std::ffi::OsStr>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let dirs: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    for tool in tools {
        let found = dirs.iter().any(|dir| {
            std::fs::metadata(dir.join(tool))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        });
        if !found {
            return Err(format!("{tool} is not on the path (scripts/setup.sh installs it)"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pack's root and block count go into the `verity` of the volume of its name, and
    /// nowhere else; a volume the manifest does not verify is refused; a wrong root differs in
    /// its first digit only.
    #[test]
    fn the_manifest_pins_the_packs_root() {
        let geometry = redoubt_verity::Geometry::new(1000).unwrap();
        let v = Verified { name: "system".into(), root: [0xab; 32], geometry, start: 0, signed: None };
        let image = br#"{ "volumes": [
            { "name": "data", "partition": 0 },
            { "name": "system", "partition": 0, "verity": { "server": "verity:system", "root": "00", "blocks": "1" } }
        ] }"#;
        let pinned: serde_json::Value =
            serde_json::from_slice(&pin_roots(image.as_slice(), &[v.clone()], false).unwrap()).unwrap();
        let system = pinned["volumes"].as_array().unwrap().iter().find(|e| e["name"] == "system").unwrap();
        assert_eq!(system["verity"]["root"], "ab".repeat(32));
        assert_eq!(system["verity"]["blocks"], "1000");
        assert_eq!(system["verity"]["server"], "verity:system");
        let data = pinned["volumes"].as_array().unwrap().iter().find(|e| e["name"] == "data").unwrap();
        assert!(data.get("verity").is_none());
        let wrong: serde_json::Value =
            serde_json::from_slice(&pin_roots(image.as_slice(), &[v.clone()], true).unwrap()).unwrap();
        let root = wrong["volumes"][1]["verity"]["root"].as_str().unwrap().to_string();
        assert_eq!(root, format!("0{}", &"ab".repeat(32)[1..]));
        let other = Verified { name: "data".into(), ..v };
        assert!(pin_roots(image.as_slice(), &[other], false).is_err(), "data is not verified");
    }

    /// A Miri case runs the named test files under nightly Miri with isolation off; a plain case
    /// runs every test target natively.
    #[test]
    fn a_miri_case_runs_its_files_under_miri() {
        let builder = Builder {
            workspace: PathBuf::from("/w"),
            run: PathBuf::from("/w/run"),
            verbose: false,
            staged: Default::default(),
        };
        let args = |host: &HostTests| {
            let cargo = builder.test_command(host, &builder.workspace, &host.tests, &["--quiet"]);
            let miriflags = cargo.get_envs().find(|(k, _)| *k == "MIRIFLAGS").and_then(|(_, v)| v);
            let args: Vec<_> = cargo.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            (args.join(" "), miriflags.map(|v| v.to_string_lossy().into_owned()))
        };
        let miri: HostTests = toml::from_str("packages = ['p']\ntests = ['a', 'b']\nmiri = true").unwrap();
        assert_eq!(
            args(&miri),
            (
                "+nightly miri test -p p --test a --test b --quiet".into(),
                Some("-Zmiri-disable-isolation".into())
            )
        );
        let native: HostTests = toml::from_str("packages = ['p']").unwrap();
        assert_eq!(args(&native), ("test -p p --quiet".into(), None));
        // Cargo's own arguments all come before the test binaries' filters and `--skip`s.
        let release: HostTests = toml::from_str(
            "packages = ['p']\nprofile = 'release'\nfilter = ['f']\nskip = ['slow', 'slower']",
        )
        .unwrap();
        assert_eq!(args(&release).0, "test -p p --profile release --quiet -- f --skip slow --skip slower");
    }

    /// A tool the oracle needs is found on the path only as an executable file; the first
    /// missing one is named.
    #[test]
    fn an_oracles_tools_must_be_on_the_path() {
        let dir = std::env::temp_dir().join(format!("testbench-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plain"), "").unwrap();
        std::fs::write(dir.join("tool"), "").unwrap();
        std::fs::set_permissions(dir.join("tool"), std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), &dir]).unwrap();
        let names = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(tools_available(&names(&["tool"]), Some(&path)), Ok(()));
        assert_eq!(tools_available(&[], None), Ok(()));
        for (tools, missing) in
            [(&["tool", "absent"][..], "absent"), (&["plain"], "plain"), (&["tool"], "tool")]
        {
            let path = if missing == "tool" { None } else { Some(path.as_os_str()) };
            let why = tools_available(&names(tools), path).unwrap_err();
            assert!(why.starts_with(&format!("{missing} is not on the path")), "{why}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A host case may name the beamlet workspace and its fake-kernel feature. Invalid
    /// workspace requests fail before a Cargo command can run at the repository root.
    #[test]
    fn host_tests_route_only_to_a_requested_workspace_and_forward_features() {
        let builder = Builder {
            workspace: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            run: PathBuf::new(),
            verbose: false,
            staged: Default::default(),
        };
        let root = builder.host_test_workspace(None).unwrap();
        assert_eq!(root, builder.workspace.canonicalize().unwrap());
        let otp = builder.host_test_workspace(Some(Path::new("userland/otp"))).unwrap();
        assert_eq!(otp, root.join("userland/otp"));
        let host: HostTests = toml::from_str(
            "packages = ['beamlet-vm', 'beamlet-redoubt']\nworkspace = 'userland/otp'\nfeatures = ['beamlet-redoubt/fake']",
        )
        .unwrap();
        let workspace = builder.host_test_workspace(host.workspace.as_deref()).unwrap();
        let cargo = builder.test_command(&host, &workspace, &host.tests, &[]);
        assert_eq!(cargo.get_current_dir(), Some(otp.as_path()));
        let args: Vec<_> = cargo.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            ["test", "-p", "beamlet-vm", "-p", "beamlet-redoubt", "--features", "beamlet-redoubt/fake"]
        );
        assert!(builder.host_test_workspace(Some(Path::new("userland/missing"))).is_err());
        assert!(builder.host_test_workspace(Some(Path::new("docs"))).is_err());
        assert!(builder.host_test_workspace(Some(Path::new("../userland/otp"))).is_err());
        assert!(builder.host_test_workspace(Some(Path::new("userland/./otp"))).is_err());
        assert!(builder.host_test_workspace(Some(Path::new("userland//otp"))).is_err());
        assert!(builder.host_test_workspace(Some(Path::new("/tmp"))).is_err());
    }

    /// Recipes of one file name in different directories stage apart, within the run.
    #[test]
    fn userland_recipes_of_one_name_stage_apart() {
        let builder = Builder {
            workspace: PathBuf::from("/w"),
            run: PathBuf::from("/w/run"),
            verbose: false,
            staged: Default::default(),
        };
        let image = builder.userland_dir(Path::new("image/userland.toml"));
        let pack = builder.userland_dir(Path::new("tests/data/pack/userland.toml"));
        assert_ne!(image, pack);
        assert!(image.starts_with("/w/run/userland") && pack.starts_with("/w/run/userland"));
    }

    /// A run of zeros is a file of exactly that many zero bytes, in the run's own directory.
    #[test]
    fn zeros_are_a_file_of_that_length_in_the_run() {
        let run = std::env::temp_dir().join(format!("testbench-zeros-{}", std::process::id()));
        std::fs::create_dir_all(&run).unwrap();
        let builder = Builder {
            workspace: PathBuf::from("/w"),
            run: run.clone(),
            verbose: false,
            staged: Default::default(),
        };
        let (name, path) = builder.zeros(5000).unwrap();
        assert_eq!(name, "zeros-5000");
        assert!(path.starts_with(&run));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 5000);
        assert!(bytes.iter().all(|&b| b == 0));
        std::fs::remove_dir_all(&run).unwrap();
    }

    /// The test programs embed a stub their build script builds; the script must rerun when
    /// anything that build reads changes, the manifests and the lock file included, or a bench
    /// run tests a stub built from other inputs than the tree names.
    #[test]
    fn the_test_programs_rebuild_their_stub_on_every_input() {
        let script = include_str!("../../../tests/programs/build.rs");
        for input in [
            "stub/src",
            "stub/link.x",
            "stub/build.rs",
            "stub/Cargo.toml",
            "libs/sys/src",
            "libs/sys/Cargo.toml",
            "libs/wire/src",
            "libs/wire/Cargo.toml",
            "Cargo.toml",
            "Cargo.lock",
        ] {
            assert!(
                script.contains(&format!("\"{input}\"")),
                "tests/programs/build.rs does not watch {input}"
            );
        }
        assert!(script.contains("cargo:rerun-if-changed={}\", workspace.join(input)"));
    }

    /// The archive these tests sign: any fixed bytes will do, since the loader is handed
    /// whatever the container holds.
    const ARCHIVE: &[u8] = b"not really a tar, but signed the same way";

    /// The bench signs with the half of the key pair the loader verifies with: the public key in
    /// `redoubt_signing`, which `init` also asks `keyd` about, is `DEV_SEED`'s.
    #[test]
    fn the_signing_seed_is_the_key_the_loader_trusts() {
        assert_eq!(*KeyPair::from_seed(Seed::new(DEV_SEED)).pk, redoubt_signing::DEV_PUBLIC_KEY);
    }

    /// The seed file the signed-volume cases sign with (`tests/data/verity/dev-seed`) is this
    /// seed, so their manifests' `"key": "bundle"` verifies it.
    #[test]
    fn the_cases_volume_seed_is_the_development_seed() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/verity/dev-seed");
        assert_eq!(std::fs::read(path).unwrap(), DEV_SEED);
    }

    /// What the bench signs is the documented preimage, and nothing else. The loader hashes
    /// `redoubt_signing::bundle_preamble(len)` and then the archive; the bench writes the same
    /// two pieces into one buffer. Both are asserted here against the bytes kernel/boot.md
    /// spells out, so changing either side alone fails before a case boots. The bare archive,
    /// the one forgery a case can ask for, is asserted to be the naked bytes.
    #[test]
    fn the_signed_bytes_are_the_documented_preimage() {
        let signed = preimage(ARCHIVE, false);

        let mut expected = b"redoubt.bundle.v1\x00".to_vec();
        expected.extend_from_slice(&(ARCHIVE.len() as u64).to_le_bytes());
        expected.extend_from_slice(ARCHIVE);
        assert_eq!(signed, expected);

        // And what the loader hashes, in its two pieces, is that same run of bytes.
        let preamble = redoubt_signing::bundle_preamble(ARCHIVE.len() as u64);
        assert_eq!(&signed[..preamble.len()], &preamble);
        assert_eq!(&signed[preamble.len()..], ARCHIVE);

        assert_eq!(preimage(ARCHIVE, true), ARCHIVE);
        assert_ne!(preimage(ARCHIVE, true), signed);
    }

    /// The signature itself, for that archive under the development seed: a golden value that
    /// pins the key, the algorithm and the preimage together. It changes only when the
    /// signature format does — and then every bundle ever signed stops verifying.
    ///
    /// The refusals live here rather than in a boot case: Ed25519 accepts exactly the one
    /// message that was signed, so once the loader boots a real bundle it has already fixed one
    /// preimage, and a foreign domain or a wrong length is refused for free. Only a second
    /// acceptance path in the loader could take them, and the bare-archive boot case is what
    /// catches that.
    #[test]
    fn golden_signature_over_a_known_archive() {
        let keypair = KeyPair::from_seed(Seed::new(DEV_SEED));
        let signature = keypair.sk.sign(preimage(ARCHIVE, false), None);
        let hex: String = signature.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(hex, GOLDEN_SIGNATURE);
        assert!(keypair.pk.verify(preimage(ARCHIVE, false), &signature).is_ok());

        // The bare archive, with no domain and no length.
        assert!(keypair.pk.verify(ARCHIVE, &signature).is_err());

        // Another Redoubt domain over the same archive: a signature made for packages
        // (servers/pkg.md) is not a bundle signature.
        let mut foreign = b"redoubt.pkg.v1\x00".to_vec();
        foreign.extend_from_slice(&(ARCHIVE.len() as u64).to_le_bytes());
        foreign.extend_from_slice(ARCHIVE);
        assert!(keypair.pk.verify(&foreign, &signature).is_err());

        // The right domain with a length that is not the archive's. The loader measures the
        // archive in the container it reads, so only a signer's own count can be wrong.
        let mut wrong_length = redoubt_signing::bundle_preamble(ARCHIVE.len() as u64 + 1).to_vec();
        wrong_length.extend_from_slice(ARCHIVE);
        assert!(keypair.pk.verify(&wrong_length, &signature).is_err());
    }

    /// Two runs build one package with different features in one build cache, interleaved:
    /// A builds, B builds, then A packs, and builds A again. A packs the binary of its own
    /// features though B's build came between, each binary is in its own run's directory and
    /// none in the cache's shared output directory, and A's second build compiles nothing.
    #[test]
    fn interleaved_builds_each_pack_their_own_binary() {
        let workspace = std::env::temp_dir().join(format!("testbench-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n\
             [features]\na = []\nb = []\n",
        )
        .unwrap();
        std::fs::write(
            workspace.join("src/main.rs"),
            "fn main() { println!(\"built with {}\", if cfg!(feature = \"a\") { \"a\" } else { \"b\" }) }\n",
        )
        .unwrap();
        let rustc = Command::new("rustc").arg("-vV").output().unwrap().stdout;
        let host = String::from_utf8(rustc)
            .unwrap()
            .lines()
            .find_map(|l| Some(l.strip_prefix("host: ")?.to_string()));
        let target = Target { name: "host", triple: host.unwrap().leak(), machine: Err("a fixture") };
        let run = |name: &str| Builder {
            workspace: workspace.clone(),
            run: workspace.join(name),
            verbose: false,
            staged: Default::default(),
        };
        let (a, b) = (run("run-a"), run("run-b"));
        let build = |builder: &Builder, feature: &str| {
            builder.binary(&target, "fixture", None, &[feature.to_string()], Profile::Release).unwrap()
        };
        let printed =
            |binary: &Path| String::from_utf8(Command::new(binary).output().unwrap().stdout).unwrap();
        let cache = workspace.join("target").join(target.triple).join("release");
        let compiled = || {
            let mut files: Vec<_> = std::fs::read_dir(cache.join("deps"))
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (e.path(), e.metadata().unwrap().modified().unwrap())
                })
                .collect();
            files.sort();
            files
        };

        let built_a = build(&a, "a");
        let built_b = build(&b, "b");
        assert_eq!(printed(&built_a), "built with a\n");
        assert_eq!(printed(&built_b), "built with b\n");
        assert!(built_a.starts_with(&a.run) && built_b.starts_with(&b.run), "{built_a:?} {built_b:?}");
        assert!(!cache.join("fixture").exists(), "a binary was copied into the shared cache");
        let before = compiled();
        assert_eq!(build(&a, "a"), built_a);
        assert_eq!(compiled(), before, "a build of features already in the cache compiled again");
        // In one run, the binary built without features and with them land apart, so neither
        // replaces the other (`netd`, and `netd` with `restart-probe`).
        let plain = a.binary(&target, "fixture", None, &[], Profile::Release).unwrap();
        assert_ne!(plain, built_a);
        assert_eq!(printed(&plain), "built with b\n");
        assert_eq!(printed(&built_a), "built with a\n");
        std::fs::remove_dir_all(&workspace).unwrap();
    }

    const GOLDEN_SIGNATURE: &str = "c5807e8b49de09f4a03ed502f87a867aded52a6f0badeca8db993d425d3d457fbf75559b65473006d1355efc665fd79952e5c16b7a9582c5f40dccc432310a04";
}
