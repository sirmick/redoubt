//! Running one boot under QEMU and judging its console output.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use regex::Regex;

use crate::case::{ALWAYS_FORBIDDEN, Boot, PASSED};
use crate::peer;
use crate::ssh;
use crate::target::Machine;
use crate::userland::Staged;

/// How long the bench keeps reading the console after the last `expect` (and the sessions),
/// so that a panic right after the last expected line still fails the case. Cases that end
/// by powering off are instead read to the end (`Boot::poweroff`).
const GRACE: Duration = Duration::from_millis(50);
/// The same for a `debug_assertions` case: a check firing a moment after the last expected
/// line is the whole point of that build, so the window is wide enough to catch it.
const CHECKED_GRACE: Duration = Duration::from_millis(1000);

pub enum Verdict {
    /// Everything expected appeared. Carries what each `distinct_across_boots` pattern captured.
    Pass(Vec<Option<String>>),
    Fail(String),
}

/// QEMU ends when the bench does, even when the bench is killed and nothing is dropped
/// (Linux's parent-death signal): a run killed at its timeout leaves no guest running.
const EXIT_WITH_PARENT: [&str; 2] = ["-run-with", "exit-with-parent=on"];

/// The oldest QEMU that knows `EXIT_WITH_PARENT`.
const EXIT_WITH_PARENT_SINCE: &str = "QEMU 10.1";

/// Whether `qemu` takes every option the bench passes it that an older QEMU lacks; probed once
/// per binary and run. Without this, a QEMU too old fails every boot case at once, each for
/// the same reason.
pub fn usable(qemu: &'static str) -> Result<(), String> {
    static PROBED: Mutex<Vec<(&str, Result<(), String>)>> = Mutex::new(Vec::new());
    let mut probed = PROBED.lock().unwrap();
    if let Some((_, usable)) = probed.iter().find(|(binary, _)| *binary == qemu) {
        return usable.clone();
    }
    let usable = probe(qemu, &EXIT_WITH_PARENT);
    probed.push((qemu, usable.clone()));
    usable
}

/// Run `qemu <options> -version`: QEMU rejects an option it does not know, or a `-run-with`
/// parameter, before it gets to `-version`.
fn probe(qemu: &str, options: &[&str]) -> Result<(), String> {
    let run = |args: &[&str]| Command::new(qemu).args(args).stdin(Stdio::null()).output();
    let probe = match run(&[options, &["-version"]].concat()) {
        Ok(probe) => probe,
        Err(e) => return Err(format!("`{qemu}` could not be run: {e}")),
    };
    if probe.status.success() {
        return Ok(());
    }
    let first = |bytes: &[u8]| String::from_utf8_lossy(bytes).lines().next().unwrap_or("").trim().to_string();
    let version = run(&["-version"]).map(|v| first(&v.stdout)).unwrap_or_default();
    Err(format!(
        "`{qemu}` does not take `{}`; the bench needs {EXIT_WITH_PARENT_SINCE} or later, this is {:?}: {}",
        options.join(" "),
        version,
        first(&probe.stderr),
    ))
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
    /// The vendored RustSBI firmware image passed to QEMU with `-bios`.
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
            .args(EXIT_WITH_PARENT)
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
    /// `print_qemu` echoes the exact command line first; `print_only` stops after printing.
    pub fn run_interactive(&self, debug: bool, print_qemu: bool, print_only: bool) -> Result<()> {
        let mut qemu = self.qemu();
        qemu.arg("-nographic");
        if debug {
            qemu.args(["-s", "-S"]);
            eprintln!(
                "\nQEMU paused with a gdb stub on :1234. In another shell:\n                   gdb {}\n  (gdb) target remote :1234\n  (gdb) break kmain\n  (gdb) continue\n\n                 (Use gdb-multiarch if your gdb lacks riscv support.)\n",
                self.loader.display()
            );
        }
        if print_qemu || print_only {
            eprintln!("+ {}", shell_line(&qemu));
            if print_only {
                return Ok(());
            }
        }
        let status = qemu.status().with_context(|| format!("starting {}", self.machine.qemu))?;
        anyhow::ensure!(status.success(), "{} exited with {status}", self.machine.qemu);
        Ok(())
    }
}

/// Render a command as a copy-pasteable shell line.
fn shell_line(cmd: &Command) -> String {
    let mut line = shell_quote(&cmd.get_program().to_string_lossy());
    for arg in cmd.get_args() {
        line.push(' ');
        line.push_str(&shell_quote(&arg.to_string_lossy()));
    }
    line
}

pub fn shell_quote(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || "\"'\\$`&|;<>()*?[]{}!#~".contains(c)) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

/// A forwarded port, (guest port, host port): each TCP `forward`, then the UDP poke's.
pub type Forward = (u16, u16);

/// `count` distinct TCP ports on the loopback interface that nothing was listening on a
/// moment ago. The OS picks them, so benches running side by side do not collide. Between
/// here and QEMU binding them another process could take one; QEMU then fails to start and
/// the boot fails. That needs a second program asking for ports at that instant; it is not
/// retried, so it would show as a failure rather than hide.
fn free_ports(count: usize) -> Result<Vec<u16>> {
    // Hold every listener until all are chosen, so the OS cannot hand out one port twice.
    let listeners =
        (0..count).map(|_| TcpListener::bind("127.0.0.1:0")).collect::<std::io::Result<Vec<_>>>()?;
    Ok(listeners.iter().map(|l| l.local_addr().map(|a| a.port())).collect::<std::io::Result<Vec<_>>>()?)
}

/// QEMU presents virtio-mmio devices in the legacy (version 1) register layout unless told
/// otherwise (`force-legacy` defaults to on). Our drivers speak only version 2, virtio 1.x's
/// layout (servers/blkd.md, servers/netd.md), and carry no second layout for a device only
/// QEMU presents, so every case with a virtio device asks for the modern one.
pub const MODERN_VIRTIO: [&str; 2] = ["-global", "virtio-mmio.force-legacy=false"];

/// The virtio-mmio slots the devices sit on, the ones `image/manifest.json` names, whether or not
/// the case has the others (docs/testbench.md, "Disks and network cards"). QEMU's `virt` machine
/// names its eight transports `virtio-mmio-bus.0` to `.7`, bus `i` at `0x10001000 + i * 0x1000`
/// with interrupt `1 + i`; left to itself it fills them from the top, so a card's slot would
/// depend on whether a disk came first.
pub const NET_BUS: &str = "virtio-mmio-bus.6";
pub const DISK_BUS: &str = "virtio-mmio-bus.7";
/// The userland disk's slot: `0x10006000`, interrupt 6.
pub const USERLAND_BUS: &str = "virtio-mmio-bus.5";

/// QEMU arguments for a case's virtio devices, for one boot: creates the disk afresh at
/// `disk`, so no boot sees another's writes, and picks free host ports for the forwards. The
/// userland disk is packed beside it from `userland`, its staged objects and what their index
/// names ([`crate::build::Builder::userland`]), and attached read-only.
pub fn virtio_devices(
    boot: &Boot,
    disk: &Path,
    userland: Option<&Staged>,
) -> Result<(Vec<String>, Vec<Forward>)> {
    let mut args = Vec::new();
    if boot.disk.is_some() || boot.net.is_some() || boot.userland.is_some() {
        args.extend(MODERN_VIRTIO.iter().map(|a| a.to_string()));
    }
    if let Some(spec) = &boot.disk {
        match &spec.recipe {
            // Packed as `./mkimage` packs it, afresh for every boot.
            Some(recipe) => {
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let recipe = crate::disk::Recipe::load(&root.join(recipe))?;
                std::fs::write(disk, crate::disk::pack_disk(&recipe, &root, spec.stage.as_deref())?)?;
            }
            None => {
                std::fs::File::create(disk)?.set_len(spec.size_kib * 1024)?;
                if spec.partitions > 0 {
                    let sectors = spec.size_kib * 1024 / crate::disk::SECTOR;
                    std::fs::write(disk, crate::disk::gpt_disk(sectors, spec.partitions))?;
                }
            }
        }
        // QEMU's option syntax separates with commas; a comma inside a value is doubled.
        let file = disk.display().to_string().replace(',', ",,");
        args.extend(["-drive".into(), format!("if=none,format=raw,id=disk0,file={file}")]);
        args.extend(["-device".into(), format!("virtio-blk-device,drive=disk0,bus={DISK_BUS}")]);
    }
    if let Some(spec) = &boot.userland {
        let staged = userland.context("a userland disk with nothing staged")?;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let recipe = crate::disk::Recipe::load(&root.join(&spec.recipe))?;
        let image = disk.with_extension("userland.img");
        // The case's damage goes on a copy of the objects, after the index was written.
        let damaged = disk.with_extension("userland");
        let stage = if spec.flip.is_some() || spec.remove.is_some() {
            let (flip, remove) = (spec.flip.as_deref(), spec.remove.as_deref());
            crate::userland::damaged(&staged.objects, &staged.names, flip, remove, &damaged)?;
            damaged.as_path()
        } else {
            staged.objects.as_path()
        };
        std::fs::write(&image, crate::disk::pack_disk(&recipe, &root, Some(stage))?)?;
        let file = image.display().to_string().replace(',', ",,");
        // readonly=on: the host refuses every write, so nothing on the box can change the disk.
        args.extend(["-drive".into(), format!("if=none,format=raw,id=disk1,readonly=on,file={file}")]);
        args.extend(["-device".into(), format!("virtio-blk-device,drive=disk1,bus={USERLAND_BUS}")]);
    }
    let mut forwards = Vec::new();
    if let Some(net) = &boot.net {
        // restrict=on: the guest reaches nothing outside QEMU; only forwarded connections
        // reach it. A case that needs an outside peer must add one deliberately. ipv6=off: the
        // guest speaks only IPv4, and slirp would otherwise advertise itself as an IPv6 router.
        let mut netdev = "user,id=net0,restrict=on,ipv6=off".to_string();
        for (guest, host) in net.forward.iter().zip(free_ports(net.forward.len())?) {
            netdev += &format!(",hostfwd=tcp:127.0.0.1:{host}-:{guest}");
            forwards.push((*guest, host));
        }
        // The poke's forward goes with the others, found by its guest port (`Console::poke`).
        if let Some(poke) = &net.poke {
            let host = peer::free_udp_port()?;
            netdev += &format!(",hostfwd=udp:127.0.0.1:{host}-:{}", poke.port);
            forwards.push((poke.port, host));
        }
        // Peers: the wider network, a guestfwd each, and the capture (`peer.rs`).
        let peers = peer::Files::beside(disk);
        netdev += &peer::netdev_options(net, &peers)?;
        args.extend([
            "-netdev".into(),
            netdev,
            "-device".into(),
            format!("virtio-net-device,netdev=net0,bus={NET_BUS}"),
        ]);
        args.extend(peer::capture_args(net, &peers));
    }
    Ok((args, forwards))
}

/// How much of QEMU's stderr the bench keeps: the last lines, each cut short, so a QEMU that
/// writes without end cannot grow the bench's memory.
const STDERR_LINES: usize = 20;
const STDERR_LINE_BYTES: usize = 512;

/// QEMU's stderr, read on its own thread: its last lines, kept to say why a guest died.
struct Stderr {
    tail: Arc<Mutex<VecDeque<String>>>,
    reader: std::thread::JoinHandle<()>,
}

impl Stderr {
    fn read(stderr: std::process::ChildStderr) -> Self {
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let kept = Arc::clone(&tail);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr).split(b'\n').map_while(|l| l.ok()) {
                let mut line =
                    String::from_utf8_lossy(&line[..line.len().min(STDERR_LINE_BYTES)]).into_owned();
                line.truncate(line.trim_end().len());
                let mut kept = kept.lock().unwrap();
                if kept.len() == STDERR_LINES {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
        });
        Stderr { tail, reader }
    }

    /// The last lines QEMU wrote, once it has exited. A process QEMU started (a peer helper) can
    /// hold the pipe open after QEMU is gone, so the reader is given a moment, not waited for.
    fn last_lines(&self) -> Vec<String> {
        let until = Instant::now() + Duration::from_secs(1);
        while !self.reader.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.tail.lock().unwrap().iter().cloned().collect()
    }
}

/// The verdict for a guest that ended: `failure` and QEMU's exit status, and when QEMU's
/// stderr is the likely reason (it exited before the console said anything, or with a failing
/// status), its last stderr lines, which go to the log too.
fn exited(failure: String, status: ExitStatus, console: &mut Console, stderr: &Stderr) -> Result<String> {
    if console.seen && status.success() {
        return Ok(format!("{failure}: QEMU exited with {status}"));
    }
    let lines = stderr.last_lines();
    writeln!(console.log, "QEMU exited with {status}; its stderr:")?;
    for line in &lines {
        writeln!(console.log, "{line}")?;
    }
    let said = if lines.is_empty() { "nothing on stderr".to_string() } else { lines.join(" | ") };
    Ok(format!("{failure}: QEMU exited with {status}: {said}"))
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
    /// The poke not sent yet: the line it waits for, the host port QEMU forwards to the guest's,
    /// and its payload (`peer::poke`).
    poke: Option<(Regex, u16, String)>,
    stdin: ChildStdin,
    /// Whether the guest has printed a line yet.
    seen: bool,
    /// The reporter's `DONE` line, and whether it has been seen (`Boot::reporter`).
    done: Option<Regex>,
    done_seen: bool,
    /// Under `init`: its line announcing the reporter (`Boot::reporter_announced`), the
    /// `[con N] ` prefix it gave, once seen, and whether the reporter's verdict has been seen.
    announced: Option<Regex>,
    reporter: Option<String>,
    passed_seen: bool,
    /// The loader's first line, which only a reset of the machine prints again: a new boot, in
    /// which `init` announces the reporter anew.
    loader: Regex,
}

/// The loader's first line (loader/src/main.rs), bare: nothing but the loader and `init` prints
/// bare, and `init` never prints this.
const LOADER_LINE: &str = r"^loader: Redoubt rv(64|32) loader, boot hart \d+$";

/// A line `log-server` prints for `DONE`. Anchored: every relayed line starts `[pid N]` or
/// `[badge N]`, so no program's text can start this way.
const DONE_LINE: &str = "[server] done:";

impl Console {
    fn next(&mut self, until: Instant) -> Result<Line> {
        let line = match self.lines.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(Line::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Line::Exited),
        };
        writeln!(self.log, "{line}")?;
        self.seen = true;
        if let Some(pattern) = self.forbid.iter().find(|p| p.is_match(&line)) {
            return Ok(Line::Forbidden(format!("forbidden output /{pattern}/: {line}")));
        }
        if let Some(announced) = &self.announced {
            // A reboot: the next boot's `init` announces the reporter again, under a console
            // connection of the new `consoled`. The verdict is still one line in the whole run.
            if self.loader.is_match(&line) {
                self.reporter = None;
            }
            // `init`'s own lines are the only bare ones (`consoled` prefixes every other), so the
            // anchored announcement cannot be a program's; and `init` announces each child once a
            // boot.
            if let Some(id) = announced.captures(&line).and_then(|c| c.get(1)) {
                if self.reporter.is_some() {
                    return Ok(Line::Forbidden(format!("the reporter announced twice: {line}")));
                }
                self.reporter = Some(format!("[con {}] ", id.as_str()));
            }
            if line.contains(PASSED) {
                match &self.reporter {
                    Some(prefix) if !self.passed_seen && line.starts_with(prefix.as_str()) => {
                        self.passed_seen = true
                    }
                    _ => return Ok(Line::Forbidden(format!("a PASSED line not the reporter's: {line}"))),
                }
            }
        }
        if line.starts_with(DONE_LINE) {
            match &self.done {
                Some(done) if !self.done_seen && done.is_match(&line) => self.done_seen = true,
                _ => return Ok(Line::Forbidden(format!("a DONE line not the reporter's: {line}"))),
            }
        }
        for (pattern, slot) in
            self.capture.iter().zip(self.captured.iter_mut()).filter(|(_, slot)| slot.is_none())
        {
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
        if let Some((_, host, payload)) = self.poke.take_if(|(after, _, _)| after.is_match(&line)) {
            peer::poke(host, &payload).context("sending the poke")?;
        }
        Ok(Line::Text(line))
    }
}

/// Boot `image` and judge it by `boot`. `forwards` are the host ports from `virtio_devices`.
pub fn run(
    image: &Image,
    boot: &Boot,
    workspace: &Path,
    forwards: &[Forward],
    log: &Path,
) -> Result<Verdict> {
    let machine = image.machine;
    let compile = |patterns: &mut dyn Iterator<Item = &str>| -> Result<Vec<Regex>> {
        patterns.map(|p| Regex::new(p).with_context(|| format!("bad regular expression {p:?}"))).collect()
    };
    let expect = compile(&mut boot.expect.iter().map(String::as_str))?;
    let always = ALWAYS_FORBIDDEN.iter().copied().filter(|p| !(boot.allow_panic && *p == "PANIC"));
    let forbid = compile(&mut boot.forbid.iter().map(String::as_str).chain(always))?;
    let capture = compile(&mut boot.distinct_across_boots.iter().map(String::as_str))?;
    let inputs =
        boot.input.iter().map(|i| Ok((Regex::new(&i.after)?, i.send.clone()))).collect::<Result<Vec<_>>>()?;

    let poke = match boot.net.as_ref().and_then(|net| net.poke.as_ref()) {
        Some(poke) => {
            let host = forwards.iter().find(|(guest, _)| *guest == poke.port).map(|(_, host)| *host);
            let host = host.context("the poke's port is not forwarded")?;
            Some((Regex::new(&poke.after)?, host, poke.payload.clone()))
        }
        None => None,
    };

    let done = boot
        .reporter_pid()
        .map(|pid| Regex::new(&format!(r"^\[server\] done: reported by pid {pid};")))
        .transpose()?;

    let mut qemu = image.qemu();
    qemu.args(["-display", "none", "-monitor", "none", "-serial", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut guest = Reaped(qemu.spawn().with_context(|| format!("starting {}", machine.qemu))?);
    let stdin = guest.0.stdin.take().unwrap();
    let stdout = guest.0.stdout.take().unwrap();
    let stderr = Stderr::read(guest.0.stderr.take().unwrap());

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
        poke,
        stdin,
        seen: false,
        done,
        done_seen: false,
        announced: boot.reporter_announced().map(|r| Regex::new(&r)).transpose()?,
        reporter: None,
        passed_seen: false,
        loader: Regex::new(LOADER_LINE)?,
    };
    let deadline = Instant::now() + Duration::from_secs_f64(boot.timeout_secs);
    let mut next = 0;
    while next < expect.len() {
        match console.next(deadline)? {
            Line::Text(line) if expect[next].is_match(&line) => next += 1,
            Line::Text(_) => {}
            Line::Forbidden(why) => return Ok(Verdict::Fail(why)),
            Line::Timeout => return Ok(Verdict::Fail(format!("timed out waiting for /{}/", expect[next]))),
            Line::Exited => {
                let failure = format!("guest exited while waiting for /{}/", expect[next]);
                return Ok(Verdict::Fail(exited(failure, guest.0.wait()?, &mut console, &stderr)?));
            }
        }
    }
    if !boot.session.is_empty() {
        if let Some(why) = run_sessions(&mut console, boot, workspace, forwards, log, deadline)? {
            return Ok(Verdict::Fail(why));
        }
    }
    // Everything expected happened; now the rest of the output must be clean too.
    let grace = if boot.debug_assertions { CHECKED_GRACE } else { GRACE };
    let until = if boot.poweroff { deadline } else { Instant::now() + grace };
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
                if status.code() != Some(boot.poweroff_status) {
                    let failure = format!("expected QEMU's exit status {}", boot.poweroff_status);
                    return Ok(Verdict::Fail(exited(failure, status, &mut console, &stderr)?));
                }
                if console.done.is_some() && !console.done_seen {
                    return Ok(Verdict::Fail("powered off without the reporter's DONE".into()));
                }
                break;
            }
            Line::Exited => break,
        }
    }
    if console.announced.is_some() && !console.passed_seen {
        return Ok(Verdict::Fail("ended without the reporter's PASSED".into()));
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
        let sessions =
            scope.spawn(|| ssh::run(workspace, &boot.session, &server, logs, &prefix, deadline, &abort));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn boot(devices: &str) -> Boot {
        toml::from_str(&format!("programs = []\nexpect = []\n{devices}")).expect("a test case's TOML")
    }

    fn args(devices: &str) -> Vec<String> {
        // One name per call: tests run side by side, and a case with peers clears the files
        // beside its disk.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, Ordering::Relaxed);
        let disk =
            std::env::temp_dir().join(format!("testbench-qemu-test-{}-{call}.img", std::process::id()));
        let (args, _) = virtio_devices(&boot(devices), &disk, None).expect("device arguments");
        std::fs::remove_file(&disk).ok();
        std::fs::remove_dir_all(disk.with_extension("peers")).ok();
        args
    }

    /// A bench killed outright (SIGKILL, nothing dropped) takes its QEMU with it: here a shell
    /// stands in for the bench, starts QEMU the way `Image::qemu` does, and is killed.
    #[test]
    fn a_killed_bench_leaves_no_qemu() {
        let qemu = "qemu-system-riscv64";
        // QEMU greets a QMP client only from its main loop, so after it has read its options and
        // asked for the parent-death signal: killing the stand-in earlier would test only a race.
        let qmp = std::env::temp_dir().join(format!("testbench-exit-with-parent-{}.qmp", std::process::id()));
        std::fs::remove_file(&qmp).ok();
        let script = format!(
            "{qemu} {} -machine virt -bios none -display none -monitor none -serial none -S -qmp unix:{},server=on,wait=off </dev/null >/dev/null 2>&1 & echo $!; exec sleep 600",
            EXIT_WITH_PARENT.join(" "),
            qmp.display()
        );
        let mut bench = std::process::Command::new("sh")
            .args(["-c", &script])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("starting sh");
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(bench.stdout.take().expect("sh's stdout")),
            &mut line,
        )
        .expect("QEMU's pid");
        let pid: u32 = line.trim().parse().expect("a pid");
        let proc = std::path::PathBuf::from(format!("/proc/{pid}"));
        let kill = || {
            std::process::Command::new("kill").args(["-9", &pid.to_string()]).status().ok();
        };
        // Generous: a loaded machine starts QEMU slowly, and this bounds only a broken run.
        let deadline = Instant::now() + Duration::from_secs(120);
        let greeted = || {
            let Ok(mut qmp) = std::os::unix::net::UnixStream::connect(&qmp) else { return false };
            qmp.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut greeting = [0u8; 16];
            std::io::Read::read(&mut qmp, &mut greeting).is_ok_and(|n| greeting[..n].starts_with(b"{\"QMP\""))
        };
        while !greeted() {
            if Instant::now() > deadline {
                kill();
                bench.kill().ok();
                panic!("QEMU never greeted on QMP");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::remove_file(&qmp).ok();
        bench.kill().expect("killing sh");
        bench.wait().ok();
        // Gone, or a zombie waiting for init to reap it.
        let deadline = Instant::now() + Duration::from_secs(120);
        let running = || {
            std::fs::read_to_string(proc.join("stat"))
                .is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
        };
        while running() {
            if Instant::now() > deadline {
                kill();
                panic!("QEMU {pid} outlived the process that started it");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The QEMU the bench runs takes every option it passes; a QEMU refusing one is reported
    /// with the version needed and QEMU's own complaint.
    #[test]
    fn a_qemu_lacking_an_option_is_named() {
        for qemu in ["qemu-system-riscv64", "qemu-system-riscv32"] {
            assert_eq!(probe(qemu, &EXIT_WITH_PARENT), Ok(()));
            let why = probe(qemu, &["-run-with", "no-such-parameter=on"]).expect_err("an unknown parameter");
            assert!(why.contains("needs QEMU 10.1 or later, this is \"QEMU emulator version "), "{why}");
            assert!(why.ends_with("Invalid parameter 'no-such-parameter'"), "{why}");
        }
        let why = probe("qemu-system-no-such-width", &EXIT_WITH_PARENT).expect_err("a missing binary");
        assert!(why.starts_with("`qemu-system-no-such-width` could not be run: "), "{why}");
    }

    fn modern(args: &[String]) -> usize { args.windows(2).filter(|w| w == &MODERN_VIRTIO).count() }

    /// Every case with a virtio device gets the version 2 transport, once; a case with none
    /// gets no device arguments at all.
    #[test]
    fn virtio_devices_are_modern() {
        assert_eq!(modern(&args("[net]\n")), 1);
        assert_eq!(modern(&args("[disk]\nsize_kib = 64\n")), 1);
        assert_eq!(modern(&args("[net]\n[disk]\nsize_kib = 64\n")), 1);
        assert!(args("").is_empty());
    }

    /// Each device sits on its fixed slot, alone or with the other: the card on bus 6
    /// (`0x10007000`, interrupt 7), the disk on bus 7 (`0x10008000`, interrupt 8), and the
    /// userland disk on bus 5 (`0x10006000`, interrupt 6), as `image/manifest.json` names them. A
    /// boot proves QEMU puts them there: `init` refuses a device that is not where its manifest
    /// says (`net-tcp` has no disk, `init-boot` all three).
    #[test]
    fn devices_sit_on_fixed_slots() {
        let net = format!("virtio-net-device,netdev=net0,bus={NET_BUS}");
        let disk = format!("virtio-blk-device,drive=disk0,bus={DISK_BUS}");
        let devices = |case: &str| -> Vec<String> {
            args(case).windows(2).filter(|w| w[0] == "-device").map(|w| w[1].clone()).collect()
        };
        assert_eq!(devices("[net]\n"), [net.clone()]);
        assert_eq!(devices("[disk]\nsize_kib = 64\n"), [disk.clone()]);
        assert_eq!(devices("[net]\n[disk]\nsize_kib = 64\n"), [disk, net]);
        // The image's manifest names each device on one line.
        let manifest = include_str!("../../../image/manifest.json");
        for (name, bus) in [("net0", NET_BUS), ("disk0", DISK_BUS), ("disk1", USERLAND_BUS)] {
            let i: u64 = bus.strip_prefix("virtio-mmio-bus.").unwrap().parse().unwrap();
            let line = format!(
                "{{ \"name\": \"{name}\", \"base\": \"{}\", \"irq\": {}, \"dma\": true }}",
                0x1000_1000 + i * 0x1000,
                1 + i
            );
            assert!(manifest.contains(&line), "image/manifest.json lacks {line}");
        }
    }

    /// The userland disk sits on its own slot, bus 5 (`0x10006000`, interrupt 6), beside the
    /// data disk, attached read-only, and packed from the staged objects with a case's damage on
    /// a copy: the staged objects stay as the index names them.
    #[test]
    fn the_userland_disk_sits_on_its_slot_read_only() {
        let dir = std::env::temp_dir().join(format!("testbench-qemu-userland-{}", std::process::id()));
        let (stage, index) = (dir.join("objects"), dir.join("system.index"));
        let objects = vec![(String::from("lists.beam"), b"FOR1 lists".to_vec())];
        let names = crate::userland::write(&objects, &stage, &index).unwrap();
        let staged = Staged { objects: stage.clone(), index, names };
        let disk = dir.join("boot.img");
        let case =
            "[disk]\nsize_kib = 64\n[userland]\nrecipe = \"image/userland.toml\"\nflip = \"lists.beam\"\n";
        let (args, _) = virtio_devices(&boot(case), &disk, Some(&staged)).unwrap();
        let drive = format!(
            "if=none,format=raw,id=disk1,readonly=on,file={}",
            disk.with_extension("userland.img").display()
        );
        assert!(args.windows(2).any(|w| w[0] == "-drive" && w[1] == drive), "{args:?}");
        let devices: Vec<&String> = args.windows(2).filter(|w| w[0] == "-device").map(|w| &w[1]).collect();
        assert_eq!(
            devices,
            [
                &format!("virtio-blk-device,drive=disk0,bus={DISK_BUS}"),
                &format!("virtio-blk-device,drive=disk1,bus={USERLAND_BUS}")
            ]
        );
        assert_eq!(USERLAND_BUS, "virtio-mmio-bus.5");
        let name = crate::userland::name(b"FOR1 lists");
        assert_eq!(std::fs::read(stage.join(&name)).unwrap(), b"FOR1 lists");
        assert_ne!(std::fs::read(disk.with_extension("userland").join(&name)).unwrap(), b"FOR1 lists");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A poke gets a UDP forward from a host port of its own to its guest port, listed with the
    /// TCP forwards; a case without one gets none.
    #[test]
    fn a_poke_gets_a_udp_forward() {
        let case = "[net]\nforward = [8000]\n[net.poke]\nport = 47000\npayload = 'x'\nafter = 'go'\n";
        let disk = std::env::temp_dir().join(format!("testbench-qemu-poke-{}.img", std::process::id()));
        let (args, forwards) = virtio_devices(&boot(case), &disk, None).expect("device arguments");
        let netdev = args.iter().find(|a| a.starts_with("user,")).expect("a user-mode netdev");
        let (guest, host) = forwards[1];
        assert_eq!((forwards.len(), forwards[0].0, guest), (2, 8000, 47000));
        assert!(netdev.contains(&format!(",hostfwd=udp:127.0.0.1:{host}-:47000")), "{netdev}");
        assert!(!self::args("[net]\nforward = [8000]\n").iter().any(|a| a.contains("hostfwd=udp")));
    }

    /// The guest reaches nothing outside QEMU: every network is `restrict=on`, the wider one a
    /// case with peers gets included, since it widens what maps to the host's loopback. And none
    /// offers IPv6: slirp's router advertisements are frames the guest never asked for.
    #[test]
    fn every_network_is_restricted() {
        let peers = "[net]\nforward = [8000]\n[[net.peer]]\naddr = '10.0.9.100:7'\nconnections = 1\n";
        for case in ["[net]\nforward = [22]\n", "[net]\n", peers] {
            let args = args(case);
            let netdev = args.iter().find(|a| a.starts_with("user,")).expect("a user-mode netdev");
            assert!(netdev.split(',').any(|o| o == "restrict=on"), "{netdev}");
            assert!(netdev.split(',').any(|o| o == "ipv6=off"), "{netdev}");
        }
    }

    /// A case with peers gets the /16, one guestfwd to the helper per peer and the capture; one
    /// without keeps slirp's default network and no capture, as the milestone's manifest does.
    #[test]
    fn peers_get_the_wider_network_and_a_capture() {
        let args = args(
            "[net]\n[[net.peer]]\naddr = '10.0.9.100:7'\nconnections = 1\n\
             [[net.peer]]\naddr = '10.0.9.101:7'\nconnections = 0\n",
        );
        let netdev = args.iter().find(|a| a.starts_with("user,")).expect("a user-mode netdev");
        assert!(netdev.contains(&format!(",{},", peer::VNET)), "{netdev}");
        assert_eq!(netdev.matches(",guestfwd=tcp:10.0.9.10").count(), 2, "{netdev}");
        assert!(
            netdev.contains(&format!(
                "-cmd:{}",
                shell_quote(&std::env::current_exe().unwrap().to_string_lossy())
            ))
        );
        assert!(netdev.contains(" peer-helper --id 10.0.9.100:7 --dir "), "{netdev}");
        let capture = args.windows(2).find(|w| w[0] == "-object").expect("a capture");
        assert!(capture[1].starts_with("filter-dump,id=capture0,netdev=net0,file="), "{}", capture[1]);

        let plain = self::args("[net]\nforward = [22]\n");
        assert!(!plain.iter().any(|a| a.contains("guestfwd") || a.contains("net=10.0.0.0/16")), "{plain:?}");
        assert!(!plain.iter().any(|a| a == "-object"), "{plain:?}");
    }
}
