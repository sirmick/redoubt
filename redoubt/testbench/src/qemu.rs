//! Running one boot under QEMU and judging its console output.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use regex::Regex;

use crate::case::{Boot, ALWAYS_FORBIDDEN};
use crate::ssh;
use crate::target::Machine;

/// How long the bench keeps reading the console after the last `expect` (and the sessions),
/// so that a panic right after the last expected line still fails the case. Cases that end
/// by powering off are instead read to the end (`Boot::poweroff`).
const GRACE: Duration = Duration::from_millis(50);

pub enum Verdict {
    /// Everything expected appeared. Carries what each `distinct_across_boots` pattern captured.
    Pass(Vec<Option<String>>),
    Fail(String),
}

/// A child process (QEMU, ssh) that is killed however the run ends.
pub struct Reaped(pub Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// What to boot: the same for a test run and for an interactive session.
pub struct Image<'a> {
    pub machine: &'a Machine,
    /// A firmware image for `-bios`, or "default" for the one QEMU ships (OpenSBI).
    pub firmware: &'a str,
    pub loader: &'a Path,
    pub bundle: &'a Path,
    pub smp: u32,
    /// Guest RAM in MiB.
    pub memory_mib: u32,
    /// Extra QEMU arguments attaching devices (`virtio_devices`).
    pub devices: &'a [String],
}

impl Image<'_> {
    fn qemu(&self) -> Command {
        let mut qemu = Command::new(self.machine.qemu);
        qemu.args(self.machine.qemu_args)
            .args(["-bios", self.firmware])
            .args(["-smp", &self.smp.to_string()])
            .args(["-m", &format!("{}M", self.memory_mib)])
            .arg("-kernel")
            .arg(self.loader)
            .arg("-initrd")
            .arg(self.bundle)
            .args(self.devices);
        qemu
    }

    /// Boot with the console on this terminal. Ctrl-A X quits QEMU. If `debug`, start
    /// paused with QEMU's gdb stub on :1234 so a host gdb can attach with our symbols.
    pub fn run_interactive(&self, debug: bool) -> Result<()> {
        let mut qemu = self.qemu();
        qemu.arg("-nographic");
        if debug {
            qemu.args(["-s", "-S"]);
            eprintln!(
                "\nQEMU paused with a gdb stub on :1234. In another shell:\n                   gdb {}\n  (gdb) target remote :1234\n  (gdb) break kmain\n  (gdb) continue\n\n                 (Use gdb-multiarch if your gdb lacks riscv support.)\n",
                self.loader.display()
            );
        }
        let status = qemu.status().with_context(|| format!("starting {}", self.machine.qemu))?;
        anyhow::ensure!(status.success(), "{} exited with {status}", self.machine.qemu);
        Ok(())
    }
}

/// A forwarded TCP port: (guest port, host port).
pub type Forward = (u16, u16);

/// `count` distinct TCP ports on the loopback interface that nothing was listening on a
/// moment ago. The OS picks them, so benches running side by side do not collide. Between
/// here and QEMU binding them another process could take one; QEMU then fails to start and
/// the boot fails. That needs a second program asking for ports at that instant; it is not
/// retried, so it would show as a failure rather than hide.
fn free_ports(count: usize) -> Result<Vec<u16>> {
    // Hold every listener until all are chosen, so the OS cannot hand out one port twice.
    let listeners = (0..count).map(|_| TcpListener::bind("127.0.0.1:0")).collect::<std::io::Result<Vec<_>>>()?;
    Ok(listeners.iter().map(|l| l.local_addr().map(|a| a.port())).collect::<std::io::Result<Vec<_>>>()?)
}

/// QEMU arguments for a case's virtio devices, for one boot: creates the disk afresh at
/// `disk`, so no boot sees another's writes, and picks free host ports for the forwards.
pub fn virtio_devices(boot: &Boot, disk: &Path) -> Result<(Vec<String>, Vec<Forward>)> {
    let mut args = Vec::new();
    if let Some(spec) = &boot.disk {
        std::fs::File::create(disk)?.set_len(spec.size_kib * 1024)?;
        // QEMU's option syntax separates with commas; a comma inside a value is doubled.
        let file = disk.display().to_string().replace(',', ",,");
        args.extend(["-drive".into(), format!("if=none,format=raw,id=disk0,file={file}")]);
        args.extend(["-device".into(), "virtio-blk-device,drive=disk0".into()]);
    }
    let mut forwards = Vec::new();
    if let Some(net) = &boot.net {
        // restrict=on: the guest reaches nothing outside QEMU; only forwarded connections
        // reach it. A case that needs an outside peer must add one deliberately.
        let mut netdev = "user,id=net0,restrict=on".to_string();
        for (guest, host) in net.forward.iter().zip(free_ports(net.forward.len())?) {
            netdev += &format!(",hostfwd=tcp:127.0.0.1:{host}-:{guest}");
            forwards.push((*guest, host));
        }
        args.extend(["-netdev".into(), netdev, "-device".into(), "virtio-net-device,netdev=net0".into()]);
    }
    Ok((args, forwards))
}

/// What the console did next.
enum Line {
    Text(String),
    Forbidden(String),
    Timeout,
    Exited,
}

/// The guest's console, and everything the bench does with each line of it.
struct Console {
    lines: mpsc::Receiver<String>,
    log: std::fs::File,
    forbid: Vec<Regex>,
    capture: Vec<Regex>,
    captured: Vec<Option<String>>,
    inputs: Vec<(Regex, String)>,
    stdin: ChildStdin,
}

impl Console {
    fn next(&mut self, until: Instant) -> Result<Line> {
        let line = match self.lines.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(Line::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Line::Exited),
        };
        writeln!(self.log, "{line}")?;
        if let Some(pattern) = self.forbid.iter().find(|p| p.is_match(&line)) {
            return Ok(Line::Forbidden(format!("forbidden output /{pattern}/: {line}")));
        }
        for (pattern, slot) in self.capture.iter().zip(self.captured.iter_mut()).filter(|(_, slot)| slot.is_none()) {
            *slot = pattern.captures(&line).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
        }
        let mut pending = Vec::new();
        for (after, send) in self.inputs.drain(..) {
            if after.is_match(&line) {
                self.stdin.write_all(send.as_bytes())?;
                self.stdin.flush()?;
            } else {
                pending.push((after, send));
            }
        }
        self.inputs = pending;
        Ok(Line::Text(line))
    }
}

/// Boot `image` and judge it by `boot`. `forwards` are the host ports from `virtio_devices`.
pub fn run(image: &Image, boot: &Boot, workspace: &Path, forwards: &[Forward], log: &Path) -> Result<Verdict> {
    let machine = image.machine;
    let compile = |patterns: &mut dyn Iterator<Item = &str>| -> Result<Vec<Regex>> {
        patterns.map(|p| Regex::new(p).with_context(|| format!("bad regular expression {p:?}"))).collect()
    };
    let expect = compile(&mut boot.expect.iter().map(String::as_str))?;
    let always = ALWAYS_FORBIDDEN.iter().copied().filter(|p| !(boot.allow_panic && *p == "PANIC"));
    let forbid = compile(&mut boot.forbid.iter().map(String::as_str).chain(always))?;
    let capture = compile(&mut boot.distinct_across_boots.iter().map(String::as_str))?;
    let inputs = boot
        .input
        .iter()
        .map(|i| Ok((Regex::new(&i.after)?, i.send.clone())))
        .collect::<Result<Vec<_>>>()?;

    let mut qemu = image.qemu();
    qemu.args(["-display", "none", "-monitor", "none", "-serial", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut guest = Reaped(qemu.spawn().with_context(|| format!("starting {}", machine.qemu))?);
    let stdin = guest.0.stdin.take().unwrap();
    let stdout = guest.0.stdout.take().unwrap();

    // Read the console on a thread so that the deadline applies even to a silent guest.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).split(b'\n').map_while(|l| l.ok()) {
            if tx.send(String::from_utf8_lossy(&line).trim_end().to_string()).is_err() {
                break;
            }
        }
    });

    let mut console = Console {
        lines: rx,
        log: std::fs::File::create(log).with_context(|| format!("creating {}", log.display()))?,
        forbid,
        captured: vec![None; capture.len()],
        capture,
        inputs,
        stdin,
    };
    let deadline = Instant::now() + Duration::from_secs_f64(boot.timeout_secs);
    let mut next = 0;
    while next < expect.len() {
        match console.next(deadline)? {
            Line::Text(line) if expect[next].is_match(&line) => next += 1,
            Line::Text(_) => {}
            Line::Forbidden(why) => return Ok(Verdict::Fail(why)),
            Line::Timeout => return Ok(Verdict::Fail(format!("timed out waiting for /{}/", expect[next]))),
            Line::Exited => return Ok(Verdict::Fail(format!("guest exited while waiting for /{}/", expect[next]))),
        }
    }
    if !boot.session.is_empty() {
        if let Some(why) = run_sessions(&mut console, boot, workspace, forwards, log, deadline)? {
            return Ok(Verdict::Fail(why));
        }
    }
    // Everything expected happened; now the rest of the output must be clean too.
    let until = if boot.poweroff { deadline } else { Instant::now() + GRACE };
    loop {
        match console.next(until)? {
            Line::Text(_) => {}
            Line::Forbidden(why) => return Ok(Verdict::Fail(why)),
            Line::Timeout if boot.poweroff => {
                return Ok(Verdict::Fail("timed out waiting for the guest to power off".into()));
            }
            Line::Timeout => break,
            Line::Exited if boot.poweroff => {
                let status = guest.0.wait()?;
                if !status.success() {
                    return Ok(Verdict::Fail(format!("QEMU exited with {status}")));
                }
                break;
            }
            Line::Exited => break,
        }
    }
    Ok(Verdict::Pass(console.captured))
}

/// Run the case's SSH sessions while still watching the console, so that a panic or the guest
/// dying during a session fails the case. Returns why it failed, if it did.
fn run_sessions(
    watched: &mut Console,
    boot: &Boot,
    workspace: &Path,
    forwards: &[Forward],
    log: &Path,
    deadline: Instant,
) -> Result<Option<String>> {
    let logs = log.parent().context("log has no directory")?;
    let prefix = log.file_stem().context("log has no name")?.to_string_lossy();
    let host_key = boot.net.as_ref().and_then(|net| net.host_key.as_deref());
    let server = ssh::Server::Guest { forwards, host_key };
    let abort = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let sessions = scope.spawn(|| ssh::run(workspace, &boot.session, &server, logs, &prefix, deadline, &abort));
        let mut console: Result<Option<String>> = Ok(None);
        while !sessions.is_finished() && matches!(console, Ok(None)) {
            console = match watched.next(Instant::now() + Duration::from_millis(50)) {
                Ok(Line::Text(_) | Line::Timeout) => Ok(None),
                Ok(Line::Forbidden(why)) => Ok(Some(why)),
                Ok(Line::Exited) => Ok(Some("guest exited during the SSH sessions".to_string())),
                Err(e) => Err(e),
            };
        }
        if !matches!(console, Ok(None)) {
            abort.store(true, Ordering::Relaxed);
        }
        let session_failure = sessions.join().expect("session runner panicked")?;
        Ok(console?.or(session_failure))
    })
}
