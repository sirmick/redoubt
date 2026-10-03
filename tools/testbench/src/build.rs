//! Building the pieces of a boot test with cargo, and packing the boot bundle.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use ed25519_compact::{KeyPair, Seed};
use serde::Deserialize;

use crate::case::{Corruption, HostTests, Program};
use crate::target::Target;

pub struct Builder {
    pub workspace: PathBuf,
    /// This run's own directory (`run.rs`): the binaries it packs and boots, and every file it
    /// makes from them, are copied or written here and nowhere another run writes.
    pub run: PathBuf,
    pub verbose: bool,
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
}

#[derive(Deserialize)]
struct MessageTarget {
    name: String,
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

    /// A file of `len` zero bytes in this run, named for its length.
    fn zeros(&self, len: u64) -> Result<(String, PathBuf)> {
        let name = format!("zeros-{len}");
        let path = self.run.join(&name);
        let file = std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        file.set_len(len).with_context(|| format!("sizing {}", path.display()))?;
        Ok((name, path))
    }

    /// Runs `command` with the pinned Erlang toolchain on the path (`userland/otp/tools/env.sh`),
    /// from the workspace root, and returns what it printed. A VM that crashes writes no
    /// `erl_crash.dump` into the tree.
    fn erlang(&self, command: &[&str]) -> Result<String> {
        let output = Command::new("bash")
            .current_dir(&self.workspace)
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

    /// `cargo test` for host packages, for the unit tests a boot cannot reach. Returns what
    /// failed, or `None` if every test passed.
    /// The `cargo test` a host-tests case runs. Under Miri it goes through rustup's `cargo`,
    /// which alone takes `+nightly`, with isolation off: some tests read files or the clock.
    fn test_command(&self, host: &HostTests) -> Command {
        let mut cargo = if host.miri {
            let mut cargo = Command::new("cargo");
            cargo.args(["+nightly", "miri"]).env("MIRIFLAGS", "-Zmiri-disable-isolation");
            cargo
        } else {
            Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        };
        cargo.current_dir(&self.workspace);
        cargo.arg("test");
        for package in &host.packages {
            cargo.args(["-p", package]);
        }
        for test in &host.tests {
            cargo.args(["--test", test]);
        }
        cargo
    }

    pub fn cargo_test(&self, host: &HostTests) -> Result<Option<String>> {
        let mut cargo = self.test_command(host);
        if !self.verbose {
            cargo.arg("--quiet").stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let output = cargo.output().context("running cargo test")?;
        if output.status.success() {
            return Ok(None);
        }
        // The last lines carry the failed assertion and the test's name; the rest is noise.
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        let reason = [out.trim(), err.trim()].map(str::to_string).join("\n");
        Ok(Some(
            reason
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n      "),
        ))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A Miri case runs the named test files under nightly Miri with isolation off; a plain case
    /// runs every test target natively.
    #[test]
    fn a_miri_case_runs_its_files_under_miri() {
        let builder =
            Builder { workspace: PathBuf::from("/w"), run: PathBuf::from("/w/run"), verbose: false };
        let args = |host: &HostTests| {
            let cargo = builder.test_command(host);
            let miriflags = cargo.get_envs().find(|(k, _)| *k == "MIRIFLAGS").and_then(|(_, v)| v);
            let args: Vec<_> = cargo.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            (args.join(" "), miriflags.map(|v| v.to_string_lossy().into_owned()))
        };
        let miri = HostTests { packages: vec!["p".into()], tests: vec!["a".into(), "b".into()], miri: true };
        assert_eq!(
            args(&miri),
            ("+nightly miri test -p p --test a --test b".into(), Some("-Zmiri-disable-isolation".into()))
        );
        let native = HostTests { packages: vec!["p".into()], tests: Vec::new(), miri: false };
        assert_eq!(args(&native), ("test -p p".into(), None));
    }

    /// A run of zeros is a file of exactly that many zero bytes, in the run's own directory.
    #[test]
    fn zeros_are_a_file_of_that_length_in_the_run() {
        let run = std::env::temp_dir().join(format!("testbench-zeros-{}", std::process::id()));
        std::fs::create_dir_all(&run).unwrap();
        let builder = Builder { workspace: PathBuf::from("/w"), run: run.clone(), verbose: false };
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
        let run =
            |name: &str| Builder { workspace: workspace.clone(), run: workspace.join(name), verbose: false };
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
