//! Building the pieces of a boot test with cargo, and packing the boot bundle.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::case::Program;
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
            Program::TestProgram(bin) => ("test-programs", bin.as_str()),
            Program::Package { package, bin } => (package.as_str(), bin.as_str()),
        };
        self.cargo(target, package, Some(bin), &[])?;
        Ok((bin.to_string(), self.out_dir(target).join(bin)))
    }

    pub fn artifact(&self, target: &Target, name: &str) -> PathBuf { self.out_dir(target).join(name) }
}

/// Pack the boot bundle: a ustar archive with the kernel first, then the programs in PID order.
pub fn bundle(path: &Path, kernel: &Path, programs: &[(String, PathBuf)]) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut archive = tar::Builder::new(file);
    let entries = std::iter::once(("kernel".to_string(), kernel.to_path_buf())).chain(programs.iter().cloned());
    for (name, elf) in entries {
        let data = std::fs::read(&elf).with_context(|| format!("reading {}", elf.display()))?;
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append_data(&mut header, &name, data.as_slice())?;
    }
    archive.finish()?;
    Ok(())
}
