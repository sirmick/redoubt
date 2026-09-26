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

    /// `cargo test` for host packages, for the unit tests a boot cannot reach. Returns what
    /// failed, or `None` if every test passed.
    pub fn cargo_test(&self, packages: &[String]) -> Result<Option<String>> {
        let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cargo.current_dir(&self.workspace).arg("test");
        for package in packages {
            cargo.args(["-p", package]);
        }
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
        Ok(Some(reason.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n      ")))
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
/// The development signing seed (public: see kernel/boot.md). NOT FOR PRODUCTION.
const DEV_SEED: [u8; 32] = [0x42; 32];

/// Build the boot bundle, sign it, and write `signature || tar` to `path`. `files` are data
/// entries, placed after the programs. If `tamper`, flip one payload byte after signing, so
/// the loader must reject it. If `bare_archive`, sign the archive alone instead of the preimage
/// kernel/boot.md states, which the loader must reject too.
pub fn bundle(
    path: &Path,
    kernel: &Path,
    programs: &[(String, PathBuf)],
    files: &[(String, PathBuf)],
    tamper: bool,
    bare_archive: bool,
) -> Result<()> {
    let mut archive = tar::Builder::new(Vec::new());
    let entries: Vec<_> =
        std::iter::once(("kernel".to_string(), kernel.to_path_buf())).chain(programs.iter().chain(files).cloned()).collect();
    let mut names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    names.sort();
    if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
        bail!("two bundle entries are named {:?}", pair[0]);
    }
    for (name, elf) in entries {
        let data = std::fs::read(&elf).with_context(|| format!("reading {}", elf.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The archive these tests sign: any fixed bytes will do, since the loader is handed
    /// whatever the container holds.
    const ARCHIVE: &[u8] = b"not really a tar, but signed the same way";

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

    const GOLDEN_SIGNATURE: &str = "c5807e8b49de09f4a03ed502f87a867aded52a6f0badeca8db993d425d3d457fbf75559b65473006d1355efc665fd79952e5c16b7a9582c5f40dccc432310a04";
}
