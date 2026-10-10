//! Running one boot under QEMU and judging its console output.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde_json::{Value, json};

use crate::case::{ALWAYS_FORBIDDEN, Boot, PASSED};
use crate::idle;
use crate::memory;
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

/// QEMU ends when the bench does, even when the bench is killed and nothing is dropped: Linux
/// sends it SIGKILL when the thread that started it ends (the parent-death signal, asked for
/// between fork and exec, so before QEMU runs at all). A run killed at its timeout leaves no
/// guest running.
fn exit_with_parent(qemu: &mut Command) -> &mut Command {
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: the hook runs in the child between fork and exec, where only async-signal-safe
    // calls may be made: it makes two system calls, reads errno, and allocates nothing.
    unsafe {
        qemu.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The bench died before the call, so no signal will come: the child is already
            // another process's.
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        })
    }
}

/// Whether `qemu` runs; probed once per binary and run. Without this, a missing or broken QEMU
/// fails every boot case at once, each for the same reason. The bench passes no option that an
/// older QEMU lacks; one it did would be probed here, with the oldest QEMU that takes it.
pub fn usable(qemu: &'static str) -> Result<(), String> {
    static PROBED: Mutex<Vec<(&str, Result<(), String>)>> = Mutex::new(Vec::new());
    let mut probed = PROBED.lock().unwrap();
    if let Some((_, usable)) = probed.iter().find(|(binary, _)| *binary == qemu) {
        return usable.clone();
    }
    let usable = probe(qemu, &[], "");
    probed.push((qemu, usable.clone()));
    usable
}

/// Run `qemu <options> -version`: QEMU rejects an option it does not know, or a parameter of
/// one, before it gets to `-version`. `since` is the oldest QEMU that takes `options`.
fn probe(qemu: &str, options: &[&str], since: &str) -> Result<(), String> {
    let run = |args: &[&str]| Command::new(qemu).args(args).stdin(Stdio::null()).output();
    let probe = match run(&[options, &["-version"]].concat()) {
        Ok(probe) => probe,
        Err(e) => return Err(format!("`{qemu}` could not be run: {e}")),
    };
    if probe.status.success() {
        return Ok(());
    }
    let first = |bytes: &[u8]| String::from_utf8_lossy(bytes).lines().next().unwrap_or("").trim().to_string();
    if options.is_empty() {
        return Err(format!("`{qemu} -version` failed with {}: {}", probe.status, first(&probe.stderr)));
    }
    let version = run(&["-version"]).map(|v| first(&v.stdout)).unwrap_or_default();
    Err(format!(
        "`{qemu}` does not take `{}`; the bench needs {since} or later, this is {:?}: {}",
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

/// A file QEMU made for the run, removed however the run ends.
struct Removed(PathBuf);

impl Drop for Removed {
    fn drop(&mut self) { std::fs::remove_file(&self.0).ok(); }
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
        exit_with_parent(&mut qemu)
            .args(self.machine.qemu_args)
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
        let qemu = self.interactive(debug);
        if print_qemu || print_only {
            eprintln!("+ {}", shell_line(&qemu));
            if print_only {
                return Ok(());
            }
        }
        run_to_end(qemu, self.machine.qemu)
    }

    /// The command that boots with the console on this terminal, and with `debug`, paused with
    /// the gdb stub (whose instructions it prints).
    pub fn interactive(&self, debug: bool) -> Command {
        let mut qemu = self.qemu();
        qemu.arg("-nographic");
        if debug {
            qemu.args(["-s", "-S"]);
            eprintln!(
                "\nQEMU paused with a gdb stub on :1234. In another shell:\n                   gdb {}\n  (gdb) target remote :1234\n  (gdb) break kmain\n  (gdb) continue\n\n                 (Use gdb-multiarch if your gdb lacks riscv support.)\n",
                self.loader.display()
            );
        }
        qemu
    }
}

/// Run `qemu` to its end, with this terminal's stdio; it fails if QEMU does.
pub fn run_to_end(mut qemu: Command, binary: &str) -> Result<()> {
    let status = qemu.status().with_context(|| format!("starting {binary}"))?;
    ensure!(status.success(), "{binary} exited with {status}");
    Ok(())
}

/// Render a command as a copy-pasteable shell line.
pub fn shell_line(cmd: &Command) -> String {
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
pub fn free_ports(count: usize) -> Result<Vec<u16>> {
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

/// QEMU arguments for a case's virtio devices, for one boot: creates the disk afresh at `disk`,
/// so no boot sees another's writes, unless it is kept and there already (`Disk::keep`), when
/// its layout must be a fresh disk's; and forwards from `ports`, the host ports in the forwards'
/// order, or with none given from free ones. The userland disk is copied beside it from
/// `userland`, the run's one pack ([`crate::build::Builder::userland`]), with the case's damage,
/// and attached read-only.
pub fn virtio_devices(
    boot: &Boot,
    disk: &Path,
    userland: Option<&Staged>,
    ports: &[u16],
) -> Result<(Vec<String>, Vec<Forward>)> {
    let mut args = Vec::new();
    if boot.disk.is_some() || boot.net.is_some() || boot.userland.is_some() {
        args.extend(MODERN_VIRTIO.iter().map(|a| a.to_string()));
    }
    if let Some(spec) = &boot.disk {
        match &spec.recipe {
            // A kept disk, from an earlier boot: as that boot left it, if laid out as a fresh one.
            _ if spec.keep && disk.exists() => check_layout(spec, disk)?,
            // Packed as `./mkimage` packs it, afresh for every boot.
            Some(recipe) => {
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let recipe = crate::disk::Recipe::load(&root.join(recipe))?;
                let (mut bytes, verified) = crate::disk::pack(&recipe, &root, spec.stage.as_deref())?;
                if spec.flip_version {
                    let signed: Vec<_> = verified.iter().filter(|v| v.signed.is_some()).collect();
                    ensure!(!signed.is_empty(), "a flipped version needs a signed volume");
                    for v in signed {
                        crate::disk::flip_version(&mut bytes, v)?;
                    }
                }
                std::fs::write(disk, bytes)?;
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
        let image = disk.with_extension("userland.img");
        // The run's one pack, whose root the bundle's manifest pins; the case's damage goes on
        // this boot's copy, after the pack, and never on the root.
        let mut bytes =
            std::fs::read(&staged.image).with_context(|| format!("reading {}", staged.image.display()))?;
        if spec.flip.is_some() || spec.flip_tree {
            let verified = staged.verified.first().context("damage needs a verified volume")?;
            if let Some(file) = &spec.flip {
                let module = std::fs::read(staged.objects.join(file))
                    .with_context(|| format!("the userland disk has no {file}"))?;
                // A file the boot pack holds is on the volume twice: both copies are damaged.
                let pack = staged.objects.join(crate::userland::PACK);
                let packed = pack.exists()
                    && crate::userland::pack_entries(&std::fs::read(&pack)?)?
                        .iter()
                        .any(|(name, _)| name == file);
                crate::disk::flip_file(&mut bytes, verified, &module, 1 + usize::from(packed))?;
            }
            if spec.flip_tree {
                crate::disk::flip_tree(&mut bytes, verified);
            }
        }
        std::fs::write(&image, bytes)?;
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
        let hosts = match ports {
            [] => free_ports(net.forward.len())?,
            given => {
                ensure!(
                    given.len() == net.forward.len(),
                    "{} host ports for {} forwards",
                    given.len(),
                    net.forward.len()
                );
                given.to_vec()
            }
        };
        for (guest, host) in net.forward.iter().zip(hosts) {
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

/// Refuse a kept disk whose layout is not the one `spec` packs now: its size, and its partition
/// table (`blkd`'s builder writes it the same for the same sizes); and, as a pack would, a recipe
/// whose manifest's volumes no longer fit its partitions. Its volumes' contents are the guest's;
/// a table that differs is never reformatted, only refused.
fn check_layout(spec: &crate::case::Disk, disk: &Path) -> Result<()> {
    let (bytes, table) = match &spec.recipe {
        Some(recipe) => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let recipe = crate::disk::Recipe::load(&root.join(recipe))?;
            crate::disk::hold_recipe(&recipe, &root)
                .with_context(|| format!("the kept disk {} is not attached", disk.display()))?;
            let sectors = recipe.size_kib * 1024 / crate::disk::SECTOR;
            (recipe.size_kib * 1024, Some(crate::disk::gpt_disk(sectors, recipe.partition.len() as u64)))
        }
        None => {
            let sectors = spec.size_kib * 1024 / crate::disk::SECTOR;
            (
                spec.size_kib * 1024,
                (spec.partitions > 0).then(|| crate::disk::gpt_disk(sectors, spec.partitions)),
            )
        }
    };
    let refuse = |why: String| {
        anyhow::anyhow!(
            "the kept disk {} is not laid out as the image's disk now ({why}): it is not reformatted; \
             delete it, or launch with --fresh-disk, to start it over from the image's",
            disk.display()
        )
    };
    let len = std::fs::metadata(disk)?.len();
    if len != bytes {
        return Err(refuse(format!("{len} bytes, not {bytes}")));
    }
    if let Some(table) = table {
        let head = crate::disk::TABLE_BYTES.min(table.len());
        let mut kept = vec![0u8; head];
        std::io::Read::read_exact(&mut std::fs::File::open(disk)?, &mut kept)?;
        if kept != table[..head] {
            return Err(refuse("its partition table differs".into()));
        }
    }
    Ok(())
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
    /// A program's failure line ([`FAIL_LINE`]), and the first one the guest printed.
    fail: Regex,
    failed: Option<String>,
}

/// A program's own failure, as the test programs print it (`[name] FAIL: what`, behind a console
/// prefix under `init`): the cause a wait that ends without its line reports first.
const FAIL_LINE: &str = r"\[[^\]\s]+\] FAIL\b";

/// What ended a wait for an expected line: `wait`, after the first failure the guest reported, if
/// it reported one, since that is the cause and the wait only its consequence.
fn after_failure(failed: Option<&str>, wait: String) -> String {
    match failed {
        Some(line) => format!("the guest reported a failure ({line}); then {wait}"),
        None => wait,
    }
}

/// The loader's first line (loader/src/main.rs), bare: nothing but the loader and `init` prints
/// bare, and `init` never prints this.
const LOADER_LINE: &str = r"^loader: Redoubt rv(64|32) loader, boot hart \d+$";

/// A line `log-server` prints for `DONE`. Anchored: every relayed line starts `[pid N]` or
/// `[badge N]`, so no program's text can start this way.
const DONE_LINE: &str = "[server] done:";

/// A console line as a terminal shows its text: the line with its control sequences removed.
/// A CSI sequence goes whole (`ESC [`, its parameter and intermediate bytes, its final byte), any
/// other escape goes with the one character after it, every other ASCII control character but tab
/// goes, DEL among them, and the line ends without trailing space, as the line itself does. Nothing
/// is drawn: a carriage return does not take the line back to its start, so the prefix the log
/// server or `consoled` gave the line stays first, and no text a program printed can stand in front
/// of it.
fn shown(line: &str) -> String {
    let mut shown = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.next() == Some('[') {
                    // Up to and with the first character that is not a parameter or intermediate.
                    for c in chars.by_ref() {
                        if !('\x20'..='\x3f').contains(&c) {
                            break;
                        }
                    }
                }
            }
            '\t' => shown.push(c),
            c if c.is_ascii_control() => {}
            c => shown.push(c),
        }
    }
    shown.truncate(shown.trim_end().len());
    shown
}

/// The first of `forbid` that matches a line, as it came (`raw`) or as it is shown: a forbidden
/// text split by control sequences is still caught, and so is a control sequence a case forbids.
fn forbidden<'a>(forbid: &'a [Regex], raw: &str, shown: &str) -> Option<&'a Regex> {
    forbid.iter().find(|p| p.is_match(raw) || p.is_match(shown))
}

impl Console {
    /// The next line. `forbid` and the verdict lines are judged on it as it came and as it is
    /// shown ([`shown`]); `expect`, `expect_after`, captures, inputs and the poke match the shown
    /// text, which [`Line::Text`] carries. The log keeps the line as it came.
    fn next(&mut self, until: Instant) -> Result<Line> {
        let raw = match self.lines.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => return Ok(Line::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Line::Exited),
        };
        writeln!(self.log, "{raw}")?;
        self.seen = true;
        let line = shown(&raw);
        if let Some(pattern) = forbidden(&self.forbid, &raw, &line) {
            return Ok(Line::Forbidden(format!("forbidden output /{pattern}/: {raw}")));
        }
        if self.failed.is_none() && self.fail.is_match(&raw) {
            self.failed = Some(raw.clone());
        }
        if let Some(announced) = &self.announced {
            // A reboot: the next boot's `init` announces the reporter again, under a console
            // connection of the new `consoled`. The verdict is still one line in the whole run.
            if self.loader.is_match(&raw) {
                self.reporter = None;
            }
            // `init`'s own lines are the only bare ones (`consoled` prefixes every other), so the
            // anchored announcement cannot be a program's; and `init` announces each child once a
            // boot.
            if let Some(id) = announced.captures(&raw).and_then(|c| c.get(1)) {
                if self.reporter.is_some() {
                    return Ok(Line::Forbidden(format!("the reporter announced twice: {raw}")));
                }
                self.reporter = Some(format!("[con {}] ", id.as_str()));
            }
            // A verdict in either form is the reporter's, or forged.
            if raw.contains(PASSED) || line.contains(PASSED) {
                match &self.reporter {
                    Some(prefix) if !self.passed_seen && raw.starts_with(prefix.as_str()) => {
                        self.passed_seen = true
                    }
                    _ => return Ok(Line::Forbidden(format!("a PASSED line not the reporter's: {raw}"))),
                }
            }
        }
        if raw.starts_with(DONE_LINE) || line.starts_with(DONE_LINE) {
            match &self.done {
                Some(done) if !self.done_seen && done.is_match(&raw) => self.done_seen = true,
                _ => return Ok(Line::Forbidden(format!("a DONE line not the reporter's: {raw}"))),
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
/// `launched`: launch's command line for `image`, booted as it is, and the key its sessions log
/// in with (`launch.rs`).
pub fn run(
    image: &Image,
    launched: Option<(Command, &Path)>,
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

    let (mut qemu, identity) = match launched {
        Some((qemu, identity)) => (qemu, Some(identity)),
        None => (image.qemu(), None),
    };
    let qmp_path = qmp_socket();
    // Declared before the guest, so dropped after QEMU is reaped.
    let _qmp = boot.memory.then(|| Removed(qmp_path.clone()));
    if boot.memory {
        qemu.args(["-qmp", &format!("unix:{},server=on,wait=off", qmp_path.display())]);
    }
    // launch's line has the console on stdio already (`-nographic`).
    if identity.is_none() {
        qemu.args(["-display", "none", "-monitor", "none", "-serial", "stdio"]);
    }
    qemu.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
        fail: Regex::new(FAIL_LINE)?,
        failed: None,
    };
    let deadline = Instant::now() + Duration::from_secs_f64(boot.timeout_secs);
    let mut next = 0;
    while next < expect.len() {
        match console.next(deadline)? {
            Line::Text(line) if expect[next].is_match(&line) => next += 1,
            Line::Text(_) => {}
            Line::Forbidden(why) => return Ok(Verdict::Fail(why)),
            Line::Timeout => {
                let failure = format!("timed out waiting for /{}/", expect[next]);
                return Ok(Verdict::Fail(after_failure(console.failed.as_deref(), failure)));
            }
            Line::Exited => {
                let failure = after_failure(
                    console.failed.as_deref(),
                    format!("guest exited while waiting for /{}/", expect[next]),
                );
                return Ok(Verdict::Fail(exited(failure, guest.0.wait()?, &mut console, &stderr)?));
            }
        }
    }
    if !boot.session.is_empty() {
        if let Some(why) = run_sessions(
            &mut console,
            (guest.0.id(), image.smp),
            boot,
            workspace,
            forwards,
            identity,
            log,
            deadline,
        )? {
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
    if boot.memory {
        if guest.0.try_wait()?.is_some() {
            return Ok(Verdict::Fail("guest exited before its stack measurement".into()));
        }
        let measured = measure_stacks(image, boot, workspace, log, &qmp_path, &mut console, deadline);
        match measured {
            Ok(measured) => {
                for line in measured.lines {
                    println!("{line}");
                    writeln!(console.log, "{line}")?;
                }
                if !measured.failures.is_empty() {
                    return Ok(Verdict::Fail(measured.failures.join("; ")));
                }
            }
            Err(error) => return Ok(Verdict::Fail(format!("stack measurement: {error:#}"))),
        }
    }
    Ok(Verdict::Pass(console.captured))
}

/// Stop the running guest before QMP saves its physical RAM, beside the case's log. A dump that
/// lacks a declared server's heap record lets the guest run on, its console read under the case's
/// `forbid`, and is taken again, until every record is present or the case's `deadline`
/// ([`memory::until_started`]). The dump is deleted once scanned; a scan that fails, as on a
/// duplicate or out-of-range unit, keeps it as the evidence. Its errors are case failures, never
/// guest verdicts.
fn measure_stacks(
    image: &Image,
    boot: &Boot,
    workspace: &Path,
    log: &Path,
    qmp_path: &Path,
    console: &mut Console,
    deadline: Instant,
) -> Result<memory::Measurement> {
    let servers = memory::servers(boot, workspace)?;
    let stream =
        UnixStream::connect(qmp_path).with_context(|| format!("connecting to {}", qmp_path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut greeting = String::new();
    reader.read_line(&mut greeting)?;
    if !greeting.contains("\"QMP\"") {
        bail!("QMP did not greet the bench");
    }
    qmp_command(&mut reader, &mut writer, json!({"execute":"qmp_capabilities"}))?;
    qmp_command(&mut reader, &mut writer, json!({"execute":"stop"}))?;
    let dump = log.with_extension("ram");
    let bytes = u64::from(image.memory_mib) * 1024 * 1024;
    let qmp = std::cell::RefCell::new((reader, writer));
    let result = memory::until_started(
        deadline,
        || {
            let (reader, writer) = &mut *qmp.borrow_mut();
            qmp_command(
                reader,
                writer,
                json!({"execute":"pmemsave", "arguments":{
                    "val": 0x8000_0000u64, "size": bytes, "filename": dump.display().to_string()
                }}),
            )?;
            let file = std::fs::File::open(&dump).with_context(|| format!("opening {}", dump.display()))?;
            memory::scan(BufReader::new(file), bytes, &servers)
        },
        |pause| {
            let (reader, writer) = &mut *qmp.borrow_mut();
            qmp_command(reader, writer, json!({"execute":"cont"}))?;
            let until = Instant::now() + pause;
            loop {
                match console.next(until)? {
                    Line::Text(_) => {}
                    Line::Forbidden(why) => bail!("{why}"),
                    Line::Timeout => break,
                    Line::Exited => bail!("the guest exited while the bench waited for its servers to start"),
                }
            }
            qmp_command(reader, writer, json!({"execute":"stop"}))
        },
    );
    if result.is_ok() {
        std::fs::remove_file(&dump).ok();
    }
    result
}

/// Where QEMU listens for QMP: a Unix socket's path must be under 108 bytes, which a run
/// directory deep in a worktree can pass, so it goes in the temporary directory, named for this
/// bench and the boot.
fn qmp_socket() -> PathBuf {
    static BOOTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let boot = BOOTS.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("redoubt-qmp-{}-{boot}.sock", std::process::id()))
}

/// QMP may send asynchronous events between a command and its reply; only `return` or `error`
/// answers the command.
fn qmp_command(reader: &mut BufReader<UnixStream>, writer: &mut UnixStream, command: Value) -> Result<()> {
    writeln!(writer, "{command}")?;
    writer.flush()?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            bail!("QMP closed before replying to {command}");
        }
        let reply: Value = serde_json::from_str(&line).context("QMP reply is not JSON")?;
        if let Some(error) = reply.get("error") {
            bail!("QMP {command}: {error}");
        }
        if reply.get("return").is_some() {
            return Ok(());
        }
    }
}

/// A case's `expect_after`: patterns each matching a console line, in order, from the sessions'
/// start on.
struct After {
    patterns: Vec<Regex>,
    next: usize,
}

impl After {
    fn new(patterns: &[String]) -> Result<After> {
        Ok(After { patterns: patterns.iter().map(|p| Regex::new(p)).collect::<Result<_, _>>()?, next: 0 })
    }

    /// Takes one console line: the next pattern, if it matches.
    fn see(&mut self, line: &str) {
        if self.patterns.get(self.next).is_some_and(|p| p.is_match(line)) {
            self.next += 1;
        }
    }

    /// The first pattern not yet matched, if any.
    fn missing(&self) -> Option<&str> { self.patterns.get(self.next).map(Regex::as_str) }
}

/// Run the case's SSH sessions while still watching the console, so that a panic or the guest
/// dying during a session fails the case, then read on until every `expect_after` pattern has
/// matched or the deadline passes. Returns why it failed, if it did. With `[idle]`, a session's
/// `idle` step is measured on `guest`, QEMU's pid and the harts it runs, and judged.
#[allow(clippy::too_many_arguments)]
fn run_sessions(
    watched: &mut Console,
    guest: (u32, u32),
    boot: &Boot,
    workspace: &Path,
    forwards: &[Forward],
    identity: Option<&Path>,
    log: &Path,
    deadline: Instant,
) -> Result<Option<String>> {
    let logs = log.parent().context("log has no directory")?;
    let prefix = log.file_stem().context("log has no name")?.to_string_lossy();
    let host_key = boot.net.as_ref().and_then(|net| net.host_key.as_deref());
    let server = ssh::Server::Guest { forwards, host_key, identity };
    let abort = AtomicBool::new(false);
    let mut after = After::new(&boot.expect_after)?;
    let (idle_tx, idle_rx) = mpsc::channel::<ssh::IdleEdge>();
    let (pid, harts) = guest;
    let mut window = boot
        .idle
        .as_ref()
        .map(|_| idle::Window::new(pid, harts, log.with_extension("int.log")))
        .transpose()?;
    // The idle step's edges, as the session told them: the window starts or ends on the console,
    // and the session, waiting, is told it has.
    let mut idle_edges = |watched: &mut Console| -> Result<()> {
        while let Ok((on, done)) = idle_rx.try_recv() {
            if let Some(window) = window.as_mut() {
                match on {
                    true => window.start(&mut watched.stdin)?,
                    false => window.end(&mut watched.stdin)?,
                }
            }
            done.send(()).ok();
        }
        Ok(())
    };
    let failed: Option<String> = std::thread::scope(|scope| -> Result<Option<String>> {
        let (server, prefix, abort) = (&server, &prefix, &abort);
        let idle = boot.idle.is_some().then_some(idle_tx);
        let sessions = scope
            .spawn(move || ssh::run(workspace, &boot.session, server, logs, prefix, deadline, abort, idle));
        let mut console: Result<Option<String>> = Ok(None);
        while !sessions.is_finished() && matches!(console, Ok(None)) {
            if let Err(e) = idle_edges(watched) {
                console = Err(e);
                break;
            }
            console = match watched.next(Instant::now() + Duration::from_millis(50)) {
                Ok(Line::Text(line)) => {
                    after.see(&line);
                    Ok(None)
                }
                Ok(Line::Timeout) => Ok(None),
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
    })?;
    // A failed session can leave QEMU's `log int` on: harmless, as QEMU ends with the case.
    if failed.is_some() {
        return Ok(failed);
    }
    if let (Some(window), Some(ceilings)) = (&window, &boot.idle) {
        let (summary, over) = window.judge(ceilings)?;
        writeln!(watched.log, "{summary}")?;
        if over.is_some() {
            return Ok(over);
        }
    }
    while let Some(pattern) = after.missing() {
        let pattern = pattern.to_string();
        match watched.next(deadline)? {
            Line::Text(line) => after.see(&line),
            Line::Forbidden(why) => return Ok(Some(why)),
            Line::Timeout => {
                return Ok(Some(format!("timed out waiting for /{pattern}/ after the sessions")));
            }
            Line::Exited => {
                return Ok(Some(format!("guest exited while waiting for /{pattern}/ after the sessions")));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `expect_after` matches in order: a line that matches a later pattern first does not count
    /// for it, and the first pattern unmatched is the one a miss names.
    #[test]
    fn expect_after_matches_in_order_and_names_the_first_miss() {
        let mut after = After::new(&["^b$".into(), "^c$".into()]).unwrap();
        assert_eq!(after.missing(), Some("^b$"));
        after.see("c");
        after.see("a");
        assert_eq!(after.missing(), Some("^b$"), "a later pattern's line before the first's");
        after.see("b");
        assert_eq!(after.missing(), Some("^c$"));
        after.see("c");
        assert_eq!(after.missing(), None, "every pattern matched, in order");
        assert_eq!(After::new(&[]).unwrap().missing(), None);
    }

    /// The shell's redraws, as the console carries them, show as the text a terminal leaves.
    #[test]
    fn a_line_is_shown_without_its_control_sequences() {
        let con = "[con e4d341640dc0d5c0] ";
        assert_eq!(shown(&format!("{con}\r55")), format!("{con}55"));
        assert_eq!(
            shown(&format!("{con}\r\x1b[1A\r\x1b[J/ (2)> Enum.sum(1..10)")),
            format!("{con}/ (2)> Enum.sum(1..10)"),
        );
        assert_eq!(shown(&format!("{con}\r\x1b[J/ (3)> \r\x1b[7C")), format!("{con}/ (3)>"));
        assert_eq!(shown("a\x1b[?25;1hb\x1b7c\x1b"), "abc", "private CSI, a two-byte escape, a cut one");
        assert_eq!(shown("a\tb\x07\x7fc"), "a\tbc", "tab stays; BEL and DEL go");
        assert_eq!(shown("x\x1b[1"), "x", "a CSI the line cuts off");
    }

    /// Stripping never moves text in front of the prefix `consoled` gave the line: a program that
    /// prints a carriage return and another connection's prefix is still shown under its own, so
    /// no anchored pattern takes the line as the other connection's.
    #[test]
    fn a_shown_line_keeps_its_own_prefix_first() {
        let line = shown("[con aaaaaaaaaaaaaaaa] \r\x1b[J[con bbbbbbbbbbbbbbbb] x");
        assert_eq!(line, "[con aaaaaaaaaaaaaaaa] [con bbbbbbbbbbbbbbbb] x");
        let other = Regex::new(r"^\[con bbbbbbbbbbbbbbbb\] x$").unwrap();
        let any = Regex::new(r"^\[con [0-9a-f]{16}\] x$").unwrap();
        assert!(!other.is_match(&line) && !any.is_match(&line));
    }

    /// `forbid` sees a line both ways: a forbidden word split by a sequence, and a raw sequence a
    /// case forbids, are each caught.
    #[test]
    fn forbid_matches_the_line_as_it_came_or_as_shown() {
        let forbid = [Regex::new("PANIC").unwrap(), Regex::new(r"\x1b").unwrap()];
        let split = "[con a] PA\x1b[0mNIC";
        assert_eq!(forbidden(&forbid, split, &shown(split)).map(Regex::as_str), Some("PANIC"));
        let escape = "[con a] \x1b]0;title\x07";
        assert_eq!(forbidden(&forbid, escape, &shown(escape)).map(Regex::as_str), Some(r"\x1b"));
        assert!(forbidden(&forbid, "[con a] clean", "[con a] clean").is_none());
    }

    /// A kept disk already there is attached as it is; one not there yet is made; a disk that
    /// is not kept is made afresh over one that is there.
    #[test]
    fn a_kept_disk_is_attached_as_it_is() {
        let disk = std::env::temp_dir().join(format!("testbench-qemu-kept-{}.img", std::process::id()));
        let kept = boot("[disk]\nsize_kib = 64\nkeep = true\n");
        virtio_devices(&kept, &disk, None, &[]).unwrap();
        assert_eq!(std::fs::metadata(&disk).unwrap().len(), 64 * 1024, "made the first time");
        let mut files = vec![0u8; 64 * 1024];
        files[..16].copy_from_slice(b"a person's files");
        std::fs::write(&disk, &files).unwrap();
        let (args, _) = virtio_devices(&kept, &disk, None, &[]).unwrap();
        assert_eq!(std::fs::read(&disk).unwrap(), files);
        assert!(args.iter().any(|a| a.contains(&disk.display().to_string())));
        virtio_devices(&boot("[disk]\nsize_kib = 64\n"), &disk, None, &[]).unwrap();
        assert_eq!(std::fs::read(&disk).unwrap(), vec![0; 64 * 1024], "afresh when not kept");
        std::fs::remove_file(&disk).ok();
    }

    /// A kept disk laid out otherwise than a fresh one is refused, and left as it is: another
    /// size, or another partition table; the same table with other contents is attached.
    #[test]
    fn a_kept_disk_of_another_layout_is_refused() {
        let disk = std::env::temp_dir().join(format!("testbench-qemu-layout-{}.img", std::process::id()));
        let two = boot("[disk]\nsize_kib = 64\npartitions = 2\nkeep = true\n");
        virtio_devices(&two, &disk, None, &[]).unwrap();
        let mut bytes = std::fs::read(&disk).unwrap();
        let end = bytes.len();
        bytes[end - 16..].copy_from_slice(b"a person's files");
        std::fs::write(&disk, &bytes).unwrap();
        virtio_devices(&two, &disk, None, &[]).unwrap();
        assert_eq!(std::fs::read(&disk).unwrap(), bytes, "the same table: attached as it is");
        let three = boot("[disk]\nsize_kib = 64\npartitions = 3\nkeep = true\n");
        let why = virtio_devices(&three, &disk, None, &[]).unwrap_err().to_string();
        assert!(why.contains("its partition table differs") && why.contains("--fresh-disk"), "{why}");
        let bigger = boot("[disk]\nsize_kib = 128\npartitions = 2\nkeep = true\n");
        let why = virtio_devices(&bigger, &disk, None, &[]).unwrap_err().to_string();
        assert!(why.contains("65536 bytes, not 131072"), "{why}");
        assert_eq!(std::fs::read(&disk).unwrap(), bytes, "never reformatted");
        std::fs::remove_file(&disk).ok();
    }

    /// A kept disk packed from a recipe is held to the recipe's manifest as a pack is: a manifest
    /// whose volume no longer fits its partition refuses the kept disk too, and leaves it as it is.
    #[test]
    fn a_kept_disk_is_held_to_its_recipe_s_manifest() {
        let dir = std::env::temp_dir().join(format!("testbench-qemu-held-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (recipe, manifest, disk) =
            (dir.join("disk.toml"), dir.join("manifest.json"), dir.join("disk.img"));
        let bytes = crate::disk::partition_bytes(64, 1)[0];
        let volume = |bytes: u64| {
            let volumes =
                json!({ "volumes": [{ "name": "data", "partition": 0, "bytes": bytes.to_string() }] });
            std::fs::write(&manifest, volumes.to_string()).unwrap();
        };
        volume(bytes);
        let text = format!(
            "size_kib = 64\nmanifest = {:?}\n[[partition]]\nname = \"data\"\nfs = \"noise\"\n",
            manifest
        );
        std::fs::write(&recipe, text).unwrap();
        let kept = boot(&format!("[disk]\nrecipe = {:?}\nkeep = true\n", recipe));
        virtio_devices(&kept, &disk, None, &[]).unwrap();
        let packed = std::fs::read(&disk).unwrap();
        virtio_devices(&kept, &disk, None, &[]).unwrap();
        volume(bytes + 4096);
        let why = format!("{:#}", virtio_devices(&kept, &disk, None, &[]).unwrap_err());
        assert!(why.contains("is not attached") && why.contains(&format!("bytes {}", bytes + 4096)), "{why}");
        assert_eq!(std::fs::read(&disk).unwrap(), packed, "left as it is");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        let (args, _) = virtio_devices(&boot(devices), &disk, None, &[]).expect("device arguments");
        std::fs::remove_file(&disk).ok();
        std::fs::remove_dir_all(disk.with_extension("peers")).ok();
        args
    }

    /// The containment gate printed `[containment] FAIL: the bystander kept its weight's share
    /// (...)`, which a mis-quoted `forbid` let through, and exited: the bench said only that the
    /// guest exited while waiting. A wait that ends without its line now leads with the failure.
    #[test]
    fn a_wait_cut_short_reports_the_failure_the_guest_printed() {
        let fail = Regex::new(FAIL_LINE).unwrap();
        let line = "[containment] FAIL: the bystander kept its weight's share (share 758 of 1000, floor 783)";
        for printed in [line, "[con 3] [ipc] FAIL: panic: oops", "[pid 4] [wx] FAIL"] {
            assert!(fail.is_match(printed), "{printed}");
        }
        for other in ["[containment] ok: FAIL is a word here", "[x] FAILED", "FAIL: bare", "[a b] FAIL: x"] {
            assert!(!fail.is_match(other), "{other}");
        }
        let wait = "guest exited while waiting for /^done$/: QEMU exited with exit status: 0".to_string();
        assert_eq!(
            after_failure(Some(line), wait.clone()),
            format!("the guest reported a failure ({line}); then {wait}")
        );
        // Nothing reported: the wait alone, as the must-fail cases match it.
        assert_eq!(after_failure(None, wait.clone()), wait);
    }

    /// Set in the stand-in bench that `a_killed_bench_leaves_no_qemu` starts: QEMU's QMP socket.
    const STAND_IN_QMP: &str = "TESTBENCH_STAND_IN_QMP";

    /// A bench killed outright (SIGKILL, nothing dropped) takes its QEMU with it: here this test
    /// binary, run again, stands in for the bench, starts QEMU the way `Image::qemu` does, and is
    /// killed.
    #[test]
    fn a_killed_bench_leaves_no_qemu() {
        let qemu = "qemu-system-riscv64";
        if let Some(qmp) = std::env::var_os(STAND_IN_QMP) {
            // The stand-in: print QEMU's pid, then wait to be killed.
            let mut command = Command::new(qemu);
            exit_with_parent(&mut command)
                .args(["-machine", "virt", "-bios", "none", "-display", "none", "-monitor", "none"])
                .args(["-serial", "none", "-S", "-qmp"])
                .arg(format!("unix:{},server=on,wait=off", qmp.to_string_lossy()))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = command.spawn().expect("starting QEMU");
            println!("qemu-pid {}", child.id());
            std::thread::sleep(Duration::from_secs(600));
            return;
        }
        // QEMU greets a QMP client only from its main loop, so after it is running: killing the
        // stand-in earlier would test only a race.
        let qmp = std::env::temp_dir().join(format!("testbench-exit-with-parent-{}.qmp", std::process::id()));
        std::fs::remove_file(&qmp).ok();
        let name = module_path!().split_once("::").map_or("", |(_, path)| path).to_string()
            + "::a_killed_bench_leaves_no_qemu";
        let mut bench = Command::new(std::env::current_exe().expect("this test binary"))
            .args([name.as_str(), "--exact", "--nocapture", "--test-threads=1"])
            .env(STAND_IN_QMP, &qmp)
            .stdout(Stdio::piped())
            .spawn()
            .expect("starting the stand-in");
        let pid: u32 = BufReader::new(bench.stdout.take().expect("the stand-in's stdout"))
            .lines()
            .map_while(Result::ok)
            // The test harness prints the test's name first, on the same line.
            .find_map(|line| line.rsplit_once("qemu-pid ").and_then(|(_, pid)| pid.parse().ok()))
            .expect("QEMU's pid");
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
        bench.kill().expect("killing the stand-in");
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

    /// The QEMU the bench runs is usable; a QEMU refusing an option is reported with the
    /// version needed and QEMU's own complaint, and one that cannot run says why.
    #[test]
    fn a_qemu_lacking_an_option_is_named() {
        for qemu in ["qemu-system-riscv64", "qemu-system-riscv32"] {
            assert_eq!(probe(qemu, &[], ""), Ok(()));
            let why = probe(qemu, &["-no-such-option"], "QEMU 99.0").expect_err("an unknown option");
            assert!(why.contains("needs QEMU 99.0 or later, this is \"QEMU emulator version "), "{why}");
            assert!(why.ends_with("-no-such-option: invalid option"), "{why}");
        }
        let why = probe("qemu-system-no-such-width", &[], "").expect_err("a missing binary");
        assert!(why.starts_with("`qemu-system-no-such-width` could not be run: "), "{why}");
    }

    /// Each boot's QMP socket is its own, and short enough to bind wherever the run directory is.
    #[test]
    fn a_qmp_socket_path_binds() {
        let (a, b) = (qmp_socket(), qmp_socket());
        assert_ne!(a, b);
        assert!(a.as_os_str().len() < 108, "{}", a.display());
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
    /// data disk, attached read-only: the run's one pack, with a case's damage on this boot's copy
    /// of it, one bit, and the run's pack untouched.
    #[test]
    fn the_userland_disk_sits_on_its_slot_read_only() {
        let dir = std::env::temp_dir().join(format!("testbench-qemu-userland-{}", std::process::id()));
        let stage = dir.join("objects");
        let module: Vec<u8> = (0..20_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let objects = vec![(String::from("Elixir.Version.beam"), module)];
        crate::userland::write(&objects, &stage).unwrap();
        let recipe: crate::disk::Recipe = toml::from_str(
            "size_kib = 1024\n[[partition]]\nname = \"system\"\nfs = \"littlefs\"\nverity = true\n",
        )
        .unwrap();
        let (packed, verified) = crate::disk::pack(&recipe, Path::new("/"), Some(&stage)).unwrap();
        let image = dir.join("userland-pack.img");
        std::fs::write(&image, &packed).unwrap();
        let staged = Staged { objects: stage, image: image.clone(), verified };
        let disk = dir.join("boot.img");
        let case = "[disk]\nsize_kib = 64\n[userland]\nrecipe = \"image/userland.toml\"\nflip = \"Elixir.Version.beam\"\n";
        let (args, _) = virtio_devices(&boot(case), &disk, Some(&staged), &[]).unwrap();
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
        let booted = std::fs::read(disk.with_extension("userland.img")).unwrap();
        assert_eq!(booted.iter().zip(&packed).filter(|(a, b)| a != b).count(), 1);
        assert_eq!(std::fs::read(&image).unwrap(), packed, "the run's pack is untouched");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A poke gets a UDP forward from a host port of its own to its guest port, listed with the
    /// TCP forwards; a case without one gets none.
    #[test]
    fn a_poke_gets_a_udp_forward() {
        let case = "[net]\nforward = [8000]\n[net.poke]\nport = 47000\npayload = 'x'\nafter = 'go'\n";
        let disk = std::env::temp_dir().join(format!("testbench-qemu-poke-{}.img", std::process::id()));
        let (args, forwards) = virtio_devices(&boot(case), &disk, None, &[]).expect("device arguments");
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
