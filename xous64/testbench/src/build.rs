//! Building the pieces of a boot test with cargo, and packing the boot bundle.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::case::{Corruption, Program};
use crate::target::Target;

pub struct Builder {
    pub workspace: PathBuf,
    pub verbose: bool,
}

impl Builder {
    fn out_dir(&self, target: &Target) -> PathBuf { self.workspace.join("target").join(target.triple).join("release") }

    /// `cargo build --release` one package for `target`.
    pub fn cargo_build(&self, target: &Target, package: &str, features: &[String]) -> Result<()> {
        self.cargo(target, package, None, features)
    }

    fn cargo(&self, target: &Target, package: &str, bin: Option<&str>, features: &[String]) -> Result<()> {
        let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cargo.current_dir(&self.workspace).args(["build", "--release", "--target", target.triple, "-p", package]);
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
                self.cargo(target, "test-programs", Some(corrupt), &[])?;
                let mut elf = std::fs::read(self.out_dir(target).join(corrupt))?;
                corrupt_elf(&mut elf, with)?;
                let name = format!("{corrupt}-corrupted");
                let path = self.workspace.join("target/testbench").join(format!("{name}-{}.elf", target.name));
                std::fs::write(&path, elf)?;
                return Ok((name, path));
            }
            Program::TestProgram(bin) => ("test-programs", bin.as_str()),
            Program::Package { package, bin } => (package.as_str(), bin.as_str()),
        };
        self.cargo(target, package, Some(bin), &[])?;
        Ok((bin.to_string(), self.out_dir(target).join(bin)))
    }

    pub fn artifact(&self, target: &Target, name: &str) -> PathBuf { self.out_dir(target).join(name) }
}

/// Rewrite one field of a little-endian ELF in place. Handles ELF32 and ELF64.
fn corrupt_elf(elf: &mut [u8], corruption: &Corruption) -> Result<()> {
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
pub fn bundle(path: &Path, kernel: &Path, programs: &[(String, PathBuf)], manifest: &str) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut archive = tar::Builder::new(file);
    let entries = std::iter::once(("kernel".to_string(), kernel.to_path_buf())).chain(programs.iter().cloned());
    for (name, elf) in entries {
        let data = std::fs::read(&elf).with_context(|| format!("reading {}", elf.display()))?;
        append(&mut archive, &name, &data)?;
    }
    // The device-grant manifest, if any, rides in the bundle as a `grants` entry.
    if !manifest.is_empty() {
        append(&mut archive, "grants", manifest.as_bytes())?;
    }
    archive.finish()?;
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
