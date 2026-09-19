//! SSH sessions: test keys, a host OpenSSH server for self-checks, and the runner that drives
//! a case's scripted sessions concurrently.
//!
//! The client is the system's OpenSSH `ssh`, not a Rust crate: the box's `sshd` is built on
//! `sunset`, and checking it with an independent implementation catches interoperability
//! bugs that the same library on both ends would share. It also keeps crypto out of the bench.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ed25519_compact::{KeyPair, Seed};
use regex::Regex;

use crate::case::{Session, Step};

pub const SSH: &str = "ssh";
/// Absolute, because sshd refuses to start otherwise.
pub const SSHD: &str = "/usr/sbin/sshd";

/// The test key called `name`. Deterministic, so that a boot manifest in the repository can
/// list a principal's public key: the seed is the name, zero-padded to 32 bytes. Like the
/// development signing seed (build.rs), these keys are public. NOT FOR PRODUCTION.
fn keypair(name: &str) -> Result<KeyPair> {
    ensure!(
        !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "bad test key name {name:?}: use 1 to 32 letters, digits and '-'"
    );
    let mut seed = [0u8; 32];
    seed[..name.len()].copy_from_slice(name.as_bytes());
    Ok(KeyPair::from_seed(Seed::new(seed)))
}

/// Append an SSH wire-format `string`: a big-endian u32 length, then the bytes.
fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn public_blob(keypair: &KeyPair) -> Vec<u8> {
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-ed25519");
    put_string(&mut blob, keypair.pk.as_ref());
    blob
}

/// The `authorized_keys` line for test key `name`.
pub fn public_key(name: &str) -> Result<String> {
    Ok(format!("ssh-ed25519 {} {name}", BASE64.encode(public_blob(&keypair(name)?))))
}

/// The private key in OpenSSH's own unencrypted format (PROTOCOL.key in OpenSSH's sources).
fn private_key_file(name: &str) -> Result<String> {
    let keypair = keypair(name)?;
    let mut private = Vec::new();
    // Two equal "check" words; they only detect a wrong passphrase, and there is none.
    private.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    put_string(&mut private, b"ssh-ed25519");
    put_string(&mut private, keypair.pk.as_ref());
    put_string(&mut private, keypair.sk.as_ref()); // seed || public key, 64 bytes
    put_string(&mut private, name.as_bytes()); // comment
    for pad in 1..=7u8 {
        if private.len() % 8 == 0 {
            break;
        }
        private.push(pad);
    }
    let mut key = b"openssh-key-v1\0".to_vec();
    put_string(&mut key, b"none"); // cipher
    put_string(&mut key, b"none"); // key derivation function
    put_string(&mut key, b""); // its options
    key.extend_from_slice(&1u32.to_be_bytes()); // number of keys
    put_string(&mut key, &public_blob(&keypair));
    put_string(&mut key, &private);

    let encoded = BASE64.encode(key);
    let lines: Vec<&str> = encoded.as_bytes().chunks(70).map(|c| std::str::from_utf8(c).unwrap()).collect();
    Ok(format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n", lines.join("\n")))
}

/// Write test key `name` to `dir/name`, readable only by us (ssh and sshd insist).
pub fn key_file(dir: &Path, name: &str) -> Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
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
        .write_all(private_key_file(name)?.as_bytes())?;
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

/// `count` distinct TCP ports on the loopback interface that nothing was listening on a
/// moment ago. The OS picks them, so concurrent benches do not collide; what remains is the
/// short window before QEMU or sshd binds them, which only another process asking the OS
/// for an ephemeral port at that instant could hit.
pub fn free_ports(count: usize) -> Result<Vec<u16>> {
    // Hold every listener until all are chosen, so the OS cannot hand out one port twice.
    let listeners = (0..count).map(|_| TcpListener::bind("127.0.0.1:0")).collect::<std::io::Result<Vec<_>>>()?;
    Ok(listeners.iter().map(|l| l.local_addr().map(|a| a.port())).collect::<std::io::Result<Vec<_>>>()?)
}

/// Kills a child process however the run ends.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// An OpenSSH server on the host, run as the current user, for `ssh-loopback` cases. It
/// accepts exactly the `authorized` test keys and runs `/bin/sh` for every login.
pub struct Loopback {
    _sshd: Reaped,
    pub port: u16,
}

pub fn start_loopback(dir: &Path, case: &str, authorized: &[String]) -> Result<Loopback> {
    std::fs::create_dir_all(dir)?;
    let keys = authorized.iter().map(|name| public_key(name)).collect::<Result<Vec<_>>>()?;
    let authorized_keys = dir.join(format!("{case}-authorized_keys"));
    std::fs::write(&authorized_keys, keys.join("\n") + "\n")?;
    let host_key = key_file(dir, "loopback-host")?;
    let config = dir.join(format!("{case}-sshd_config"));
    std::fs::write(
        &config,
        format!(
            "ListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nPasswordAuthentication no\n\
             KbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n\
             ForceCommand /bin/sh\nLogLevel VERBOSE\n",
            host_key.display(),
            authorized_keys.display()
        ),
    )?;
    let port = free_ports(1)?[0];
    let log = dir.join(format!("{case}-sshd.log"));
    let sshd = Command::new(SSHD)
        .args(["-D", "-e", "-p", &port.to_string(), "-f"])
        .arg(&config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log)?)
        .spawn()
        .context("starting sshd")?;
    let mut sshd = Reaped(sshd);
    // Ready once it accepts a connection.
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if let Some(status) = sshd.0.try_wait()? {
            bail!("sshd exited with {status}; see {}", log.display());
        }
        ensure!(Instant::now() < deadline, "sshd did not start listening; see {}", log.display());
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(Loopback { _sshd: sshd, port })
}

/// Where sessions connect.
pub enum Server<'a> {
    /// A booted guest, through the ports QEMU forwards to it.
    Guest(&'a [crate::qemu::Forward]),
    /// A loopback sshd. An unprivileged sshd can log in only the user running it, so every
    /// session logs in as that user; its `user` still chooses its key.
    Loopback { port: u16, user: String },
}

/// Why a session stopped because another one failed; not reported as its own failure.
const ABORTED: &str = "aborted";

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

/// Run `sessions` concurrently until each has done all its steps. Returns why the first
/// failing session failed, or None. Setting `abort` makes every session give up promptly.
pub fn run(
    sessions: &[Session],
    server: &Server,
    logs: &Path,
    log_prefix: &str,
    deadline: Instant,
    abort: &AtomicBool,
) -> Result<Option<String>> {
    let keys = logs.join("ssh");
    let shared = Shared { marks: Mutex::new(HashSet::new()), changed: Condvar::new(), abort };
    let mut commands = Vec::new();
    for session in sessions {
        let (login, port) = match server {
            Server::Guest(forwards) => {
                let (_, host) = forwards.iter().find(|(guest, _)| *guest == session.port).context("port not forwarded")?;
                (session.user.clone(), *host)
            }
            Server::Loopback { port, user } => (user.clone(), *port),
        };
        // ssh gives up connecting when the case runs out of time, so that it reports why
        // (refused, no banner, ...) instead of the bench reporting a bare timeout.
        let connect_timeout = deadline.saturating_duration_since(Instant::now()).as_secs().max(1);
        let mut ssh = Command::new(SSH);
        ssh.args(["-F", "/dev/null", if session.pty { "-tt" } else { "-T" }, "-p", &port.to_string(), "-i"])
            .arg(key_file(&keys, session.key())?)
            .args(["-o", "IdentitiesOnly=yes", "-o", "IdentityAgent=none", "-o", "BatchMode=yes"])
            .args(["-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null"])
            .args(["-o", "GlobalKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR"])
            .args(["-o", &format!("ConnectTimeout={connect_timeout}"), &format!("{login}@127.0.0.1")]);
        let log = logs.join(format!("{log_prefix}-{}.ssh.log", session.name()));
        commands.push((session, ssh, log));
    }

    let failures: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = commands
            .into_iter()
            .map(|(session, ssh, log)| {
                let shared = &shared;
                scope.spawn(move || {
                    let result = drive(session, ssh, &log, shared, deadline);
                    if result.is_err() {
                        shared.fail();
                    }
                    result.map_err(|why| format!("session {}: {why}", session.name()))
                })
            })
            .collect();
        threads.into_iter().filter_map(|t| t.join().expect("session thread panicked").err()).collect()
    });
    Ok(failures.into_iter().find(|why| !why.ends_with(ABORTED)))
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
    exited: bool,
    forbid: Vec<Regex>,
    log: std::fs::File,
    /// Output bytes that do not yet form a whole UTF-8 character.
    undecoded: Vec<u8>,
    /// The line being received, for `forbid`.
    line: String,
    /// Output not yet consumed by an `expect`.
    unmatched: String,
}

fn drive(session: &Session, mut ssh: Command, log: &Path, shared: &Shared, deadline: Instant) -> Result<(), String> {
    let forbid = session
        .forbid
        .iter()
        .map(|p| Regex::new(p))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut child = ssh
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting {SSH}: {e}"))?;
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
        exited: false,
        forbid,
        log: std::fs::File::create(log).map_err(|e| format!("creating {}: {e}", log.display()))?,
        undecoded: Vec::new(),
        line: String::new(),
        unmatched: String::new(),
    };
    for (number, step) in session.steps.iter().enumerate() {
        driver.step(step).map_err(|why| format!("step {}: {why}", number + 1))?;
    }
    // Output that already arrived still has to pass `forbid`.
    while let Ok(event) = driver.events.try_recv() {
        driver.handle(event)?;
    }
    let tail = std::mem::take(&mut driver.line);
    driver.check_line(&tail)
}

impl Driver<'_> {
    fn step(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::Send(text) => {
                let stdin = self.stdin.as_mut().ok_or("send after close")?;
                stdin.write_all(text.as_bytes()).and_then(|()| stdin.flush()).map_err(|e| format!("sending: {e}"))
            }
            Step::Close(_) => {
                self.stdin = None;
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
                        return Err(ABORTED.into());
                    }
                    let left = self.deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(format!("timed out waiting for mark {mark:?}"));
                    }
                    // Wake up now and then to notice the case being aborted from outside.
                    marks = self.shared.changed.wait_timeout(marks, left.min(Duration::from_millis(50))).unwrap().0;
                }
                Ok(())
            }
            Step::Expect(pattern) => {
                // Multi-line mode: the unmatched output may span lines, and `^`/`$` should
                // still mean the start and end of a line, as they do everywhere else here.
                let regex = Regex::new(&format!("(?m){pattern}")).map_err(|e| e.to_string())?;
                loop {
                    if let Some(found) = regex.find(&self.unmatched) {
                        self.unmatched.drain(..found.end());
                        return Ok(());
                    }
                    if self.exited {
                        return Err(format!("ssh exited while waiting for /{pattern}/"));
                    }
                    self.wait_for_output(pattern)?;
                }
            }
        }
    }

    /// Take in the next piece of output (or ssh's exit), whichever comes first.
    fn wait_for_output(&mut self, pattern: &str) -> Result<(), String> {
        loop {
            if self.shared.aborted() {
                return Err(ABORTED.into());
            }
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("timed out waiting for /{pattern}/"));
            }
            // Wake up now and then to notice another session failing.
            match self.events.recv_timeout(left.min(Duration::from_millis(50))) {
                Ok(event) => return self.handle(event),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("ssh's output ended".into()),
            }
        }
    }

    fn handle(&mut self, event: Event) -> Result<(), String> {
        match event {
            Event::Output(bytes) => self.take(&bytes),
            Event::Closed => {
                self.open_streams -= 1;
                if self.open_streams > 0 {
                    return Ok(());
                }
                let status = self.ssh.0.wait().map_err(|e| e.to_string())?;
                self.exited = true;
                let code = status.code().map_or("killed".into(), |c| c.to_string());
                self.take(format!("[ssh exited: {code}]\n").as_bytes())
            }
        }
    }

    fn take(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.undecoded.extend_from_slice(bytes);
        // Keep a character split across two reads for the next one; replace invalid bytes.
        let complete = match std::str::from_utf8(&self.undecoded) {
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            _ => self.undecoded.len(),
        };
        // A terminal ends lines with "\r\n"; drop the '\r' so patterns see plain lines.
        let text = String::from_utf8_lossy(&self.undecoded[..complete]).replace('\r', "");
        self.undecoded.drain(..complete);
        self.log.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        self.unmatched.push_str(&text);
        for c in text.chars() {
            if c == '\n' {
                let line = std::mem::take(&mut self.line);
                self.check_line(&line)?;
            } else {
                self.line.push(c);
            }
        }
        Ok(())
    }

    fn check_line(&self, line: &str) -> Result<(), String> {
        match self.forbid.iter().find(|p| p.is_match(line)) {
            Some(pattern) => Err(format!("forbidden output /{pattern}/: {line}")),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OpenSSH itself must read our private key file and derive the same public key.
    #[test]
    fn ssh_keygen_accepts_test_keys() {
        let dir = std::env::temp_dir().join(format!("testbench-keys-{}", std::process::id()));
        for name in ["alice", "a-rather-long-name-of-32-bytes-x"] {
            let path = key_file(&dir, name).unwrap();
            let output = Command::new("ssh-keygen").arg("-y").arg("-f").arg(&path).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let derived = String::from_utf8(output.stdout).unwrap();
            let ours = public_key(name).unwrap();
            assert_eq!(derived.split_whitespace().take(2).collect::<Vec<_>>(), ours.split_whitespace().take(2).collect::<Vec<_>>());
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::remove_dir(&dir).unwrap();
        assert!(keypair("").is_err() && keypair("alice+x").is_err());
        assert_ne!(public_key("alice").unwrap(), public_key("bob").unwrap());
    }
}
