//! SSH sessions: the test keys, the host servers for loopback cases (Redoubt's `sshd` on its host
//! platform, and OpenSSH's for the reference case), and the runner that drives a case's scripted
//! sessions concurrently.
//!
//! The client is the system's OpenSSH `ssh`, not a Rust crate: the box's `sshd` is built on
//! `sunset`, and checking it with an independent implementation catches interoperability
//! bugs that the same library on both ends would share. It also keeps crypto out of the bench.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;

use crate::case::{Session, Step};
use crate::qemu::{Forward, Reaped};

pub const SSH: &str = "ssh";
/// The recipe of OpenSSH's server image for the reference case, relative to the workspace.
const CONTAINERFILE: &str = "tests/ssh-reference/Containerfile";
/// The reference server's only login: root in its container, which is the bench's user outside.
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
    /// OpenSSH's sshd, in a container that ssh itself starts for each session through
    /// `ProxyCommand`, in inetd mode (`sshd -i`) and with no network: it never listens on a port,
    /// so nobody else on the machine can reach it.
    Loopback { proxy: String, host_key: String },
    /// Redoubt's `sshd` on its host platform, `redoubt-sshd-host`, run by ssh the same way.
    Redoubt { proxy: String, host_key: String },
}

/// Makes `case`'s own directory and removes its server log: both servers append, and a line from
/// an earlier run must not satisfy this one's `server_log`.
fn fresh_log(dir: &Path, case: &str) -> Result<PathBuf> {
    let log = loopback_log(dir, case);
    std::fs::create_dir_all(log.parent().unwrap())?;
    match std::fs::remove_file(&log) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => bail!("removing {}: {e}", log.display()),
        _ => Ok(log),
    }
}

/// ssh hands `ProxyCommand` to a shell; keep what the bench does not choose itself (paths, the
/// group's name) free of anything the shell would interpret.
fn plain(part: &str) -> Result<&str> {
    ensure!(
        part.bytes().all(|b| b.is_ascii_alphanumeric() || b" /._+=-".contains(&b)),
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

/// Set up Redoubt's `sshd` on its host platform: it builds `redoubt-sshd-host`, whose host key is
/// `loopback-host` and whose login table gives each `authorized` test key a principal of its name.
pub fn redoubt(
    workspace: &Path,
    dir: &Path,
    case: &str,
    authorized: &[String],
    host_key: Option<&str>,
) -> Result<Server<'static>> {
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
    let keys = workspace.join(KEYS);
    let mut proxy = format!(
        "{} --host-key {} --log {}",
        workspace.join("target/debug/redoubt-sshd-host").display(),
        keys.join("loopback-host").display(),
        fresh_log(dir, case)?.display()
    );
    for name in authorized {
        check_key_name(name)?;
        proxy += &format!(" --login {name}={}", keys.join(format!("{name}.pub")).display());
    }
    Ok(Server::Redoubt { proxy: plain(&proxy)?.into(), host_key: expected_host_key(workspace, host_key)? })
}

/// OpenSSH's server image for the reference case, and how the bench runs `podman` for it.
struct Reference {
    /// The user's primary group from the password database: a service may start the bench under
    /// another, and rootless `podman`'s `newuidmap` then refuses to map the container's users.
    group: String,
    /// `localhost/redoubt-ssh-reference:` and the first 16 hex digits of the Containerfile's
    /// sha256, so that a changed recipe is a new image.
    tag: String,
}

impl Reference {
    fn new(workspace: &Path) -> Result<Reference> {
        let run = |command: &mut Command| -> Result<String> {
            let output = command.output().with_context(|| format!("running {command:?}"))?;
            ensure!(output.status.success(), "{command:?} failed");
            Ok(String::from_utf8(output.stdout)?.trim().to_string())
        };
        let user = run(Command::new("id").arg("-un"))?;
        let group = run(Command::new("id").arg("-gn").arg(&user))?;
        let hash = run(Command::new("sha256sum").arg(workspace.join(CONTAINERFILE)))?;
        let hash = hash.get(..16).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        let hash = hash.context("sha256sum printed no hash")?;
        Ok(Reference { group: plain(&group)?.into(), tag: format!("localhost/redoubt-ssh-reference:{hash}") })
    }

    /// A `podman` command line. A service has no systemd user session for `podman` to put a
    /// container's cgroup in, so it uses cgroupfs.
    fn podman(args: &str) -> String { format!("podman --cgroup-manager=cgroupfs {args}") }

    /// Runs a `podman` command line under the user's own group.
    fn sg(&self, args: &str) -> std::io::Result<std::process::Output> {
        Command::new("sg").args([&self.group, "-c", &Self::podman(args)]).output()
    }

    /// Builds the image unless one has its tag: the one step that needs the network. What the host
    /// lacks for it (`sg`, `podman`, the network to build) is named as such. A build that fails while
    /// the recipe's sources answer is the recipe's fault (a pin Debian has superseded, say): the case
    /// fails.
    fn ensure_image(&self, workspace: &Path) -> Result<(), Unusable> {
        let lacks = |why: String| Unusable::Host(format!("the reference sshd's container: {why}"));
        let version = self.sg("--version").map_err(|e| lacks(format!("sg: {e}")))?;
        match version.status.code() {
            Some(0) => {}
            Some(127) => return Err(lacks("podman is not installed".into())),
            _ => {
                return Err(Unusable::Broken(format!(
                    "sg {} could not run podman: {}",
                    self.group,
                    String::from_utf8_lossy(&version.stderr).trim()
                )));
            }
        }
        let exists = self.sg(&format!("image exists {}", self.tag)).map_err(|e| lacks(format!("sg: {e}")))?;
        if exists.status.success() {
            return Ok(());
        }
        let containerfile = workspace.join(CONTAINERFILE);
        let broken = |e: anyhow::Error| Unusable::Broken(format!("{e:#}"));
        let file = containerfile.to_str().context("a non-UTF-8 path").and_then(plain).map_err(broken)?;
        let context = file.strip_suffix("/Containerfile").unwrap();
        let build = self
            .sg(&format!("build -q -t {} -f {file} {context}", self.tag))
            .map_err(|e| lacks(format!("sg: {e}")))?;
        if !build.status.success() {
            let stderr = String::from_utf8_lossy(&build.stderr);
            let last = stderr.lines().rfind(|l| !l.contains("level=warn")).unwrap_or("");
            let failed = format!("building {} failed: {last}", self.tag);
            return Err(match BUILD_SOURCES.iter().find(|source| !reachable(source)) {
                Some(source) => lacks(format!("{failed}; no network: {source} does not answer")),
                None => Unusable::Broken(failed),
            });
        }
        Ok(())
    }

    /// ssh's `ProxyCommand` for one session: a container of its own, with no network, that mounts
    /// only the case's files: its configuration and keys read-only, and its log writable. Each has a
    /// shared label (`:z`) so that every session's server can read them and append to the one log.
    fn proxy(&self, case_dir: &Path) -> Result<String> {
        let case_dir = plain(case_dir.to_str().context("a non-UTF-8 path")?)?;
        let mounts: String = CASE_FILES
            .iter()
            .map(|(file, mode)| format!("-v {case_dir}/{file}:/case/{file}:{mode} "))
            .collect();
        let run = Self::podman(&format!(
            "run -i --rm --network=none --pull=never {mounts}{} \
             /usr/sbin/sshd -i -f /case/sshd_config -E /case/sshd.log",
            self.tag
        ));
        // sshd logs to its log file, but its monitor, and podman, can still write a last line to
        // stderr, which is ssh's: it would stand in for the session's own last output.
        Ok(format!("sg {} -c '{run}' 2>/dev/null", self.group))
    }
}

/// Set up OpenSSH's server for a case: it accepts the `authorized` test keys, logs in only root
/// in its container, runs `/bin/sh` for every login and allows nothing else: no forwarding, no
/// agent, no rc files it controls. Its configuration, keys and log are in the case's directory.
pub fn loopback(
    workspace: &Path,
    dir: &Path,
    case: &str,
    authorized: &[String],
    host_key: Option<&str>,
) -> Result<Server<'static>> {
    let reference = Reference::new(workspace)?;
    let keys = authorized.iter().map(|name| public_key(workspace, name)).collect::<Result<Vec<_>>>()?;
    let log = fresh_log(dir, case)?;
    let case_dir = log.parent().unwrap();
    // The container mounts the log file itself, so it must exist.
    std::fs::File::create(&log)?;
    std::fs::write(case_dir.join("authorized_keys"), keys.join("\n") + "\n")?;
    key_file(workspace, case_dir, "loopback-host")?;
    std::fs::write(
        case_dir.join("sshd_config"),
        format!(
            "HostKey /case/loopback-host\nAuthorizedKeysFile /case/authorized_keys\n\
             AllowUsers {REFERENCE_USER}\nPermitRootLogin prohibit-password\nPasswordAuthentication no\n\
             KbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n\
             ForceCommand /bin/sh\nAllowTcpForwarding no\nAllowAgentForwarding no\n\
             AllowStreamLocalForwarding no\nX11Forwarding no\nPermitTunnel no\nPermitUserRC no\n\
             PermitUserEnvironment no\nLogLevel DEBUG1\n"
        ),
    )?;
    Ok(Server::Loopback {
        proxy: reference.proxy(case_dir)?,
        host_key: expected_host_key(workspace, host_key)?,
    })
}

/// The loopback server's log for `case`, in the case's own directory, written by `sshd -E` or
/// `redoubt-sshd-host --log`.
pub fn loopback_log(dir: &Path, case: &str) -> PathBuf { dir.join(case).join("sshd.log") }

/// Where building the reference image fetches from: the base image's registry and Debian's archive.
const BUILD_SOURCES: [&str; 2] = ["registry-1.docker.io:443", "deb.debian.org:443"];

/// Whether `source` (host and port) resolves and accepts a TCP connection within five seconds.
fn reachable(source: &str) -> bool {
    let timeout = Duration::from_secs(5);
    source
        .to_socket_addrs()
        .is_ok_and(|mut addrs| addrs.any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok()))
}

/// The reference case's files the container mounts, and how: only its log is writable.
const CASE_FILES: [(&str, &str); 4] =
    [("sshd_config", "ro,z"), ("authorized_keys", "ro,z"), ("loopback-host", "ro,z"), ("sshd.log", "z")];

/// Why the loopback server cannot be used.
#[derive(Clone, Debug, PartialEq)]
pub enum Unusable {
    /// Something the host lacks, known by name: the bench reports it as such (`--allow-skip`).
    Host(String),
    /// Anything else: the case fails.
    Broken(String),
}

/// Whether OpenSSH's server can serve the reference case: its image, built if missing, then one
/// loopback session that runs `exit 0`, whose server must log OpenSSH's version, so that no other
/// server can pass for it.
pub fn loopback_usable(workspace: &Path, dir: &Path) -> Result<(), Unusable> {
    let broken =
        |e: anyhow::Error| Unusable::Broken(format!("the reference sshd could not be set up: {e:#}"));
    Reference::new(workspace).map_err(broken)?.ensure_image(workspace)?;
    let probe = || -> Result<std::process::Output> {
        let Server::Loopback { proxy, .. } =
            loopback(workspace, dir, "loopback-probe", &["alice".into()], None)?
        else {
            unreachable!("loopback() makes a loopback server")
        };
        Command::new(SSH)
            .args(["-F", "/dev/null", "-T", "-l", REFERENCE_USER, "-i"])
            .arg(key_file(workspace, dir, "alice")?)
            .args(["-o", "IdentitiesOnly=yes", "-o", "IdentityAgent=none", "-o", "BatchMode=yes"])
            .args(["-o", "LogLevel=ERROR", "-o", "StrictHostKeyChecking=no"])
            .args(["-o", "UserKnownHostsFile=/dev/null", "-o", "GlobalKnownHostsFile=/dev/null"])
            .args(["-o", &format!("ProxyCommand={proxy}"), "--", "loopback", "exit 0"])
            .stdin(Stdio::null())
            .output()
            .context("running ssh")
    };
    let output = probe().map_err(broken)?;
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
    std::fs::create_dir_all(&dir)?;
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
            std::fs::write(&known_hosts, format!("redoubt {key}\n"))?;
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
            // Its exchange is not post-quantum, and OpenSSH's warning would be session output.
            ssh.args(["-o", "WarnWeakCrypto=no-pq-kex"]);
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
    Ok(failure)
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
    stdin: Option<ChildStdin>,
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
    let mut child = ssh.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
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
        stdin: child.stdin.take(),
        ssh: Reaped(child),
        events,
        open_streams: 2,
        status: None,
        forbid,
        log: std::fs::File::create(log)?,
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

    /// The reference server's `ProxyCommand` is the container command the rule gives, and a case
    /// directory the shell would read as more than a path never reaches it.
    #[test]
    fn the_reference_proxy_quotes_only_what_the_bench_chose() {
        let reference = Reference {
            group: "users".into(),
            tag: "localhost/redoubt-ssh-reference:0123456789abcdef".into(),
        };
        assert_eq!(
            reference.proxy(Path::new("/w/c")).unwrap(),
            "sg users -c 'podman --cgroup-manager=cgroupfs run -i --rm --network=none --pull=never \
             -v /w/c/sshd_config:/case/sshd_config:ro,z -v /w/c/authorized_keys:/case/authorized_keys:ro,z \
             -v /w/c/loopback-host:/case/loopback-host:ro,z -v /w/c/sshd.log:/case/sshd.log:z \
             localhost/redoubt-ssh-reference:0123456789abcdef \
             /usr/sbin/sshd -i -f /case/sshd_config -E /case/sshd.log' 2>/dev/null"
        );
        for case_dir in ["/w/it's", "/w/a:b", "/w/$(x)", "/w/a;b", "/w/a\nb"] {
            assert!(reference.proxy(Path::new(case_dir)).is_err(), "{case_dir:?}");
        }
    }
}
