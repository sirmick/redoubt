//! SSH sessions: the test keys, a host OpenSSH server for self-checks, and the runner that
//! drives a case's scripted sessions concurrently.
//!
//! The client is the system's OpenSSH `ssh`, not a Rust crate: the box's `sshd` is built on
//! `sunset`, and checking it with an independent implementation catches interoperability
//! bugs that the same library on both ends would share. It also keeps crypto out of the bench.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context, Result};
use regex::Regex;

use crate::case::{Session, Step};
use crate::qemu::{Forward, Reaped};

pub const SSH: &str = "ssh";
/// Absolute, because sshd refuses to start otherwise.
pub const SSHD: &str = "/usr/sbin/sshd";
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
    Ok(std::fs::read_to_string(&path).with_context(|| format!("no test key {name} ({})", path.display()))?.trim().into())
}

/// Copy test key `name`'s private half to `dir/name`, readable only by us: ssh and sshd
/// refuse a key file others can read, and git does not keep file modes.
fn key_file(workspace: &Path, dir: &Path, name: &str) -> Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    check_key_name(name)?;
    let key = std::fs::read(workspace.join(KEYS).join(name)).with_context(|| format!("no test key {name}"))?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    // Write a temporary file and rename it, so a concurrent reader never sees half a key.
    let temporary = dir.join(format!(".{name}.{}", std::process::id()));
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temporary)?.write_all(&key)?;
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

/// Where sessions connect.
pub enum Server<'a> {
    /// A booted guest, through the port QEMU forwards to its port 22. `host_key`: see `Net`.
    Guest { forwards: &'a [Forward], host_key: Option<&'a str> },
    /// A host sshd run by ssh itself for each session, in inetd mode (`sshd -i`) through
    /// `ProxyCommand`: it never listens on a port, so nobody else on the machine can reach it.
    Loopback { proxy: String, host_key: String, user: String },
}

/// Set up a loopback server that accepts the `authorized` test keys, logs in only the user
/// running the bench (an unprivileged sshd can log in no one else), runs `/bin/sh` for every
/// login and allows nothing else: no forwarding, no agent, no rc files it controls.
pub fn loopback(workspace: &Path, dir: &Path, case: &str, authorized: &[String], host_key: Option<&str>) -> Result<Server<'static>> {
    let keys = authorized.iter().map(|name| public_key(workspace, name)).collect::<Result<Vec<_>>>()?;
    let authorized_keys = dir.join(format!("{case}-authorized_keys"));
    std::fs::create_dir_all(dir)?;
    std::fs::write(&authorized_keys, keys.join("\n") + "\n")?;
    let user = Command::new("id").arg("-un").output().context("running id -un")?;
    let user = String::from_utf8(user.stdout)?.trim().to_string();
    let config = dir.join(format!("{case}-sshd_config"));
    std::fs::write(
        &config,
        format!(
            "HostKey {}\nAuthorizedKeysFile {}\nAllowUsers {user}\nPasswordAuthentication no\n\
             KbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n\
             ForceCommand /bin/sh\nAllowTcpForwarding no\nAllowAgentForwarding no\n\
             AllowStreamLocalForwarding no\nX11Forwarding no\nPermitTunnel no\nPermitUserRC no\n\
             PermitUserEnvironment no\nLogLevel VERBOSE\n",
            key_file(workspace, dir, "loopback-host")?.display(),
            authorized_keys.display()
        ),
    )?;
    let log = dir.join(format!("{case}-sshd.log"));
    // ssh hands ProxyCommand to a shell; keep the paths free of anything it would interpret.
    let proxy = format!("{SSHD} -i -f {} -E {}", config.display(), log.display());
    ensure!(proxy.bytes().all(|b| b.is_ascii_alphanumeric() || b" /._+-".contains(&b)), "unusual path in {proxy:?}");
    let host_key = match host_key {
        Some(key) => key.to_string(),
        None => public_key(workspace, "loopback-host")?,
    };
    Ok(Server::Loopback { proxy, host_key, user })
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
    // alias. The guest's key is not known before WP-S3, so guest cases may accept any.
    let host_key = match server {
        Server::Guest { host_key, .. } => *host_key,
        Server::Loopback { host_key, .. } => Some(host_key.as_str()),
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
        None => host_key_options.extend(["StrictHostKeyChecking=no".into(), "UserKnownHostsFile=/dev/null".into()]),
    }

    let shared = Shared { marks: Mutex::new(HashSet::new()), changed: Condvar::new(), abort };
    let mut commands = Vec::new();
    for session in sessions {
        let mut ssh = Command::new(SSH);
        // The loopback sshd can log in only the user running it; there `user` only picks the key.
        let login = match server {
            Server::Guest { .. } => &session.user,
            Server::Loopback { user, .. } => user,
        };
        ssh.args(["-F", "/dev/null", if session.pty { "-tt" } else { "-T" }, "-l", login, "-i"])
            .arg(key_file(workspace, &dir, session.key())?)
            .args(["-o", "IdentitiesOnly=yes", "-o", "IdentityAgent=none", "-o", "BatchMode=yes", "-o", "LogLevel=ERROR"]);
        for option in &host_key_options {
            ssh.args(["-o", option]);
        }
        match server {
            Server::Guest { forwards, .. } => {
                let (_, port) = forwards.iter().find(|(guest, _)| *guest == 22).context("port 22 is not forwarded")?;
                // Give up connecting half a second before the case's deadline, so that ssh
                // says why (refused, no banner, ...) rather than the bench timing out bare.
                let left = deadline.saturating_duration_since(Instant::now()).as_secs_f64() - 0.5;
                let connect_timeout = (left.floor() as u64).max(1);
                ssh.args(["-p", &port.to_string(), "-o", &format!("ConnectTimeout={connect_timeout}"), "--", "127.0.0.1"]);
            }
            Server::Loopback { proxy, .. } => {
                ssh.args(["-o", &format!("ProxyCommand={proxy}"), "--", "loopback"]);
            }
        }
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

fn drive(session: &Session, mut ssh: Command, log: &Path, shared: &Shared, deadline: Instant) -> Result<(), Stop> {
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
                    marks = self.shared.changed.wait_timeout(marks, left.min(Duration::from_millis(50))).unwrap().0;
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
                        return Err(Stop::Failed(format!("ssh exited ({}) while waiting for /{pattern}/{}", describe(status), self.last())));
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
                    return Err(Stop::Failed(format!("ssh exited ({}), expected {expected}{}", describe(status), self.last())));
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
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Stop::Broken("lost ssh's output".into())),
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
