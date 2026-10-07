//! SSH sessions: the test keys, the host servers for loopback cases (Redoubt's `sshd` on its host
//! platform, and OpenSSH's for the reference case), and the runner that drives a case's scripted
//! sessions concurrently.
//!
//! The client is the system's OpenSSH `ssh`, not a Rust crate: the box's `sshd` is built on
//! `sunset`, and checking it with an independent implementation catches interoperability
//! bugs that the same library on both ends would share. It also keeps crypto out of the bench.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;

use crate::case::{Session, Step};
use crate::qemu::{Forward, Reaped};
use crate::ssh_guest;

pub const SSH: &str = "ssh";

/// What `ssh` gets against Redoubt's server, whose exchange is not post-quantum: OpenSSH's
/// warning would otherwise be session output. OpenSSH 10.1 introduced the warning and this
/// option together, so an older `ssh`, which would refuse the option, has no warning to quiet
/// and none to filter from its output: OpenSSH 9.6 prints nothing over such an exchange.
const REDOUBT_OPTIONS: [&str; 2] = ["-o", "WarnWeakCrypto=no-pq-kex"];

/// The options against Redoubt's server that the host's `ssh` takes: all of them, or none on an
/// `ssh` older than they are. Probed once per run; the run's output says why when they are
/// dropped, since a session that then fails on the warning would not.
fn redoubt_options() -> &'static [&'static str] {
    static TAKEN: OnceLock<bool> = OnceLock::new();
    let taken = *TAKEN.get_or_init(|| match takes(&REDOUBT_OPTIONS) {
        Ok(()) => true,
        Err(why) => {
            println!("note  ssh gets no `{}` against Redoubt's server: {why}", REDOUBT_OPTIONS.join(" "));
            false
        }
    });
    if taken { &REDOUBT_OPTIONS } else { &[] }
}

/// Whether `ssh` takes `options`, as the sessions run it: with no configuration file, `-G`
/// parses them and prints the configuration, connecting to nothing. If not, the error is ssh's
/// complaint.
fn takes(options: &[&str]) -> Result<(), String> {
    let probe = Command::new(SSH)
        .args(["-F", "/dev/null", "-G"])
        .args(options)
        .arg("redoubt")
        .stdin(Stdio::null())
        .output();
    let probe = probe.map_err(|e| format!("`{SSH}` could not be run: {e}"))?;
    if probe.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&probe.stderr).lines().next().unwrap_or("").trim().to_string())
}
/// The reference server's only login: root in its guest.
const REFERENCE_USER: &str = "root";
/// What OpenSSH's server logs at DEBUG1 when a session starts, and no other server's log has.
const REFERENCE_VERSION: &str = "sshd-session version OpenSSH_";
/// The test keys, relative to the workspace: `NAME` and `NAME.pub`, made by ssh-keygen.
/// They are public, like the development signing seed (build.rs). NOT FOR PRODUCTION.
const KEYS: &str = "tests/keys";

fn check_key_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "bad test key name {name:?}: use lower-case letters, digits and '-'"
    );
    Ok(())
}

/// The `authorized_keys` line of test key `name`.
pub fn public_key(workspace: &Path, name: &str) -> Result<String> {
    check_key_name(name)?;
    let path = workspace.join(KEYS).join(format!("{name}.pub"));
    Ok(std::fs::read_to_string(&path)
        .with_context(|| format!("no test key {name} ({})", path.display()))?
        .trim()
        .into())
}

/// Copy test key `name`'s private half to `dir/name`, readable only by us: ssh and sshd
/// refuse a key file others can read, and git does not keep file modes.
fn key_file(workspace: &Path, dir: &Path, name: &str) -> Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    check_key_name(name)?;
    let key =
        std::fs::read(workspace.join(KEYS).join(name)).with_context(|| format!("no test key {name}"))?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    // Write a temporary file and rename it, so a concurrent reader never sees half a key.
    let temporary = dir.join(format!(".{name}.{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)?
        .write_all(&key)?;
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

/// Where sessions connect.
pub enum Server<'a> {
    /// A booted guest, through the port QEMU forwards to its port 22. `host_key`: see `Net`.
    Guest { forwards: &'a [Forward], host_key: Option<&'a str> },
    /// OpenSSH's sshd, in a QEMU guest that ssh itself starts for each session through
    /// `ProxyCommand`, in inetd mode (`sshd -i`) and with no network: it never listens on a port,
    /// so nobody else on the machine can reach it. `case_dir` names the case's guests.
    Loopback { proxy: String, host_key: String, case_dir: PathBuf },
    /// Redoubt's `sshd` on its host platform, `redoubt-sshd-host`, run by ssh the same way.
    Redoubt { proxy: String, host_key: String },
}

/// Makes `case`'s own directory and removes its server log: both servers append, and a line from
/// an earlier run must not satisfy this one's `server_log`.
fn fresh_log(dir: &Path, case: &str) -> Result<PathBuf> {
    let log = loopback_log(dir, case);
    std::fs::create_dir_all(log.parent().unwrap()).with_context(|| format!("creating {}", log.display()))?;
    match std::fs::remove_file(&log) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => bail!("removing {}: {e}", log.display()),
        _ => Ok(log),
    }
}

/// ssh hands `ProxyCommand` to a shell; keep each word the bench does not choose itself (a path)
/// free of anything the shell, or QEMU's option syntax, would interpret, spaces included: a
/// checkout whose path has one fails by name rather than splitting into other words.
fn plain(part: &str) -> Result<&str> {
    ensure!(
        part.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._+=-".contains(&b)),
        "unusual characters in {part:?}"
    );
    Ok(part)
}

/// The key `ssh` expects: the case's, or the loopback servers' own, `loopback-host`.
fn expected_host_key(workspace: &Path, host_key: Option<&str>) -> Result<String> {
    match host_key {
        Some(key) => Ok(key.to_string()),
        None => public_key(workspace, "loopback-host"),
    }
}

/// Build Redoubt's `sshd` on its host platform, `redoubt-sshd-host`, and return its binary.
pub fn build_redoubt(workspace: &Path) -> Result<PathBuf> {
    let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    let output = cargo
        .current_dir(workspace)
        .args(["build", "--quiet", "-p", "redoubt-sshd-host"])
        .output()
        .context("running cargo")?;
    ensure!(
        output.status.success(),
        "building redoubt-sshd-host failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(workspace.join("target/debug/redoubt-sshd-host"))
}

/// Set up Redoubt's `sshd` on its host platform, `binary` ([`build_redoubt`]), whose host key is
/// `loopback-host` and whose login table gives each `authorized` test key a principal of its name.
pub fn redoubt(
    workspace: &Path,
    binary: &Path,
    dir: &Path,
    case: &str,
    authorized: &[String],
    host_key: Option<&str>,
) -> Result<Server<'static>> {
    let keys = workspace.join(KEYS);
    let log = fresh_log(dir, case)?;
    // The log exists even if no session's ssh gets as far as starting the server, as the
    // reference server's does: a case whose sessions time out first reads an empty log.
    std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
    let log = log.display().to_string();
    let mut words = vec![
        binary.display().to_string(),
        "--host-key".into(),
        keys.join("loopback-host").display().to_string(),
        "--log".into(),
        log.clone(),
    ];
    for name in authorized {
        check_key_name(name)?;
        words.push("--login".into());
        words.push(format!("{name}={}", keys.join(format!("{name}.pub")).display()));
    }
    for word in &words {
        plain(word)?;
    }
    // The server's standard error is ssh's unless sent elsewhere: an error it prints after ssh has
    // hung up (a broken pipe, writing to a client that refused its host key) would stand in for
    // ssh's own last line. It goes to the server's log, after the lines it wrote there itself.
    let proxy = format!("{} 2>>{log}", words.join(" "));
    Ok(Server::Redoubt { proxy, host_key: expected_host_key(workspace, host_key)? })
}

/// ssh's `ProxyCommand` for one session: a guest of its own, with no network and no host
/// filesystem, whose initramfs holds the case's files. Its stdio is a virtio-serial port, which
/// the guest's sshd reads and writes; its sshd's log, another port, which QEMU appends to the
/// case's log, as it does the guest's console to `guest.log`. A failed boot powers off rather
/// than reboots, and the guest ends with ssh.
fn proxy(image: &ssh_guest::Image, case_dir: &Path) -> Result<String> {
    let path =
        |path: &Path| -> Result<String> { Ok(plain(path.to_str().context("a non-UTF-8 path")?)?.into()) };
    let (kernel, case_dir) = (path(&image.kernel())?, path(case_dir)?);
    let port = |id: &str, chardev: &str| {
        format!("-chardev {chardev},id={id} -device virtserialport,chardev={id},name={id}")
    };
    // QEMU's stderr is ssh's: a last line of QEMU's would stand in for the session's own output.
    Ok(format!(
        "{} -M virt -m 256M -smp 1 -no-reboot \
         -display none -monitor none -nic none \
         -kernel {kernel} -initrd {case_dir}/initrd.img -append 'console=ttyS0 quiet panic=-1' \
         -chardev file,id=con,path={case_dir}/guest.log,append=on -serial chardev:con \
         -device virtio-serial-device {} {} 2>/dev/null",
        ssh_guest::QEMU,
        port("io", "stdio,signal=off"),
        port("log", &format!("file,path={case_dir}/sshd.log,append=on")),
    ))
}

/// Set up OpenSSH's server for a case: it accepts the `authorized` test keys, logs in only root
/// in its guest, runs `/bin/sh` for every login and allows nothing else: no forwarding, no agent,
/// no rc files it controls. Its configuration, keys and log are in the case's directory, and the
/// first three in the case's initramfs.
pub fn loopback(
    workspace: &Path,
    dir: &Path,
    case: &str,
    authorized: &[String],
    host_key: Option<&str>,
) -> Result<Server<'static>> {
    let image = ssh_guest::Image::locate(workspace)?;
    let keys = authorized.iter().map(|name| public_key(workspace, name)).collect::<Result<Vec<_>>>()?;
    let log = fresh_log(dir, case)?;
    let case_dir = log.parent().unwrap();
    // A case's sessions append, and an earlier run's lines must not satisfy this one; the log
    // exists even if no session ever starts.
    std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
    std::fs::File::create(case_dir.join("guest.log"))?;
    let authorized_keys = keys.join("\n") + "\n";
    let host_private = std::fs::read(key_file(workspace, case_dir, "loopback-host")?)?;
    let sshd_config = format!(
        "HostKey /case/loopback-host\nAuthorizedKeysFile /case/authorized_keys\n\
         AllowUsers {REFERENCE_USER}\nPermitRootLogin prohibit-password\nPasswordAuthentication no\n\
         KbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n\
         ForceCommand /bin/sh\nAllowTcpForwarding no\nAllowAgentForwarding no\n\
         AllowStreamLocalForwarding no\nX11Forwarding no\nPermitTunnel no\nPermitUserRC no\n\
         PermitUserEnvironment no\nLogLevel DEBUG1\n"
    );
    std::fs::write(case_dir.join("authorized_keys"), &authorized_keys)?;
    std::fs::write(case_dir.join("sshd_config"), &sshd_config)?;
    // sshd refuses a host key that others may read.
    let files: [(&str, u32, &[u8]); 3] = [
        ("sshd_config", 0o644, sshd_config.as_bytes()),
        ("authorized_keys", 0o644, authorized_keys.as_bytes()),
        ("loopback-host", 0o600, &host_private),
    ];
    ssh_guest::case_initrd(&image, &files, &case_dir.join("initrd.img"))?;
    Ok(Server::Loopback {
        proxy: proxy(&image, case_dir)?,
        host_key: expected_host_key(workspace, host_key)?,
        case_dir: case_dir.into(),
    })
}

/// The loopback server's log for `case`, in the case's own directory: appended by QEMU from the
/// guest's `sshd -E`, or written by `redoubt-sshd-host --log`.
pub fn loopback_log(dir: &Path, case: &str) -> PathBuf { dir.join(case).join("sshd.log") }

/// Why the loopback server cannot be used.
#[derive(Clone, Debug, PartialEq)]
pub enum Unusable {
    /// Something the host lacks, known by name: the bench reports it as such (`--allow-skip`).
    Host(String),
    /// Anything else: the case fails.
    Broken(String),
}

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Unusable::Host(why) | Unusable::Broken(why) => f.write_str(why),
        }
    }
}

/// So that the guest's builder can carry an `Unusable` through `anyhow` to say whose fault it is.
impl std::error::Error for Unusable {}

/// How long the probe's `ssh` waits for the server's banner: a guest boots in under two seconds
/// under TCG, and a loaded host may take many times that.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether OpenSSH's server can serve the reference case: a QEMU to boot its guest, the guest's
/// image, built if missing, then one loopback session that runs `exit 0`, whose server must log
/// OpenSSH's version, so that no other server can pass for it.
pub fn loopback_usable(workspace: &Path, dir: &Path) -> Result<(), Unusable> {
    let broken =
        |e: anyhow::Error| Unusable::Broken(format!("the reference sshd could not be set up: {e:#}"));
    crate::qemu::usable(ssh_guest::QEMU).map_err(Unusable::Host)?;
    ssh_guest::Image::locate(workspace).map_err(broken)?.ensure(workspace)?;
    let probe = || -> Result<(std::process::Output, PathBuf)> {
        let Server::Loopback { proxy, case_dir, .. } =
            loopback(workspace, dir, "loopback-probe", &["alice".into()], None)?
        else {
            unreachable!("loopback() makes a loopback server")
        };
        let output = Command::new(SSH)
            .args(["-F", "/dev/null", "-T", "-l", REFERENCE_USER, "-i"])
            .arg(key_file(workspace, dir, "alice")?)
            .args(["-o", "IdentitiesOnly=yes", "-o", "IdentityAgent=none", "-o", "BatchMode=yes"])
            .args(["-o", "LogLevel=ERROR", "-o", "StrictHostKeyChecking=no"])
            .args(["-o", "UserKnownHostsFile=/dev/null", "-o", "GlobalKnownHostsFile=/dev/null"])
            // A guest that never sends sshd's banner must not hang the bench.
            .args(["-o", &format!("ConnectTimeout={}", PROBE_TIMEOUT.as_secs())])
            .args(["-o", &format!("ProxyCommand={proxy}"), "--", "loopback", "exit 0"])
            .stdin(Stdio::null())
            .output()
            .context("running ssh")?;
        Ok((output, case_dir))
    };
    let (output, case_dir) = probe().map_err(broken)?;
    if let Some(why) = leftover_guest(&case_dir, Instant::now() + PROBE_TIMEOUT) {
        return Err(Unusable::Broken(format!("the loopback probe: {why}")));
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr.lines().last().unwrap_or("");
        return Err(Unusable::Broken(format!("the loopback probe failed: last output {last:?}")));
    }
    let server_log = std::fs::read_to_string(loopback_log(dir, "loopback-probe")).unwrap_or_default();
    if !server_log.lines().any(|line| line.contains(REFERENCE_VERSION)) {
        return Err(Unusable::Broken(format!(
            "the loopback probe's server log has no {REFERENCE_VERSION:?}"
        )));
    }
    Ok(())
}

/// Why a session stopped early.
enum Stop {
    /// The case fails for this reason.
    Failed(String),
    /// Another session failed first; this one only gave up.
    Aborted,
    /// The bench itself went wrong (ssh would not start, a log could not be written).
    Broken(String),
}

impl From<std::io::Error> for Stop {
    fn from(e: std::io::Error) -> Self { Stop::Broken(e.to_string()) }
}

/// State shared by the sessions of one case: marks set so far, and whether to give up.
struct Shared<'a> {
    marks: Mutex<HashSet<String>>,
    changed: Condvar,
    abort: &'a AtomicBool,
}

impl Shared<'_> {
    fn aborted(&self) -> bool { self.abort.load(Ordering::Relaxed) }

    fn fail(&self) {
        self.abort.store(true, Ordering::Relaxed);
        self.changed.notify_all();
    }
}

/// Run `sessions` concurrently until each has done its steps and its ssh has exited. Returns
/// why the first failing session failed, or None. Setting `abort` makes them all give up.
pub fn run(
    workspace: &Path,
    sessions: &[Session],
    server: &Server,
    logs: &Path,
    log_prefix: &str,
    deadline: Instant,
    abort: &AtomicBool,
) -> Result<Option<String>> {
    let dir = logs.join("ssh");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    // Host keys: checked when the case says which key to expect, as known_hosts under one
    // alias. The guest's key is not known until `sshd` exists (docs/plan/m1-separation.md), so
    // guest cases may accept any.
    let host_key = match server {
        Server::Guest { host_key, .. } => *host_key,
        Server::Loopback { host_key, .. } | Server::Redoubt { host_key, .. } => Some(host_key.as_str()),
    };
    let mut host_key_options = vec!["GlobalKnownHostsFile=/dev/null".to_string()];
    match host_key {
        Some(key) => {
            let known_hosts = dir.join(format!("{log_prefix}-known_hosts"));
            std::fs::write(&known_hosts, format!("redoubt {key}\n"))
                .with_context(|| format!("writing {}", known_hosts.display()))?;
            host_key_options.push("HostKeyAlias=redoubt".into());
            host_key_options.push("StrictHostKeyChecking=yes".into());
            host_key_options.push(format!("UserKnownHostsFile={}", known_hosts.display()));
        }
        None => host_key_options
            .extend(["StrictHostKeyChecking=no".into(), "UserKnownHostsFile=/dev/null".into()]),
    }

    let shared = Shared { marks: Mutex::new(HashSet::new()), changed: Condvar::new(), abort };
    let mut commands = Vec::new();
    for session in sessions {
        let mut ssh = Command::new(SSH);
        // OpenSSH's loopback server logs in only root; there `user` only picks the key.
        let login = match server {
            Server::Guest { .. } | Server::Redoubt { .. } => session.user.as_str(),
            Server::Loopback { .. } => REFERENCE_USER,
        };
        ssh.args(["-F", "/dev/null", if session.pty { "-tt" } else { "-T" }, "-l", login, "-i"])
            .arg(key_file(workspace, &dir, session.key())?)
            .args([
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "IdentityAgent=none",
                "-o",
                "BatchMode=yes",
                "-o",
                "LogLevel=ERROR",
            ]);
        for option in &host_key_options {
            ssh.args(["-o", option]);
        }
        if let Server::Redoubt { .. } = server {
            ssh.args(redoubt_options());
        }
        ssh.args(&session.ssh_args);
        match server {
            Server::Guest { forwards, .. } => {
                let (_, port) =
                    forwards.iter().find(|(guest, _)| *guest == 22).context("port 22 is not forwarded")?;
                // Give up connecting half a second before the case's deadline, so that ssh
                // says why (refused, no banner, ...) rather than the bench timing out bare.
                let left = deadline.saturating_duration_since(Instant::now()).as_secs_f64() - 0.5;
                let connect_timeout = (left.floor() as u64).max(1);
                ssh.args([
                    "-p",
                    &port.to_string(),
                    "-o",
                    &format!("ConnectTimeout={connect_timeout}"),
                    "--",
                    "127.0.0.1",
                ]);
            }
            Server::Loopback { proxy, .. } | Server::Redoubt { proxy, .. } => {
                ssh.args(["-o", &format!("ProxyCommand={proxy}"), "--", "loopback"]);
            }
        }
        ssh.args(&session.command);
        let log = logs.join(format!("{log_prefix}-{}.ssh.log", session.user));
        commands.push((session, ssh, log));
    }

    let stops: Vec<Result<(), Stop>> = std::thread::scope(|scope| {
        let threads: Vec<_> = commands
            .into_iter()
            .map(|(session, ssh, log)| {
                let shared = &shared;
                scope.spawn(move || {
                    let result = drive(session, ssh, &log, shared, deadline);
                    if result.is_err() {
                        shared.fail();
                    }
                    result
                })
            })
            .collect();
        threads.into_iter().map(|t| t.join().expect("session thread panicked")).collect()
    });
    // The keeper: no guest outlives its case's sessions.
    let leftover = match server {
        Server::Loopback { case_dir, .. } => leftover_guest(case_dir, deadline),
        Server::Guest { .. } | Server::Redoubt { .. } => None,
    };
    let mut failure = None;
    for (session, stop) in sessions.iter().zip(stops) {
        match stop {
            Err(Stop::Broken(why)) => bail!("session {}: {why}", session.user),
            Err(Stop::Failed(why)) => {
                failure.get_or_insert(format!("session {}: {why}", session.user));
            }
            Ok(()) | Err(Stop::Aborted) => {}
        }
    }
    Ok(failure.or(leftover))
}

/// The least time a case's guests have to go once their sessions' ssh have exited, however near
/// the deadline those ended: ssh hangs up on its proxy as it exits.
const GUESTS_GO: Duration = Duration::from_secs(5);

/// The keeper: a guest of the case's still running at `deadline` (the case's, or the probe's),
/// or `GUESTS_GO` after its sessions are done if that is later, is named, and killed so that it
/// does not outlive the bench. Until then a guest may take as long as a loaded host makes it to
/// go. A session's own failure, a timeout included, is the case's verdict before this one. The
/// case's directory is the run's own, so a guest whose command line names it is this case's.
fn leftover_guest(case_dir: &Path, deadline: Instant) -> Option<String> {
    leftover(ssh_guest::QEMU, &format!("{}/", case_dir.display()), deadline)
}

/// `leftover_guest` for any `program` with an argument naming `named`.
fn leftover(program: &str, named: &str, deadline: Instant) -> Option<String> {
    let start = Instant::now();
    let deadline = deadline.max(start + GUESTS_GO);
    loop {
        let left = processes_naming(program, named);
        if left.is_empty() {
            return None;
        }
        if Instant::now() >= deadline {
            let pids: Vec<String> = left.iter().map(u32::to_string).collect();
            Command::new("kill").arg("-KILL").args(&pids).status().ok();
            return Some(format!(
                "{program} for {named} still ran {}s after its sessions ended (pid {}; killed)",
                start.elapsed().as_secs(),
                pids.join(", ")
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The processes running `program` with an argument containing `named`. One that has exited and
/// not been reaped has no command line, and is not running; nor has one in the middle of its
/// `execve`, briefly, which the keeper never meets: it looks long after its guests started.
fn processes_naming(program: &str, named: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let pid = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let mut args = cmdline.split(|&b| b == 0).map(String::from_utf8_lossy);
            let first = args.next()?;
            let running = Path::new(first.as_ref()).file_name()? == program;
            (running && args.any(|arg| arg.contains(named))).then_some(pid)
        })
        .collect()
}

enum Event {
    Output(Vec<u8>),
    /// One of ssh's two output streams ended.
    Closed,
}

/// One session in progress.
struct Driver<'a> {
    shared: &'a Shared<'a>,
    deadline: Instant,
    ssh: Reaped,
    /// ssh's standard input: a pipe, or for a `pty = true` session its terminal's master.
    stdin: Option<std::fs::File>,
    pty: bool,
    events: mpsc::Receiver<Event>,
    open_streams: usize,
    /// Set once both output streams have ended and ssh has been waited for.
    status: Option<ExitStatus>,
    forbid: Vec<Regex>,
    log: std::fs::File,
    /// Output bytes that do not yet form a whole UTF-8 character.
    undecoded: Vec<u8>,
    /// The line being received, for `forbid`.
    line: String,
    /// The last whole line, to say what ssh last said when it fails.
    last_line: String,
    /// Output not yet consumed by an `expect`.
    unmatched: String,
}

fn drive(
    session: &Session,
    mut ssh: Command,
    log: &Path,
    shared: &Shared,
    deadline: Instant,
) -> Result<(), Stop> {
    let forbid = session
        .forbid
        .iter()
        .map(|p| Regex::new(p))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Stop::Broken(e.to_string()))?;
    // A terminal session reads a terminal, so that ssh reports its size changes; its output stays
    // on pipes. The size is the one a terminal gets when nothing sets it.
    let (master, input) = match session.pty {
        true => {
            let (master, slave) = crate::pty::open(80, 24)?;
            (Some(master), Stdio::from(slave))
        }
        false => (None, Stdio::piped()),
    };
    let mut child = ssh
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Stop::Broken(format!("running {SSH}: {e}")))?;
    let (tx, events) = mpsc::channel();
    let streams: [Box<dyn Read + Send>; 2] =
        [Box::new(child.stdout.take().unwrap()), Box::new(child.stderr.take().unwrap())];
    for mut stream in streams {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(n @ 1..) = stream.read(&mut buffer) {
                if tx.send(Event::Output(buffer[..n].to_vec())).is_err() {
                    return;
                }
            }
            tx.send(Event::Closed).ok();
        });
    }
    drop(tx);
    let mut driver = Driver {
        shared,
        deadline,
        stdin: master.or_else(|| child.stdin.take().map(|pipe| std::os::fd::OwnedFd::from(pipe).into())),
        pty: session.pty,
        ssh: Reaped(child),
        events,
        open_streams: 2,
        status: None,
        forbid,
        log: std::fs::File::create(log)
            .map_err(|e| Stop::Broken(format!("creating {}: {e}", log.display())))?,
        undecoded: Vec::new(),
        line: String::new(),
        last_line: String::new(),
        unmatched: String::new(),
    };
    for (number, step) in session.steps.iter().enumerate() {
        driver.step(step).map_err(|stop| match stop {
            Stop::Failed(why) => Stop::Failed(format!("step {}: {why}", number + 1)),
            other => other,
        })?;
    }
    // Every session ends with ssh's exit, so all of its output passes `forbid`.
    if !matches!(session.steps.last(), Some(Step::Exit(_))) {
        driver.step(&Step::Exit(0)).map_err(|stop| match stop {
            Stop::Failed(why) => Stop::Failed(format!("at the end: {why}")),
            other => other,
        })?;
    }
    let tail = std::mem::take(&mut driver.line);
    driver.check_line(&tail)
}

impl Driver<'_> {
    fn step(&mut self, step: &Step) -> Result<(), Stop> {
        match step {
            Step::Send(text) => {
                let stdin = self.stdin.as_mut().ok_or(Stop::Broken("send after exit".into()))?;
                // ssh may be gone already; its exit is reported at the next expect or exit.
                stdin.write_all(text.as_bytes()).and_then(|()| stdin.flush()).ok();
                Ok(())
            }
            Step::Resize([cols, rows]) => {
                let master = self.stdin.as_ref().filter(|_| self.pty);
                let master = master.ok_or(Stop::Broken("resize without a terminal".into()))?;
                // ssh may be gone already; its exit is reported at the next expect or exit.
                if self.status.is_none() {
                    crate::pty::resize(master, self.ssh.0.id(), *cols, *rows)?;
                }
                Ok(())
            }
            Step::Mark(mark) => {
                self.shared.marks.lock().unwrap().insert(mark.clone());
                self.shared.changed.notify_all();
                Ok(())
            }
            Step::Wait(mark) => {
                let mut marks = self.shared.marks.lock().unwrap();
                while !marks.contains(mark) {
                    if self.shared.aborted() {
                        return Err(Stop::Aborted);
                    }
                    let left = self.deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(Stop::Failed(format!("timed out waiting for mark {mark:?}")));
                    }
                    // Wake up now and then to notice the case being aborted from outside.
                    marks = self
                        .shared
                        .changed
                        .wait_timeout(marks, left.min(Duration::from_millis(50)))
                        .unwrap()
                        .0;
                }
                Ok(())
            }
            Step::Expect(pattern) => {
                // Multi-line mode: the unmatched output may span lines, and `^`/`$` should
                // still mean the start and end of a line, as they do everywhere else here.
                let regex = Regex::new(&format!("(?m){pattern}")).map_err(|e| Stop::Broken(e.to_string()))?;
                loop {
                    if let Some(found) = regex.find(&self.unmatched) {
                        self.unmatched.drain(..found.end());
                        return Ok(());
                    }
                    if let Some(status) = self.status {
                        return Err(Stop::Failed(format!(
                            "ssh exited ({}) while waiting for /{pattern}/{}",
                            describe(status),
                            self.last()
                        )));
                    }
                    self.pump(&format!("/{pattern}/"))?;
                }
            }
            Step::Exit(expected) => {
                self.stdin = None;
                while self.status.is_none() {
                    self.pump("ssh to exit")?;
                }
                let status = self.status.unwrap();
                if status.code() != Some(*expected) {
                    return Err(Stop::Failed(format!(
                        "ssh exited ({}), expected {expected}{}",
                        describe(status),
                        self.last()
                    )));
                }
                Ok(())
            }
        }
    }

    fn last(&self) -> String {
        if self.last_line.is_empty() { String::new() } else { format!("; last output: {}", self.last_line) }
    }

    /// Take in the next piece of output, or ssh's exit, whichever comes first.
    fn pump(&mut self, waiting_for: &str) -> Result<(), Stop> {
        loop {
            if self.shared.aborted() {
                return Err(Stop::Aborted);
            }
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Stop::Failed(format!("timed out waiting for {waiting_for}")));
            }
            // Wake up now and then to notice another session failing.
            match self.events.recv_timeout(left.min(Duration::from_millis(50))) {
                Ok(Event::Output(bytes)) => return self.take(&bytes),
                Ok(Event::Closed) => {
                    self.open_streams -= 1;
                    if self.open_streams == 0 {
                        let status = self.ssh.0.wait()?;
                        self.status = Some(status);
                        // Only in the log: an exit marker in the output could be forged.
                        writeln!(self.log, "[ssh exited ({})]", describe(status))?;
                    }
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Stop::Broken("lost ssh's output".into()));
                }
            }
        }
    }

    fn take(&mut self, bytes: &[u8]) -> Result<(), Stop> {
        self.undecoded.extend_from_slice(bytes);
        // Keep a character split across two reads for the next one; replace invalid bytes.
        let complete = match std::str::from_utf8(&self.undecoded) {
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            _ => self.undecoded.len(),
        };
        // A terminal ends lines with "\r\n"; drop the '\r' so patterns see plain lines.
        let text = String::from_utf8_lossy(&self.undecoded[..complete]).replace('\r', "");
        self.undecoded.drain(..complete);
        self.log.write_all(text.as_bytes())?;
        self.unmatched.push_str(&text);
        for c in text.chars() {
            if c == '\n' {
                let line = std::mem::take(&mut self.line);
                self.check_line(&line)?;
                if !line.trim().is_empty() {
                    self.last_line = line;
                }
            } else {
                self.line.push(c);
            }
        }
        Ok(())
    }

    fn check_line(&self, line: &str) -> Result<(), Stop> {
        match self.forbid.iter().find(|p| p.is_match(line)) {
            Some(pattern) => Err(Stop::Failed(format!("forbidden output /{pattern}/: {line}"))),
            None => Ok(()),
        }
    }
}

fn describe(status: ExitStatus) -> String {
    status.code().map_or("killed by a signal".into(), |code| format!("status {code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `ssh` that refuses an option the bench gives it says why, in ssh's own words; one it
    /// takes passes.
    #[test]
    fn an_ssh_lacking_an_option_is_named() {
        assert_eq!(takes(&["-o", "BatchMode=yes"]), Ok(()));
        let why = takes(&["-o", "NoSuchOption=yes"]).expect_err("an unknown option");
        assert!(why.ends_with("Bad configuration option: nosuchoption"), "{why}");
    }

    /// The reference server's `ProxyCommand` is the guest the rule gives, and a case directory
    /// the shell would read as more than a path never reaches it.
    #[test]
    fn the_reference_proxy_quotes_only_what_the_bench_chose() {
        let workspace = Path::new("/w");
        let image =
            ssh_guest::Image::locate(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").as_path()).unwrap();
        let kernel = image.kernel();
        let kernel = kernel.to_str().unwrap();
        assert_eq!(
            proxy(&image, Path::new("/w/c")).unwrap(),
            format!(
                "qemu-system-riscv64 -M virt -m 256M -smp 1 -no-reboot \
                 -display none -monitor none -nic none \
                 -kernel {kernel} -initrd /w/c/initrd.img -append 'console=ttyS0 quiet panic=-1' \
                 -chardev file,id=con,path=/w/c/guest.log,append=on -serial chardev:con \
                 -device virtio-serial-device \
                 -chardev stdio,signal=off,id=io -device virtserialport,chardev=io,name=io \
                 -chardev file,path=/w/c/sshd.log,append=on,id=log -device virtserialport,chardev=log,name=log \
                 2>/dev/null"
            )
        );
        for case_dir in ["/w/it's", "/w/a:b", "/w/$(x)", "/w/a;b", "/w/a\nb", "/w/a,b", "/w/a b"] {
            assert!(proxy(&image, &workspace.join(case_dir)).is_err(), "{case_dir:?}");
        }
    }

    /// What Redoubt's server prints on its standard error, run as ssh runs its `ProxyCommand`
    /// (`sh -c`), reaches its log and never ssh's standard error, so it cannot be the session's
    /// last output: ssh's refusal of a host key is.
    #[test]
    fn the_redoubt_proxy_keeps_its_errors_off_ssh() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("redoubt-proxy-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("server");
        std::fs::write(&binary, "#!/bin/sh\necho 'Error: Broken pipe (os error 32)' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let Server::Redoubt { proxy, .. } =
            redoubt(Path::new("/w"), &binary, &dir, "case", &["alice".into()], Some("ssh-ed25519 K"))
                .unwrap()
        else {
            unreachable!()
        };
        let output = Command::new("sh").args(["-c", &proxy]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stderr), "");
        let log = std::fs::read_to_string(loopback_log(&dir, "case")).unwrap();
        assert_eq!(log, "Error: Broken pipe (os error 32)\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Waits until `pid` shows in `/proc` as `program` with an argument containing `named`. A
    /// spawn returns while the child's `execve` is still under way (glibc's `posix_spawn` lets the
    /// parent go once the new address space is in, before its arguments are), and until then the
    /// child's command line reads empty; on a loaded host a scan can land in that window.
    fn exec_done(program: &str, named: &str, pid: u32) {
        let started = Instant::now();
        while !processes_naming(program, named).contains(&pid) {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "pid {pid} never ran {program} naming {named}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Redoubt's server's log exists, empty, before any session starts it, as the reference
    /// server's does: a case whose sessions time out before ssh starts the server reads it empty.
    #[test]
    fn the_redoubt_servers_log_exists_before_any_session() {
        let dir = std::env::temp_dir().join(format!("redoubt-log-test-{}", std::process::id()));
        redoubt(Path::new("/w"), Path::new("/w/server"), &dir, "case", &[], Some("ssh-ed25519 K")).unwrap();
        assert_eq!(std::fs::read_to_string(loopback_log(&dir, "case")).unwrap(), "");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The keeper finds a process by its program and an argument naming the case's directory,
    /// and only while it runs.
    #[test]
    fn the_keeper_finds_what_names_the_case() {
        let named = format!("/keeper-test-{}/", std::process::id());
        let mut child = Command::new("sh").args(["-c", "sleep 30", &format!("{named}x")]).spawn().unwrap();
        let pid = child.id();
        exec_done("sh", &named, pid);
        assert_eq!(processes_naming("sh", &named), [pid]);
        assert_eq!(processes_naming("sleep", &named), [] as [u32; 0]);
        assert_eq!(processes_naming("sh", "/keeper-test-other/"), [] as [u32; 0]);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(processes_naming("sh", &named), [] as [u32; 0]);
    }

    /// A guest that goes late but before the case's deadline is not reported; one still running
    /// at the deadline is named and killed, and never before `GUESTS_GO`, however near the
    /// deadline its sessions ended.
    #[test]
    fn the_keeper_waits_to_the_deadline() {
        let named = format!("/keeper-wait-{}/", std::process::id());
        // It goes when its input closes, which a thread does a little after the keeper starts
        // watching: the verdict is the same if it goes first, but then the wait is not tried.
        let mut late = Command::new("sh")
            .args(["-c", "read _", &format!("{named}late")])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        exec_done("sh", &named, late.id());
        assert_eq!(processes_naming("sh", &named), [late.id()]);
        let input = late.stdin.take().unwrap();
        // Reaped as it exits, as ssh reaps its proxy.
        let reaper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(input);
            late.wait().unwrap()
        });
        assert_eq!(leftover("sh", &named, Instant::now() + Duration::from_secs(60)), None);
        reaper.join().unwrap();

        let mut stuck =
            Command::new("sh").args(["-c", "sleep 30", &format!("{named}stuck")]).spawn().unwrap();
        exec_done("sh", &named, stuck.id());
        let ended = Instant::now();
        let why = leftover("sh", &named, ended).expect("still running");
        assert!(ended.elapsed() >= GUESTS_GO, "killed after {:?}", ended.elapsed());
        assert!(why.contains(&format!("(pid {}; killed)", stuck.id())), "{why}");
        assert!(!stuck.wait().unwrap().success());
    }
}
