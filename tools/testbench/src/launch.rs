//! `./launch --system`: the whole image booted by hand, with its disks and network, and the line
//! to log in over SSH. The machine is the `launch-system` case's (`tests/launch-system.toml`),
//! assembled by the bench's own builder, packer and device arguments, so what a person boots is
//! what that case boots. Alice's login key is a development key made once per checkout under
//! `$REDOUBT_TMP/launch`, or the user's own (`--key`), given to alice in this boot's copy of the
//! manifest only.

use std::net::TcpListener;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::build::Builder;
use crate::case::{Boot, Case, Kind};
use crate::prebuilt::Packed;
use crate::qemu::{self, Image};
use crate::target::Machine;
use crate::{Outcome, target};

/// The case whose machine `--system` boots.
pub const CASE: &str = "tests/launch-system.toml";

/// The principal the login key is given to.
const PRINCIPAL: &str = "alice";

/// What `--run --system` was asked for.
pub struct System<'a> {
    pub arch: &'a str,
    pub smp: u32,
    pub ssh_port: u16,
    /// The user's own public key, instead of the checkout's development key.
    pub key: Option<&'a Path>,
    pub print_only: bool,
    /// Write the QEMU command here, each argument ended by a NUL, for `./launch` to boot.
    pub argv: Option<&'a Path>,
}

/// Build, pack and print; boot unless `print_only`.
pub fn system(builder: &Builder, workspace: &Path, logs: &Path, ask: &System) -> Result<()> {
    // The port first: a taken one is refused before minutes of building.
    TcpListener::bind(("127.0.0.1", ask.ssh_port))
        .with_context(|| format!("host port {} is taken; choose another with --ssh-port", ask.ssh_port))?;
    let (public, identity) = login_key(workspace, ask.key)?;

    let mut case = Case::load(&workspace.join(CASE))?;
    let Kind::Boot(boot) = &mut case.kind else { bail!("{CASE} is not a boot case") };
    boot.smp = vec![ask.smp];
    boot.login_key = Some(public);

    let target = target::find(ask.arch).with_context(|| format!("unknown arch {}", ask.arch))?;
    let machine =
        target.machine.as_ref().map_err(|why| anyhow::anyhow!("{} cannot boot: {why}", target.name))?;
    let packed = match crate::pack_case(builder, &case, target, logs)? {
        Ok(packed) => packed,
        Err(results) => match results.into_iter().next() {
            Some((_, Outcome::Fail(why) | Outcome::Skip(why), _)) => bail!("{why}"),
            _ => bail!("the image did not build"),
        },
    };
    let Kind::Boot(boot) = &case.kind else { unreachable!("checked above") };
    let firmware = crate::rustsbi_prototyper(target).map_err(|why| anyhow::anyhow!(why))?;
    let disk = logs.join(format!("{}-{}.img", case.name, target.name));
    let qemu = command(machine, &firmware, &packed, boot, &disk, ask.smp, ask.ssh_port)?;
    let fingerprint = host_key_fingerprint(&std::fs::read(packed.bundle.with_extension("manifest.json"))?)?;

    eprintln!("+ {}", qemu::shell_line(&qemu));
    if let Some(path) = ask.argv {
        std::fs::write(path, nul_separated(&qemu)).with_context(|| format!("writing {}", path.display()))?;
    }
    let identity =
        identity.map(|i| format!(" -i {}", qemu::shell_quote(&i.display().to_string()))).unwrap_or_default();
    eprintln!(
        "\nRedoubt {arch}, {smp} harts, {mib} MiB. This terminal is the serial console, alice's console \
         session; Ctrl-A X quits QEMU.\nThe disk is new at every launch: files written in this boot are \
         gone at the next.\nOnce the console prints \"sshd: listening on port 22\", log in from another \
         terminal:\n\n  ssh -p {port}{identity} {PRINCIPAL}@localhost\n\nHost key: {fingerprint} (ED25519), \
         the image's development host key\n",
        arch = target.name,
        smp = ask.smp,
        mib = boot.memory_mib.unwrap_or(target::DEFAULT_MEMORY_MIB),
        port = ask.ssh_port,
    );
    if ask.print_only {
        return Ok(());
    }
    qemu::run_to_end(qemu, machine.qemu)
}

/// The QEMU command that boots the launch machine's `packed` image at `smp` harts, its disk
/// packed afresh at `disk` and the guest's port 22 forwarded from the host's `port`: what
/// `./launch --system` boots, and the `launch-system` case.
pub fn command(
    machine: &Machine,
    firmware: &str,
    packed: &Packed,
    boot: &Boot,
    disk: &Path,
    smp: u32,
    port: u16,
) -> Result<Command> {
    // The principals' homes and the vault's tree, as `./mkimage` makes them: the disk's stages.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for dir in ["target/image/stage/home/alice", "target/image/stage/home/bob", "target/image/vault"] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    let (devices, _) = qemu::virtio_devices(boot, disk, packed.userland.as_ref(), &[port])?;
    let image = Image {
        machine,
        firmware,
        loader: &packed.loader,
        bundle: &packed.bundle,
        smp,
        memory_mib: boot.memory_mib.unwrap_or(target::DEFAULT_MEMORY_MIB),
        devices: &devices,
    };
    Ok(image.interactive(false))
}

/// `manifest` with alice's login keys replaced by `key` alone.
pub fn give_key(manifest: &[u8], key: &str) -> Result<Vec<u8>> {
    let mut m: Value = serde_json::from_slice(manifest).context("a manifest is JSON")?;
    let alice = m["principals"]
        .as_array_mut()
        .and_then(|p| p.iter_mut().find(|p| p["name"] == PRINCIPAL))
        .context("the manifest has no principal alice")?;
    alice["ssh_keys"] = Value::Array(vec![key.into()]);
    Ok(serde_json::to_vec_pretty(&m)?)
}

/// The private key the `launch-system` case's sessions log in with, beside its `bundle`.
pub fn case_identity(bundle: &Path) -> PathBuf { bundle.with_extension("id_ed25519") }

/// The public key line of the case's key ([`case_identity`]), made the first time.
pub fn case_key(bundle: &Path) -> Result<String> {
    let private = case_identity(bundle);
    let public = private.with_extension("id_ed25519.pub");
    if !private.exists() {
        keygen(&private)?;
    }
    public_line(&public)
}

/// An Ed25519 key pair with no passphrase at `private` and `private.pub`, by OpenSSH's own tool.
fn keygen(private: &Path) -> Result<()> {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "redoubt-launch", "-f"])
        .arg(private)
        .status()
        .context("running ssh-keygen (OpenSSH's client tools)")?;
    ensure!(status.success(), "ssh-keygen failed: {status}");
    Ok(())
}

/// The public key line alice gets, and the private half `ssh -i` names, if known: the user's own
/// (`key`, a `.pub` file; its private half beside it, if there), or the checkout's development
/// key, made the first time.
fn login_key(workspace: &Path, key: Option<&Path>) -> Result<(String, Option<PathBuf>)> {
    let (public, identity) = match key {
        Some(public) => {
            let private = public.with_extension("");
            let identity =
                (public.extension().is_some_and(|e| e == "pub") && private.exists()).then_some(private);
            (public.to_path_buf(), identity)
        }
        None => {
            let tmp = std::env::var_os("REDOUBT_TMP").map_or_else(|| workspace.join(".tmp"), PathBuf::from);
            let dir = tmp.join("launch");
            std::fs::create_dir_all(&dir)?;
            let private = dir.join("id_ed25519");
            if !private.exists() {
                keygen(&private)?;
            }
            (private.with_extension("pub"), Some(private))
        }
    };
    Ok((public_line(&public)?, identity))
}

/// The manifest's form of the Ed25519 public key in the file `public`: the type and the key,
/// without the comment.
fn public_line(public: &Path) -> Result<String> {
    let text = std::fs::read_to_string(public).with_context(|| format!("reading {}", public.display()))?;
    match text.split_whitespace().take(2).collect::<Vec<_>>()[..] {
        ["ssh-ed25519", key] => Ok(format!("ssh-ed25519 {key}")),
        _ => bail!("{}: the login key must be an Ed25519 public key (ssh-ed25519 ...)", public.display()),
    }
}

/// The SSH host key's fingerprint, as `ssh` prints it (`SHA256:` and unpadded base64 of the
/// key blob's hash), from the `ssh_host` seed in `keyd`'s entry of `manifest`.
fn host_key_fingerprint(manifest: &[u8]) -> Result<String> {
    Ok(format!("SHA256:{}", base64(&Sha256::digest(key_blob(&host_key(manifest)?)))))
}

/// The SSH host key's public half, from the `ssh_host` seed in `keyd`'s entry of `manifest`.
fn host_key(manifest: &[u8]) -> Result<[u8; 32]> {
    let m: Value = serde_json::from_slice(manifest).context("a manifest is JSON")?;
    let keyd = m["servers"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "keyd"))
        .context("the manifest has no keyd")?;
    let seed = keyd["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|arg| match arg.split(',').collect::<Vec<_>>()[..] {
            [_, "ssh_host", seed] => Some(seed),
            _ => None,
        })
        .context("keyd holds no ssh_host key")?;
    ensure!(seed.len() == 64, "an ssh_host seed is 64 hex digits");
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&seed[2 * i..2 * i + 2], 16).context("an ssh_host seed is hex")?;
    }
    Ok(*ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(bytes)).pk)
}

/// The SSH wire form of an Ed25519 public key: the algorithm's name, then the key, each a
/// length-prefixed string (RFC 8709).
fn key_blob(public: &[u8]) -> Vec<u8> {
    let mut blob = Vec::new();
    for part in [&b"ssh-ed25519"[..], public] {
        blob.extend((part.len() as u32).to_be_bytes());
        blob.extend(part);
    }
    blob
}

/// Standard base64 without padding, as OpenSSH prints fingerprints.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// The command's program and arguments, each ended by a NUL.
fn nul_separated(cmd: &Command) -> Vec<u8> {
    let mut out = Vec::new();
    for part in std::iter::once(cmd.get_program()).chain(cmd.get_args()) {
        out.extend(part.as_bytes());
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fingerprint of the image's host key, seed 11..11, is the one `ssh-keygen -lf` prints
    /// for its public key:
    /// `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINBKsjJ0K7SrOhNovUYV5ObQIkq3GgFrr4UgozLJd4c3`.
    #[test]
    fn the_host_key_fingerprint_is_ssh_s() {
        let manifest = br#"{"servers": [{"name": "keyd", "args": [
            "host,ssh_host,1111111111111111111111111111111111111111111111111111111111111111"]}]}"#;
        assert_eq!(host_key_fingerprint(manifest).unwrap(), FINGERPRINT);
        assert!(host_key_fingerprint(br#"{"servers": [{"name": "keyd", "args": []}]}"#).is_err());
    }

    const FINGERPRINT: &str = "SHA256:2HS8bJpYBy7bDfWIQN304coFKT0s6CVPABFLYq+IX5E";

    /// The host key the `launch-system` case's sessions insist on is the one launch prints the
    /// fingerprint of: the image manifest's `ssh_host` key. (A blob of 51 bytes needs no base64
    /// padding, so the unpadded form is the key line's.)
    #[test]
    fn the_case_expects_the_host_key_launch_prints() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest = std::fs::read(root.join("image/manifest.json")).unwrap();
        let line = format!("ssh-ed25519 {}", base64(&key_blob(&host_key(&manifest).unwrap())));
        let case = Case::load(&root.join(CASE)).unwrap();
        let Kind::Boot(boot) = &case.kind else { panic!("a boot case") };
        assert!(boot.launch);
        assert_eq!(boot.net.as_ref().unwrap().host_key.as_deref(), Some(line.as_str()));
    }

    /// alice's keys are replaced by the one key; bob and the rest are as they were.
    #[test]
    fn alice_alone_gets_the_key() {
        let base = br#"{"servers": [{"name": "keyd"}], "principals": [
            {"name": "alice", "account": "1001", "ssh_keys": ["ssh-ed25519 OLD", "ssh-ed25519 OLD2"]},
            {"name": "bob", "ssh_keys": ["ssh-ed25519 BOB"]}]}"#;
        let given: Value = serde_json::from_slice(&give_key(base, "ssh-ed25519 NEW").unwrap()).unwrap();
        let expected = serde_json::json!({"servers": [{"name": "keyd"}], "principals": [
            {"name": "alice", "account": "1001", "ssh_keys": ["ssh-ed25519 NEW"]},
            {"name": "bob", "ssh_keys": ["ssh-ed25519 BOB"]},
        ]});
        assert_eq!(given, expected);
        assert!(give_key(br#"{"principals": [{"name": "bob"}]}"#, "ssh-ed25519 NEW").is_err());
    }

    #[test]
    fn base64_matches_rfc_4648_without_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg");
        assert_eq!(base64(b"fo"), "Zm8");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_argv_file_ends_each_argument_with_a_nul() {
        let mut cmd = Command::new("qemu");
        cmd.args(["-m", "1024M", "a b"]);
        assert_eq!(nul_separated(&cmd), b"qemu\0-m\01024M\0a b\0");
    }
}
