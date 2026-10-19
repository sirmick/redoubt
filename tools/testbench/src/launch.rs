//! `./launch --system`: the whole image booted by hand, with its disks and network, and the lines
//! to log in over SSH. The machine is the `launch-system` case's (`tests/launch-system.toml`),
//! assembled by the bench's own builder, packer and device arguments, so what a person boots is
//! what that case boots. Alice's login key is a development key made once per checkout under
//! `$REDOUBT_TMP/launch`, or the user's own (`--key`), given to alice in this boot's copy of the
//! manifest only; agent, the image's third principal, gets a second development key there the
//! same way (`--agent-key`). The data disk is new at every launch, or with `--keep-disk` kept
//! there too, refused if the image's disk is laid out otherwise since and started over by
//! `--fresh-disk`; a kept disk is one launch's at a time, locked before anything is built or
//! written. The ssh lines are printed with the QEMU command, and again once QEMU runs and the
//! guest answers a connection through the forward: until then nothing holds the host port, and
//! `ssh` is refused.

use std::ffi::OsStr;
use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::build::Builder;
use crate::case::{Boot, Case, Kind, Smp};
use crate::prebuilt::Packed;
use crate::qemu::{self, Image};
use crate::target::Machine;
use crate::{Outcome, target};

/// The case whose machine `--system` boots.
pub const CASE: &str = "tests/launch-system.toml";

/// The principal the login key is given to, and its development key's name under the launch
/// directory.
pub const PRINCIPAL: &str = "alice";
const DEVELOPMENT_KEY: &str = "id_ed25519";

/// The image's third principal, a sponsor-run agent's, and its own development key's name.
pub const AGENT: &str = "agent";
const AGENT_DEVELOPMENT_KEY: &str = "agent_ed25519";

/// What `--run --system` was asked for.
pub struct System<'a> {
    pub arch: &'a str,
    pub smp: u32,
    pub ssh_port: u16,
    /// The user's own public key for alice, instead of the checkout's development key.
    pub key: Option<&'a Path>,
    /// The user's own public key for agent, instead of the checkout's second development key.
    pub agent_key: Option<&'a Path>,
    pub print_only: bool,
    /// Write the QEMU command here, each argument ended by a NUL, for `./launch` to boot.
    pub argv: Option<&'a Path>,
    /// Boot the QEMU command written there by an earlier run, instead of building: `./launch`'s
    /// boot step, under a lease of its own.
    pub boot_argv: Option<&'a Path>,
    /// Keep the data disk across launches, under `$REDOUBT_TMP/launch`, instead of a new one.
    pub keep_disk: bool,
    /// With `keep_disk`, start the kept disk over from the image's.
    pub fresh_disk: bool,
}

/// Build, pack and print; boot unless `print_only`.
pub fn system(builder: &Builder, workspace: &Path, logs: &Path, ask: &System) -> Result<()> {
    if let Some(path) = ask.boot_argv {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        std::fs::remove_file(path).ok();
        let (_, identity) = login_key(workspace, ask.key, DEVELOPMENT_KEY)?;
        let (_, agent_identity) = login_key(workspace, ask.agent_key, AGENT_DEVELOPMENT_KEY)?;
        let lines = ssh_lines(ask.ssh_port, identity.as_deref(), agent_identity.as_deref());
        return run(from_nul_separated(&bytes)?, ask.ssh_port, &lines);
    }
    // The port first: a taken one is refused before minutes of building.
    TcpListener::bind(("127.0.0.1", ask.ssh_port))
        .with_context(|| format!("host port {} is taken; choose another with --ssh-port", ask.ssh_port))?;
    let (public, identity) = login_key(workspace, ask.key, DEVELOPMENT_KEY)?;
    let (agent_public, agent_identity) = login_key(workspace, ask.agent_key, AGENT_DEVELOPMENT_KEY)?;

    let mut case = Case::load(&workspace.join(CASE))?;
    let Kind::Boot(boot) = &mut case.kind else { bail!("{CASE} is not a boot case") };
    boot.smp = Smp::List(vec![ask.smp]);
    boot.login_key = Some(public);
    boot.agent_key = Some(agent_public);
    let kept =
        ask.keep_disk.then(|| launch_dir(workspace).map(|dir| dir.join(format!("disk-{}.img", ask.arch))));
    let kept = kept.transpose()?;
    // Held until the boot ends, or the caller holds it for the boot it runs (`./launch`).
    let _lock = kept.as_deref().map(lock_kept).transpose()?;
    if let Some(path) = &kept {
        boot.disk.as_mut().context("the launch machine has no disk")?.keep = true;
        if ask.fresh_disk && path.exists() {
            std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        }
    }

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
    let disk = kept.clone().unwrap_or_else(|| logs.join(format!("{}-{}.img", case.name, target.name)));
    let made = !disk.exists();
    let qemu = command(machine, &firmware, &packed, boot, &disk, ask.smp, ask.ssh_port)?;
    let fingerprint = host_key_fingerprint(&std::fs::read(packed.bundle.with_extension("manifest.json"))?)?;

    eprintln!("+ {}", qemu::shell_line(&qemu));
    if let Some(path) = ask.argv {
        std::fs::write(path, nul_separated(&qemu)).with_context(|| format!("writing {}", path.display()))?;
    }
    let lines = ssh_lines(ask.ssh_port, identity.as_deref(), agent_identity.as_deref());
    let disk = match (&kept, made) {
        (None, _) => "The disk is new at every launch: files written in this boot are gone at the next \
                      (--keep-disk keeps them)."
            .to_string(),
        (Some(path), true) => format!(
            "The disk is kept at {}, new from the image now: files written survive to the next launch \
             with --keep-disk.",
            path.display()
        ),
        (Some(path), false) => format!(
            "The disk is the one kept at {}, as the last launch left it; --fresh-disk starts it over \
             from the image's.",
            path.display()
        ),
    };
    eprintln!(
        "\nRedoubt {arch}, {smp} harts, {mib} MiB. This terminal is the serial console, alice's console \
         session; Ctrl-A X quits QEMU.\n{disk}\nLog in from another terminal once launch says \"port \
         {port} forwards to the guest\" (QEMU runs then; until it does, nothing holds the port and ssh \
         is refused), as alice, or as agent, the image's third principal:\n\n  {lines}\n\n\
         Host key: {fingerprint} (ED25519), the image's development host key\n",
        arch = target.name,
        smp = ask.smp,
        mib = boot.memory_mib.unwrap_or(target::DEFAULT_MEMORY_MIB),
        port = ask.ssh_port,
        lines = lines.join("\n  "),
    );
    if ask.print_only {
        return Ok(());
    }
    run(qemu, ask.ssh_port, &lines)
}

/// The line that logs in as `principal` through the forward from the host's `port`, with
/// `-i identity` when the private key's path is known.
pub fn ssh_line(port: u16, principal: &str, identity: Option<&Path>) -> String {
    let identity =
        identity.map(|i| format!(" -i {}", qemu::shell_quote(&i.display().to_string()))).unwrap_or_default();
    format!("ssh -p {port}{identity} {principal}@localhost")
}

/// The two lines launch prints, alice's then agent's, each with its own key.
fn ssh_lines(port: u16, identity: Option<&Path>, agent_identity: Option<&Path>) -> [String; 2] {
    [ssh_line(port, PRINCIPAL, identity), ssh_line(port, AGENT, agent_identity)]
}

/// Boot `qemu` with this terminal as its console, and once the guest answers through the forward
/// from the host's `port`, say so with the ssh `lines`; QEMU's end is the boot's.
pub fn run(mut qemu: Command, port: u16, lines: &[String]) -> Result<()> {
    let binary = qemu.get_program().to_string_lossy().into_owned();
    let mut child = qemu.spawn().with_context(|| format!("starting {binary}"))?;
    // QEMU has the terminal in raw mode: lines end in \r\n.
    let lines = lines.join("\r\n  ");
    match prove_forward(port, &mut child)? {
        Answer::Banner(banner) => eprint!(
            "\r\nport {port} forwards to the guest's sshd ({banner}; its \"connection 1\" on the console \
             was this check). Log in from another terminal:\r\n\r\n  {lines}\r\n\r\n"
        ),
        Answer::Closed => eprint!(
            "\r\nport {port} forwards to the guest. Once the console prints \"sshd: listening on port \
             22\", log in from another terminal:\r\n\r\n  {lines}\r\n\r\n"
        ),
    }
    let status = child.wait().with_context(|| format!("waiting for {binary}"))?;
    ensure!(status.success(), "{binary} exited with {status}");
    Ok(())
}

/// What answered a connection through the forward: the guest, either way.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// sshd's first line: it listens.
    Banner(String),
    /// The guest closed the connection: its network is up and nothing listens on the port yet.
    Closed,
}

/// Connect to the host's `port` until the guest answers, or `qemu` ends. QEMU accepts a host
/// connection as soon as it runs, guest or not, and relays it: a guest whose network is not up
/// leaves it hanging (dropped and tried again), a guest with nothing on the port closes it, and
/// sshd sends its banner.
pub fn prove_forward(port: u16, qemu: &mut Child) -> Result<Answer> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    loop {
        if let Some(status) = qemu.try_wait()? {
            bail!("QEMU exited with {status} before the guest answered on port {port}");
        }
        // Refused while QEMU has not bound the port yet.
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        let mut buf = [0u8; 256];
        match stream.read(&mut buf) {
            Ok(0) => return Ok(Answer::Closed),
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                return Ok(Answer::Banner(text.lines().next().unwrap_or_default().to_string()));
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(_) => return Ok(Answer::Closed),
        }
    }
}

/// The QEMU command that boots the launch machine's `packed` image at `smp` harts, its disk
/// packed afresh at `disk` (or, kept, as it is there) and the guest's port 22 forwarded from the
/// host's `port`: what `./launch --system` boots, and the `launch-system` case.
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
    for dir in [
        "target/image/stage/home/alice",
        "target/image/stage/home/bob",
        "target/image/stage/home/agent",
        "target/image/vault",
    ] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    let (devices, _) =
        qemu::virtio_devices(boot, disk, packed.userland.as_ref(), qemu::HostPorts::Given(&[port]))?;
    let image = Image {
        machine,
        firmware,
        loader: &packed.loader,
        bundle: &packed.bundle,
        smp,
        memory_mib: boot.memory_mib.unwrap_or(target::DEFAULT_MEMORY_MIB),
        devices: &devices,
        dtb: None,
    };
    Ok(image.interactive(false))
}

/// `manifest` with the login keys of `principal` replaced by `key` alone.
pub fn give_key(manifest: &[u8], principal: &str, key: &str) -> Result<Vec<u8>> {
    let mut m: Value = serde_json::from_slice(manifest).context("a manifest is JSON")?;
    let given = m["principals"]
        .as_array_mut()
        .and_then(|p| p.iter_mut().find(|p| p["name"] == principal))
        .with_context(|| format!("the manifest has no principal {principal}"))?;
    given["ssh_keys"] = Value::Array(vec![key.into()]);
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

/// The public key line a principal gets, and the private half `ssh -i` names, if known: the
/// user's own (`key`, a `.pub` file; its private half beside it, if there), or the checkout's
/// development key named `development` under the launch directory, made the first time.
fn login_key(workspace: &Path, key: Option<&Path>, development: &str) -> Result<(String, Option<PathBuf>)> {
    let (public, identity) = match key {
        Some(public) => {
            let private = public.with_extension("");
            let identity =
                (public.extension().is_some_and(|e| e == "pub") && private.exists()).then_some(private);
            (public.to_path_buf(), identity)
        }
        None => {
            let private = launch_dir(workspace)?.join(development);
            if !private.exists() {
                keygen(&private)?;
            }
            (private.with_extension("pub"), Some(private))
        }
    };
    Ok((public_line(&public)?, identity))
}

/// Set by `./launch` when it holds the kept disk's lock itself, from before the build to the end of
/// the QEMU it then boots, which outlives this process: the lock found held is then the caller's.
pub const LOCK_HELD: &str = "REDOUBT_LAUNCH_DISK_LOCKED";

/// The lock of the kept disk `disk`, `disk-<arch>.lock` beside it, taken before anything writes the
/// kept disk or the userland image beside it: a second launch on the disk while one runs is refused
/// and leaves the running one's files as they are. `None` when it is held already and the caller
/// says it holds it ([`LOCK_HELD`]); the variable alone, with the lock free, takes nothing on trust.
fn lock_kept(disk: &Path) -> Result<Option<std::fs::File>> {
    let path = disk.with_extension("lock");
    let lock = std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(std::fs::TryLockError::WouldBlock) if std::env::var_os(LOCK_HELD).is_some() => Ok(None),
        Err(std::fs::TryLockError::WouldBlock) => {
            bail!("the kept disk {} is in use by another launch: quit it first (Ctrl-A X)", disk.display())
        }
        Err(std::fs::TryLockError::Error(e)) => Err(e).with_context(|| format!("locking {}", path.display())),
    }
}

/// Where launch keeps what outlives a boot, for this checkout: `$REDOUBT_TMP/launch`, by default
/// under the checkout's `.tmp`, which git ignores.
fn launch_dir(workspace: &Path) -> Result<PathBuf> {
    let tmp = std::env::var_os("REDOUBT_TMP").map_or_else(|| workspace.join(".tmp"), PathBuf::from);
    let dir = tmp.join("launch");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
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

/// The command [`nul_separated`] wrote: its program and arguments, ended by the parent's death as
/// the bench's own QEMU is.
fn from_nul_separated(bytes: &[u8]) -> Result<Command> {
    let bytes = bytes.strip_suffix(b"\0").context("a QEMU argv file ends each argument with a NUL")?;
    let mut parts = bytes.split(|b| *b == 0).map(OsStr::from_bytes);
    let mut cmd = Command::new(parts.next().context("an empty QEMU argv file")?);
    cmd.args(parts);
    qemu::exit_with_parent(&mut cmd);
    Ok(cmd)
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

    /// A kept disk is one launch's at a time: while another holds its lock, by this function or by
    /// `./launch`'s `flock` on the same file, a launch is refused and writes nothing; once it is
    /// let go, the next takes it.
    #[test]
    fn a_kept_disk_in_use_is_refused() {
        let dir = std::env::temp_dir().join(format!("testbench-launch-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let disk = dir.join("disk-rv64.img");
        std::fs::write(&disk, b"a person's files").unwrap();
        let first = lock_kept(&disk).unwrap().expect("not held by a caller");
        let why = lock_kept(&disk).unwrap_err().to_string();
        assert!(why.contains("in use by another launch"), "{why}");
        drop(first);
        drop(lock_kept(&disk).unwrap());

        // ./launch's lock, held by flock(1) for as long as its command runs.
        let lock = disk.with_extension("lock");
        // -o: flock itself holds the lock, not the command, so killing flock lets it go.
        let mut held = std::process::Command::new("flock")
            .arg("-o")
            .arg(&lock)
            .args(["-c", "echo held; exec sleep 60"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("flock(1), from util-linux");
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(held.stdout.take().unwrap()), &mut line)
            .unwrap();
        assert_eq!(line, "held\n");
        assert!(lock_kept(&disk).is_err(), "./launch holds it");
        held.kill().unwrap();
        held.wait().unwrap();
        lock_kept(&disk).unwrap();
        assert_eq!(std::fs::read(&disk).unwrap(), b"a person's files");
        std::fs::remove_dir_all(dir).unwrap();
    }

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

    /// The named principal's keys are replaced by the one key; the rest are as they were: alice's
    /// by launch's first key, then agent's by its second, and bob keeps his.
    #[test]
    fn the_named_principal_alone_gets_the_key() {
        let base = br#"{"servers": [{"name": "keyd"}], "principals": [
            {"name": "alice", "account": "1001", "ssh_keys": ["ssh-ed25519 OLD", "ssh-ed25519 OLD2"]},
            {"name": "bob", "ssh_keys": ["ssh-ed25519 BOB"]},
            {"name": "agent", "ssh_keys": ["ssh-ed25519 TEST"]}]}"#;
        let given = give_key(base, PRINCIPAL, "ssh-ed25519 NEW").unwrap();
        let given: Value =
            serde_json::from_slice(&give_key(&given, AGENT, "ssh-ed25519 AGENT").unwrap()).unwrap();
        let expected = serde_json::json!({"servers": [{"name": "keyd"}], "principals": [
            {"name": "alice", "account": "1001", "ssh_keys": ["ssh-ed25519 NEW"]},
            {"name": "bob", "ssh_keys": ["ssh-ed25519 BOB"]},
            {"name": "agent", "ssh_keys": ["ssh-ed25519 AGENT"]},
        ]});
        assert_eq!(given, expected);
        assert!(give_key(br#"{"principals": [{"name": "bob"}]}"#, PRINCIPAL, "ssh-ed25519 NEW").is_err());
        assert!(give_key(br#"{"principals": [{"name": "alice"}]}"#, AGENT, "ssh-ed25519 NEW").is_err());
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
        let bytes = nul_separated(&cmd);
        assert_eq!(bytes, b"qemu\0-m\01024M\0a b\0");
        let read = from_nul_separated(&bytes).unwrap();
        assert_eq!(read.get_program(), "qemu");
        assert_eq!(read.get_args().collect::<Vec<_>>(), ["-m", "1024M", "a b"]);
        assert!(from_nul_separated(b"").is_err());
        assert!(from_nul_separated(b"qemu").is_err(), "an argument cut short");
    }

    /// The ssh lines name the host port the forward is made from: the one `./launch --ssh-port`
    /// gives both; alice's line first, then agent's, each with its own key.
    #[test]
    fn the_ssh_line_names_the_forwarded_port() {
        let boot: Boot = toml::from_str("programs = []\nexpect = []\n[net]\nforward = [22]\n").unwrap();
        let disk = std::env::temp_dir().join(format!("testbench-launch-forward-{}", std::process::id()));
        let (args, forwards) =
            qemu::virtio_devices(&boot, &disk, None, qemu::HostPorts::Given(&[2200])).unwrap();
        assert_eq!(forwards, [(22, 2200)]);
        assert!(args.iter().any(|a| a.contains("hostfwd=tcp:127.0.0.1:2200-:22")), "{args:?}");
        assert_eq!(
            ssh_line(2200, PRINCIPAL, Some(Path::new("a key"))),
            "ssh -p 2200 -i 'a key' alice@localhost"
        );
        assert_eq!(ssh_line(2222, PRINCIPAL, None), "ssh -p 2222 alice@localhost");
        assert_eq!(
            ssh_line(2222, AGENT, Some(Path::new("agent key"))),
            "ssh -p 2222 -i 'agent key' agent@localhost"
        );
        assert_eq!(
            ssh_lines(2200, Some(Path::new("a")), Some(Path::new("b"))),
            ["ssh -p 2200 -i a alice@localhost", "ssh -p 2200 -i b agent@localhost"]
        );
    }

    /// The forward is proved by the guest's answer: sshd's banner, or the guest closing the
    /// connection; a port nothing answers on is tried until QEMU ends, and that is the failure.
    #[test]
    fn the_forward_is_proved_by_the_guest_s_answer() {
        let mut qemu = Command::new("sleep").arg("30").spawn().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let guest = std::thread::spawn(move || {
            let (mut sshd, _) = listener.accept().unwrap();
            std::io::Write::write_all(&mut sshd, b"SSH-2.0-Sunset-0.6.0\r\nmore").unwrap();
            drop(listener.accept().unwrap());
        });
        assert_eq!(prove_forward(port, &mut qemu).unwrap(), Answer::Banner("SSH-2.0-Sunset-0.6.0".into()));
        assert_eq!(prove_forward(port, &mut qemu).unwrap(), Answer::Closed);
        guest.join().unwrap();
        qemu.kill().unwrap();
        qemu.wait().unwrap();

        // Nothing on the port, and QEMU gone: the proof fails with QEMU's exit.
        let mut qemu = Command::new("false").spawn().unwrap();
        let why = prove_forward(port, &mut qemu).unwrap_err().to_string();
        assert!(why.contains("QEMU exited"), "{why}");
    }
}
