//! Running one boot under QEMU and judging its console output.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use regex::Regex;

use crate::case::{Boot, ALWAYS_FORBIDDEN};
use crate::ssh;
use crate::target::Machine;

pub enum Verdict {
    /// Everything expected appeared. Carries what each `distinct_across_boots` pattern captured.
    Pass(Vec<Option<String>>),
    Fail(String),
}

/// Kills QEMU however the run ends.
struct Guest(Child);

impl Drop for Guest {
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
    /// Extra QEMU arguments attaching devices (`virtio_devices`).
    pub devices: &'a [String],
}

impl Image<'_> {
    fn qemu(&self) -> Command {
        let mut qemu = Command::new(self.machine.qemu);
        qemu.args(self.machine.qemu_args)
            .args(["-bios", self.firmware])
            .args(["-smp", &self.smp.to_string()])
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

/// QEMU arguments for a case's virtio devices, for one boot: creates the disk afresh at
/// `disk`, so no boot sees another's writes, and picks free host ports for the forwards.
/// Returns the arguments and the forwards.
pub fn virtio_devices(boot: &Boot, workspace: &Path, disk: &Path) -> Result<(Vec<String>, Vec<Forward>)> {
    // QEMU's option syntax separates with commas; a comma inside a value is doubled.
    let quote = |path: &Path| path.display().to_string().replace(',', ",,");
    let mut args = Vec::new();
    if let Some(spec) = &boot.disk {
        let size = spec.size_kib * 1024;
        match &spec.image {
            Some(image) => {
                let image = workspace.join(image);
                std::fs::copy(&image, disk).with_context(|| format!("copying {}", image.display()))?;
            }
            None => drop(std::fs::File::create(disk)?),
        }
        let file = std::fs::OpenOptions::new().write(true).open(disk)?;
        ensure!(file.metadata()?.len() <= size, "the disk image is larger than size_kib");
        file.set_len(size)?;
        args.extend(["-drive".into(), format!("if=none,format=raw,id=disk0,file={}", quote(disk))]);
        args.extend(["-device".into(), "virtio-blk-device,drive=disk0".into()]);
    }
    let mut forwards = Vec::new();
    if let Some(net) = &boot.net {
        // restrict=on: the guest reaches nothing outside QEMU, only forwarded connections reach
        // it. A case that needs an outside peer must add one deliberately.
        let mut netdev = "user,id=net0,restrict=on".to_string();
        for (guest, host) in net.forward.iter().zip(ssh::free_ports(net.forward.len())?) {
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
    results_pattern: Option<Regex>,
    results: Vec<String>,
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
        if let Some(result) = self.results_pattern.as_ref().and_then(|p| p.captures(&line)).and_then(|c| c.get(1)) {
            self.results.push(result.as_str().to_string());
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

/// Boot `image` and judge it by `boot`. `forwards` are the host ports from `virtio_devices`;
/// `expected_results` the lines of `boot.results.expected`.
pub fn run(
    image: &Image,
    boot: &Boot,
    forwards: &[Forward],
    expected_results: &[String],
    log: &Path,
) -> Result<Verdict> {
    let machine = image.machine;
    let compile = |patterns: &mut dyn Iterator<Item = &str>| -> Result<Vec<Regex>> {
        patterns.map(|p| Regex::new(p).with_context(|| format!("bad regular expression {p:?}"))).collect()
    };
    let expect = compile(&mut boot.expect.iter().map(String::as_str))?;
    let defaults = if boot.default_forbid { ALWAYS_FORBIDDEN } else { &[] };
    let forbid = compile(&mut boot.forbid.iter().map(String::as_str).chain(defaults.iter().copied()))?;
    let capture = compile(&mut boot.distinct_across_boots.iter().map(String::as_str))?;
    let results_pattern = compile(&mut boot.results.iter().map(|r| r.pattern.as_str()))?.pop();
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
    let mut guest = Guest(qemu.spawn().with_context(|| format!("starting {}", machine.qemu))?);
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
        results_pattern,
        results: Vec::new(),
        inputs,
        stdin,
    };
    let deadline = Instant::now() + Duration::from_secs(boot.timeout_secs);
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
        if let Some(why) = run_sessions(&mut console, boot, forwards, log, deadline)? {
            return Ok(Verdict::Fail(why));
        }
    }
    if boot.results.is_some() {
        let got = &console.results;
        if let Some((n, (got, expected))) = got.iter().zip(expected_results).enumerate().find(|(_, (g, e))| g != e) {
            return Ok(Verdict::Fail(format!("result {}: guest reported {got:?}, expected {expected:?}", n + 1)));
        }
        if got.len() != expected_results.len() {
            let (got, expected) = (got.len(), expected_results.len());
            return Ok(Verdict::Fail(format!("guest reported {got} results, expected {expected}")));
        }
    }
    Ok(Verdict::Pass(console.captured))
}

/// Run the case's SSH sessions while still watching the console, so that a panic or the guest
/// dying during a session fails the case. Returns why it failed, if it did.
fn run_sessions(
    console: &mut Console,
    boot: &Boot,
    forwards: &[Forward],
    log: &Path,
    deadline: Instant,
) -> Result<Option<String>> {
    let logs = log.parent().context("log has no directory")?;
    let prefix = log.file_stem().context("log has no name")?.to_string_lossy();
    let abort = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let sessions =
            scope.spawn(|| ssh::run(&boot.session, &ssh::Server::Guest(forwards), logs, &prefix, deadline, &abort));
        let mut console_failure = None;
        while !sessions.is_finished() && console_failure.is_none() {
            console_failure = match console.next(Instant::now() + Duration::from_millis(50)) {
                Ok(Line::Text(_) | Line::Timeout) => None,
                Ok(Line::Forbidden(why)) => Some(why),
                Ok(Line::Exited) => Some("guest exited during the SSH sessions".to_string()),
                Err(e) => Some(format!("{e:#}")),
            };
        }
        if console_failure.is_some() {
            abort.store(true, Ordering::Relaxed);
        }
        let session_failure = sessions.join().expect("session runner panicked")?;
        Ok(console_failure.or(session_failure))
    })
}
