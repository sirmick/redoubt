//! Building the pieces of a boot test with cargo, and packing the boot bundle.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, ensure, Context, Result};
use ed25519_compact::{KeyPair, Seed};

use crate::case::{Corruption, Program};
use crate::target::Target;

pub struct Builder {
    pub workspace: PathBuf,
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

impl Builder {
    fn out_dir(&self, target: &Target, profile: Profile) -> PathBuf {
        self.workspace.join("target").join(target.triple).join(profile.name())
    }

    /// `cargo build` one package for `target` with `profile`.
    pub fn cargo_build(&self, target: &Target, package: &str, features: &[String], profile: Profile) -> Result<()> {
        self.cargo(target, package, None, features, profile)
    }

    fn cargo(
        &self,
        target: &Target,
        package: &str,
        bin: Option<&str>,
        features: &[String],
        profile: Profile,
    ) -> Result<()> {
        let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cargo.current_dir(&self.workspace).args(["build", "--profile", profile.name(), "--target", target.triple, "-p", package]);
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
            bail!("building {what} for {} failed: {}", target.name, String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

    /// Build `program` if it comes from the workspace, and return the path of its ELF.
    pub fn program(&self, target: &Target, program: &Program) -> Result<(String, PathBuf)> {
        let (package, bin) = match program {
            Program::Path { path } => {
                let name = path.file_name().context("program path has no file name")?.to_string_lossy().into_owned();
                return Ok((name, self.workspace.join(path)));
            }
            Program::Corrupted { corrupt, with } => {
                self.cargo(target, "test-programs", Some(corrupt), &[], Profile::Release)?;
                let mut elf = std::fs::read(self.out_dir(target, Profile::Release).join(corrupt))?;
                corrupt_elf(&mut elf, with)?;
                let name = format!("{corrupt}-corrupted");
                let path = self.workspace.join("target/testbench").join(format!("{name}-{}.elf", target.name));
                std::fs::write(&path, elf)?;
                return Ok((name, path));
            }
            Program::TestProgram(bin) => ("test-programs", bin.as_str()),
            Program::Package { package, bin } => (package.as_str(), bin.as_str()),
        };
        self.cargo(target, package, Some(bin), &[], Profile::Release)?;
        Ok((bin.to_string(), self.out_dir(target, Profile::Release).join(bin)))
    }

    pub fn artifact(&self, target: &Target, name: &str, profile: Profile) -> PathBuf {
        self.out_dir(target, profile).join(name)
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
                (u64::from_le_bytes(elf[0x20..0x28].try_into()?) as usize, u16_at(elf, 0x36), u16_at(elf, 0x38), 0x10)
            } else {
                (u32::from_le_bytes(elf[0x1c..0x20].try_into()?) as usize, u16_at(elf, 0x2a), u16_at(elf, 0x2c), 0x08)
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

/// Pack the boot bundle: a ustar archive with the kernel first, then the programs in PID order.
/// The development signing seed (public: see VERIFIED-BOOT.md). NOT FOR PRODUCTION.
const DEV_SEED: [u8; 32] = [0x42; 32];

/// Build the boot bundle, sign it, and write `signature || tar` to `path`. `files` are data
/// entries, placed after the programs. If `tamper`, flip one payload byte after signing, so
/// the loader must reject it.
pub fn bundle(
    path: &Path,
    kernel: &Path,
    programs: &[(String, PathBuf)],
    files: &[(String, PathBuf)],
    manifest: &str,
    tamper: bool,
) -> Result<()> {
    let mut archive = tar::Builder::new(Vec::new());
    let entries: Vec<_> =
        std::iter::once(("kernel".to_string(), kernel.to_path_buf())).chain(programs.iter().chain(files).cloned()).collect();
    let mut names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).chain(["grants"]).collect();
    names.sort();
    if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
        bail!("two bundle entries are named {:?}", pair[0]);
    }
    for (name, elf) in entries {
        let data = std::fs::read(&elf).with_context(|| format!("reading {}", elf.display()))?;
        append(&mut archive, &name, &data)?;
    }
    // The device-grant manifest, if any, rides in the bundle as a `grants` entry.
    if !manifest.is_empty() {
        append(&mut archive, "grants", manifest.as_bytes())?;
    }
    let mut tar = archive.into_inner()?;

    let keypair = KeyPair::from_seed(Seed::new(DEV_SEED));
    let signature = keypair.sk.sign(&tar, None);
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

fn append<W: std::io::Write>(archive: &mut tar::Builder<W>, name: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive.append_data(&mut header, name, data)?;
    Ok(())
}
