//! A boot at rest, measured from outside (`[idle]`, docs/testbench.md "The machine at rest"):
//! over a session's `idle` step, the interrupts and exceptions each hart takes, from QEMU's own
//! log, and QEMU's host CPU time. The log is turned on for that window alone, through the monitor
//! that `-nographic` multiplexes on the console (Ctrl-A c), so the command line booted is
//! launch's, unchanged; outside the window every system call would be logged. The window opens
//! once the frames a login freed are all zeroed and back, as the kernel's console says
//! (kernel/memory.md, "Frames in flight"), so it never measures that drain, which it prints beside.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use crate::case::Idle;

/// QEMU's mux escape, then `c`: the console's input goes to the monitor, or back.
const SWITCH: &[u8] = b"\x01c";

/// The longest the idle step waits for the frames in flight to come back to none before its window
/// opens. About 35,000 frames, what the boot and the login leave in flight, wait until every hart
/// first idles after the login; at four harts alone they were back 0.32 to 0.39 s after the step
/// began on rv64 and 5.8 to 6.2 s on rv32, six runs each, the last given back at the lock's next
/// take, at rest the next timer interrupt. About five times rv32's most: margin for a loaded host.
/// A drain that takes longer fails the case.
pub const DRAIN_WAIT: Duration = Duration::from_secs(30);

/// The kernel's console line when the frames in flight reach its threshold (`mem: N frames in
/// flight`), and when they are next none (`mem: frames in flight 0, at most N`). Only the kernel's
/// lines are bare: `consoled` prefixes every program's.
const RISE: (&str, &str) = ("mem: ", " frames in flight");
const FALL: &str = "mem: frames in flight 0, at most ";

/// What the console said of the frames in flight: a drain opens at the kernel's first line and
/// closes at its second.
#[derive(Debug, Default)]
struct Drain {
    /// The threshold the open drain's frames passed, if one is open.
    open: Option<u64>,
    /// The most frames the last closed drain had in flight.
    peak: Option<u64>,
    /// The drains closed so far.
    closed: u32,
    /// The idle step's start, while the window waits: when, QEMU's CPU time then, and the drains
    /// closed by then.
    held: Option<(Instant, Duration, u32)>,
    /// The console fell silent after the step started: every line queued before it was read.
    caught_up: bool,
    /// The wait, once the window opened: wall time and QEMU's CPU time, and whether a drain ended
    /// in it.
    waited: Option<(Duration, Duration, bool)>,
    /// Drains that opened inside the window: named on the window's own line, once it has ended.
    inside: u32,
}

/// What [`Window::poll`] found.
#[derive(Debug, PartialEq)]
pub enum Poll {
    /// Nothing to do: no step waits, or its wait goes on.
    Waiting,
    /// The window just opened: tell the session.
    Started,
    /// The drain outlasted [`DRAIN_WAIT`]: why the case fails.
    Failed(String),
}

/// One idle window: started and ended as the session's step is, then judged.
pub struct Window {
    pid: u32,
    /// The guest's hart count: each must show it measured something.
    harts: u32,
    log: PathBuf,
    /// The host's clock ticks per second, the unit of `/proc/<pid>/stat`'s times.
    ticks: u64,
    start: Option<(Instant, Duration)>,
    end: Option<(Instant, Duration)>,
    drain: Drain,
}

impl Window {
    /// The window of QEMU `pid`, a guest of `harts` harts, whose log goes to `log`.
    pub fn new(pid: u32, harts: u32, log: PathBuf) -> Result<Window> {
        let getconf =
            std::process::Command::new("getconf").arg("CLK_TCK").output().context("running getconf")?;
        let ticks: u64 =
            String::from_utf8_lossy(&getconf.stdout).trim().parse().context("getconf CLK_TCK")?;
        ensure!(ticks > 0, "no clock tick rate");
        Ok(Window { pid, harts, log, ticks, start: None, end: None, drain: Drain::default() })
    }

    /// A console line, as shown: the kernel's lines on the frames in flight open and close a drain.
    pub fn see(&mut self, line: &str) {
        if let Some(peak) = line.strip_prefix(FALL).and_then(|n| n.trim_end().parse().ok()) {
            self.drain.open = None;
            self.drain.peak = Some(peak);
            self.drain.closed += 1;
        } else if let Some(at) = line
            .strip_prefix(RISE.0)
            .and_then(|rest| rest.trim_end().strip_suffix(RISE.1))
            .and_then(|n| n.parse().ok())
        {
            self.drain.open = Some(at);
            if self.start.is_some() && self.end.is_none() {
                self.drain.inside += 1;
            }
        }
    }

    /// The console was silent for a while: every line it had queued is read.
    pub fn silence(&mut self) { self.drain.caught_up = true; }

    /// The idle step starts: the window opens at a [`Self::poll`] once the console has caught up
    /// and no drain is open.
    pub fn step(&mut self) -> Result<()> {
        ensure!(self.start.is_none() && self.drain.held.is_none(), "a second idle window");
        self.drain.held = Some((Instant::now(), cpu_time(self.pid, self.ticks)?, self.drain.closed));
        self.drain.caught_up = false;
        Ok(())
    }

    /// Open the window if the step waits and the frames in flight are back to none, after at least
    /// one drain closed since the sessions began: the boot and the login leave tens of thousands of
    /// frames in flight until then, so a kernel that says nothing fails too. Fail the case if they are not
    /// within [`DRAIN_WAIT`] of the step's start.
    pub fn poll(&mut self, console: &mut dyn Write) -> Result<Poll> {
        let Some((t0, cpu0, closed)) = self.drain.held else { return Ok(Poll::Waiting) };
        let waited = t0.elapsed();
        if self.drain.caught_up && self.drain.open.is_none() && self.drain.peak.is_some() {
            let cpu = cpu_time(self.pid, self.ticks)?.saturating_sub(cpu0);
            self.drain.held = None;
            self.drain.waited = Some((waited, cpu, self.drain.closed > closed));
            self.start(console)?;
            return Ok(Poll::Started);
        }
        if waited <= DRAIN_WAIT {
            return Ok(Poll::Waiting);
        }
        Ok(Poll::Failed(format!(
            "idle: the frames in flight did not come back to 0 within {} s of the idle step: {}",
            DRAIN_WAIT.as_secs(),
            self.drain_line()
        )))
    }

    /// The drain beside the window, one line: what the step waited for, how long, and what QEMU
    /// spent meanwhile.
    pub fn drain_line(&self) -> String {
        let peak = |p: Option<u64>| p.map_or("none seen".to_string(), |n| format!("at most {n} frames"));
        let what = match (self.drain.held, self.drain.waited) {
            // Waiting still: every condition the window waits on that is unmet.
            (Some((t0, ..)), _) => {
                let mut unmet = Vec::new();
                if !self.drain.caught_up {
                    unmet.push("the console never fell silent for 50 ms".to_string());
                }
                match (self.drain.open, self.drain.peak) {
                    (Some(at), _) => unmet.push(format!("a drain still open, over {at} frames in flight")),
                    (None, None) => unmet.push("no drain ended since the sessions began".to_string()),
                    (None, Some(_)) => {}
                }
                format!("{} {:.1} s after the step began", unmet.join(" and "), t0.elapsed().as_secs_f64())
            }
            (None, Some((wall, cpu, true))) => format!(
                "back to 0 frames in flight {:.2} s after the step began ({}), QEMU {:.2} s of host CPU \
                 meanwhile ({:.3} cores); the window opened then",
                wall.as_secs_f64(),
                peak(self.drain.peak),
                cpu.as_secs_f64(),
                cpu.as_secs_f64() / wall.as_secs_f64().max(f64::EPSILON)
            ),
            (None, Some((wall, _, false))) => format!(
                "none open at the step and none since (the last: {}); the window opened {:.2} s after it \
                 began",
                peak(self.drain.peak),
                wall.as_secs_f64()
            ),
            (None, None) => "the step never began".to_string(),
        };
        format!("[idle] drain: {what}")
    }

    /// Turn QEMU's interrupt log on, through the monitor on `console`, and take its CPU time.
    fn start(&mut self, console: &mut dyn Write) -> Result<()> {
        ensure!(self.start.is_none(), "a second idle window");
        let path = self.log.display().to_string();
        ensure!(!path.contains('\n'), "a log path with a newline");
        monitor(console, &[&format!("logfile {path}"), "log int"])?;
        self.start = Some((Instant::now(), cpu_time(self.pid, self.ticks)?));
        Ok(())
    }

    /// Take QEMU's CPU time, and turn its log off, which closes the file.
    pub fn end(&mut self, console: &mut dyn Write) -> Result<()> {
        ensure!(self.start.is_some() && self.end.is_none(), "an idle window ended that never started");
        self.end = Some((Instant::now(), cpu_time(self.pid, self.ticks)?));
        monitor(console, &["log none"])
    }

    /// The window's measures as one line, naming any drain that opened inside it, and why it fails
    /// `ceilings`, if it does. A window that measured nothing is an error, never a pass: no log, no
    /// timer interrupt on any hart, no system call from user mode, or no host CPU time.
    pub fn judge(&self, ceilings: &Idle) -> Result<(String, Option<String>)> {
        let (Some((t0, cpu0)), Some((t1, cpu1))) = (self.start, self.end) else {
            bail!("the idle window never ran")
        };
        let seconds = (t1 - t0).as_secs_f64();
        let cores = (cpu1.saturating_sub(cpu0)).as_secs_f64() / seconds;
        settle(&self.log)?;
        let text =
            std::fs::read_to_string(&self.log).with_context(|| format!("reading {}", self.log.display()))?;
        let counts = Counts::parse(&text);
        let inside = match self.drain.inside {
            0 => String::new(),
            n => format!("; {n} drain(s) opened inside the window"),
        };
        // Per frame the card answered: every hart's interrupts, the kernel's firmware calls and the
        // system calls from user mode, the window's background included.
        // Over the span of exactly `ceilings.frames` frames, if the case names it, so a window that
        // held one frame more or fewer reads the same; else over the whole window's frames.
        let whole = (
            counts.interrupts.values().flat_map(|by| by.values()).sum::<u64>(),
            counts.exceptions.get("supervisor_ecall").copied().unwrap_or(0),
            counts.exceptions.get("user_ecall").copied().unwrap_or(0),
        );
        let (over_frames, (interrupts, sbi, ecalls)) = match ceilings.frames {
            Some(n) => (n, counts.span(n).unwrap_or((0, 0, 0))),
            None => (counts.frames, whole),
        };
        let per = |n: u64| n as f64 / over_frames.max(1) as f64;
        let frames = match (counts.frames, ceilings.frames) {
            (0, _) => String::new(),
            (n, None) => format!(
                "; {n} answer frames, per frame {:.1} interrupts, {:.1} SBI calls, {:.1} user ecalls",
                per(interrupts),
                per(sbi),
                per(ecalls)
            ),
            (n, Some(span)) => format!(
                "; {n} answer frames, per frame over the first {span} {:.1} interrupts, {:.1} SBI calls, \
                 {:.1} user ecalls",
                per(interrupts),
                per(sbi),
                per(ecalls)
            ),
        };
        let summary = format!("{}{frames}{inside}", counts.summary(seconds, cores));
        // The floors. A timeout whose payer runs nowhere is armed on one hart (kernel/timer.md,
        // "The hart timer"), so an idle hart may take no timer interrupt in a whole window; a
        // window in which no hart took one was not logged.
        let mut nothing = Vec::new();
        if text.is_empty() {
            nothing.push("the log is empty".to_string());
        }
        if counts.interrupts.values().all(|by| by.get("s_timer").is_none_or(|n| *n == 0)) {
            nothing.push("no hart took a timer interrupt".to_string());
        }
        if counts.exceptions.get("user_ecall").is_none_or(|n| *n == 0) {
            nothing.push("no system call from user mode".to_string());
        }
        if cores <= 0.0 {
            nothing.push("QEMU took no host CPU time".to_string());
        }
        ensure!(nothing.is_empty(), "the idle window measured nothing: {} ({summary})", nothing.join("; "));
        let busiest = counts.interrupts.values().map(|by| by.values().sum::<u64>()).max().unwrap_or(0);
        let mut over = [
            ("interrupts per hart", busiest as f64 / seconds, ceilings.interrupts_per_hart),
            ("user ecalls", ecalls as f64 / seconds, ceilings.user_ecalls),
            ("host cores", cores, ceilings.host_cores),
        ]
        .into_iter()
        .filter_map(|(what, rate, ceiling)| {
            ceiling.filter(|c| rate > *c).map(|c| format!("idle: {what} {rate:.3}/s over its ceiling {c}"))
        })
        .collect::<Vec<_>>();
        let per_frame =
            [ceilings.interrupts_per_frame, ceilings.sbi_calls_per_frame, ceilings.user_ecalls_per_frame];
        ensure!(
            ceilings.frames.is_some() || per_frame.iter().all(Option::is_none),
            "a per-frame ceiling without `frames`"
        );
        if let Some(span) = ceilings.frames.filter(|span| counts.frames <= *span) {
            over.push(format!(
                "idle: {} answer frames, too few to judge {span} (the frame after them ends the span)",
                counts.frames
            ));
        }
        over.extend(
            [
                ("interrupts", per(interrupts), ceilings.interrupts_per_frame),
                ("SBI calls", per(sbi), ceilings.sbi_calls_per_frame),
                ("user ecalls", per(ecalls), ceilings.user_ecalls_per_frame),
            ]
            .into_iter()
            .filter_map(|(what, rate, ceiling)| {
                ceiling
                    .filter(|c| rate > *c)
                    .map(|c| format!("idle: {what} {rate:.1} a frame over its ceiling {c}"))
            }),
        );
        Ok((summary, (!over.is_empty()).then(|| over.join("; "))))
    }
}

/// Wait until QEMU has written the log's last lines after `log none`: its size the same in two
/// reads about 200 ms apart, within about 2 s. A log never made is an error.
fn settle(log: &Path) -> Result<()> {
    let size =
        || std::fs::metadata(log).map(|m| m.len()).with_context(|| format!("no log at {}", log.display()));
    let mut last = size()?;
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(200));
        let now = size()?;
        if now == last {
            return Ok(());
        }
        last = now;
    }
    Ok(())
}

/// Type `commands` at QEMU's monitor, from the console and back.
fn monitor(console: &mut dyn Write, commands: &[&str]) -> Result<()> {
    console.write_all(SWITCH)?;
    for command in commands {
        console.write_all(command.as_bytes())?;
        console.write_all(b"\n")?;
    }
    console.write_all(SWITCH)?;
    console.flush()?;
    Ok(())
}

/// Process `pid`'s user and system CPU time so far (`/proc/<pid>/stat`, fields 14 and 15, in
/// `ticks` a second).
fn cpu_time(pid: u32, ticks: u64) -> Result<Duration> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // The name, in parentheses, may hold spaces; the fields after it do not. Field 3 is first.
    let fields: Vec<&str> = stat.rsplit_once(')').context("a stat line")?.1.split_whitespace().collect();
    let used: u64 = fields
        .get(11..13)
        .context("a short stat line")?
        .iter()
        .map(|f| f.parse::<u64>())
        .sum::<Result<_, _>>()?;
    Ok(Duration::from_secs_f64(used as f64 / ticks as f64))
}

/// What QEMU's `-d int` log counts: each trap a hart takes, as
/// `riscv_cpu_do_interrupt: hart:H, async:A, cause:..., epc:..., tval:..., desc=NAME`.
#[derive(Debug, Default, PartialEq)]
struct Counts {
    /// Interrupts (`async:1`) by hart, then by name.
    interrupts: BTreeMap<u64, BTreeMap<String, u64>>,
    /// Exceptions (`async:0`), every hart's, by name.
    exceptions: BTreeMap<String, u64>,
    /// Device interrupts (`s_external`) in clusters: one within [`FRAME_TRAPS`] traps of the last,
    /// or with no system call from user mode since it, is the same source taken on another hart
    /// (every hart's context takes it, kernel/boot.md). At rest the network card is the one source,
    /// and each cluster one frame it answered: `ipd` and `netd` run before the next.
    frames: u64,
    /// Each frame's first trap, by its place among the log's traps.
    starts: Vec<usize>,
    /// Each trap in order: whether an interrupt, an SBI call, a system call from user mode.
    traps: Vec<(bool, bool, bool)>,
}

/// Traps between two device interrupts that are one frame's, taken on two harts.
const FRAME_TRAPS: usize = 6;

impl Counts {
    fn parse(log: &str) -> Counts {
        let mut counts = Counts::default();
        let mut last_external: Option<usize> = None;
        // A system call from user mode since the last device interrupt.
        let mut user_since = false;
        for (at, line) in log.lines().filter(|l| l.starts_with("riscv_cpu_do_interrupt:")).enumerate() {
            let field = |key: &str| line.split([',', ' ']).find_map(|part| part.strip_prefix(key));
            let (Some(hart), Some(kind), Some(name)) = (field("hart:"), field("async:"), field("desc="))
            else {
                continue;
            };
            let Ok(hart) = hart.parse() else { continue };
            counts.traps.push((kind == "1", name == "supervisor_ecall", name == "user_ecall"));
            if kind == "1" && name == "s_external" {
                if last_external.is_none_or(|last| at - last > FRAME_TRAPS && user_since) {
                    counts.frames += 1;
                    counts.starts.push(counts.traps.len() - 1);
                }
                last_external = Some(at);
                user_since = false;
            }
            if name == "user_ecall" {
                user_since = true;
            }
            let by_name = match kind {
                "1" => counts.interrupts.entry(hart).or_default(),
                _ => &mut counts.exceptions,
            };
            *by_name.entry(name.to_string()).or_default() += 1;
        }
        counts
    }

    /// Interrupts, SBI calls and system calls from user mode over the span of `n` frames: from the
    /// first frame's first trap to the first trap of the frame after the `n`th. None without it.
    fn span(&self, n: u64) -> Option<(u64, u64, u64)> {
        let (first, end) = (*self.starts.first()?, *self.starts.get(usize::try_from(n).ok()?)?);
        Some(self.traps[first..end].iter().fold((0, 0, 0), |(i, s, u), &(int, sbi, user)| {
            (i + u64::from(int), s + u64::from(sbi), u + u64::from(user))
        }))
    }

    /// One line: the window, QEMU's cores, each hart's interrupts per second by name, and the
    /// exceptions per second by name.
    fn summary(&self, seconds: f64, cores: f64) -> String {
        let rates = |by: &BTreeMap<String, u64>| {
            by.iter()
                .map(|(name, n)| format!("{name} {:.1}", *n as f64 / seconds))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let harts = self
            .interrupts
            .iter()
            .map(|(hart, by)| {
                format!("hart {hart} {:.1} ({})", by.values().sum::<u64>() as f64 / seconds, rates(by))
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "[idle] {seconds:.1} s: QEMU {cores:.3} host cores; interrupts/s: {}; exceptions/s: {}",
            if harts.is_empty() { "none".into() } else { harts },
            if self.exceptions.is_empty() { "none".into() } else { rates(&self.exceptions) },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two harts' window, in QEMU's own names: every hart with a timer interrupt, and a system
    /// call from user mode.
    const LOG: &str = "\
riscv_cpu_do_interrupt: hart:0, async:1, cause:0000000000000005, epc:0x80200000, tval:0x0, desc=s_timer
riscv_cpu_do_interrupt: hart:0, async:1, cause:0000000000000003, epc:0x80200000, tval:0x0, desc=m_software
riscv_cpu_do_interrupt: hart:1, async:1, cause:0000000000000005, epc:0x80200000, tval:0x0, desc=s_timer
riscv_cpu_do_interrupt: hart:1, async:1, cause:0000000000000001, epc:0x80200000, tval:0x0, desc=s_software
riscv_cpu_do_interrupt: hart:1, async:0, cause:0000000000000008, epc:0x10000, tval:0x0, desc=user_ecall
riscv_cpu_do_interrupt: hart:0, async:0, cause:0000000000000008, epc:0x10000, tval:0x0, desc=user_ecall
some other line
";

    #[test]
    fn the_log_is_counted_by_hart_and_name() {
        let counts = Counts::parse(LOG);
        assert_eq!(counts.interrupts[&0]["s_timer"], 1);
        assert_eq!(counts.interrupts[&0]["m_software"], 1);
        assert_eq!(counts.interrupts[&1]["s_software"], 1);
        assert!(!counts.interrupts.contains_key(&2));
        assert_eq!(counts.exceptions["user_ecall"], 2);
        let line = counts.summary(2.0, 0.25);
        assert!(line.starts_with("[idle] 2.0 s: QEMU 0.250 host cores; interrupts/s: hart 0 1.0"), "{line}");
        assert!(line.ends_with("exceptions/s: user_ecall 1.0"), "{line}");
    }

    /// A test's directory, removed however the test ends.
    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).ok(); }
    }

    /// A one-second window of `harts` over `log`, its QEMU taking `cpu` of it, and its directory.
    fn window(name: &str, harts: u32, log: &str, cpu: Duration) -> (Window, Dir) {
        let dir = std::env::temp_dir().join(format!("testbench-idle-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("int.log");
        std::fs::write(&path, log).unwrap();
        let t0 = Instant::now();
        let window = Window {
            pid: 0,
            harts,
            log: path,
            ticks: 100,
            start: Some((t0, Duration::ZERO)),
            end: Some((t0 + Duration::from_secs(1), cpu)),
            drain: Drain::default(),
        };
        (window, Dir(dir))
    }

    /// A window not yet opened, over this test's own process, whose CPU time it reads.
    fn unopened() -> Window {
        Window {
            pid: std::process::id(),
            harts: 1,
            log: PathBuf::from("unused"),
            ticks: 100,
            start: None,
            end: None,
            drain: Drain::default(),
        }
    }

    const RISE_LINE: &str = "mem: 1024 frames in flight";
    const FALL_LINE: &str = "mem: frames in flight 0, at most 35440";

    /// With a drain open at the step, the window opens at the kernel's line that the frames in
    /// flight are back to none, not before, and the drain line says what it waited.
    #[test]
    fn the_window_opens_once_the_frames_in_flight_are_back() {
        let (mut window, mut typed) = (unopened(), Vec::new());
        window.see(RISE_LINE);
        window.step().unwrap();
        window.silence();
        assert_eq!(window.poll(&mut typed).unwrap(), Poll::Waiting);
        assert!(typed.is_empty());
        window.see(FALL_LINE);
        assert_eq!(window.poll(&mut typed).unwrap(), Poll::Started);
        assert!(String::from_utf8_lossy(&typed).contains("log int"));
        let line = window.drain_line();
        assert!(line.starts_with("[idle] drain: back to 0 frames in flight "), "{line}");
        assert!(line.contains("(at most 35440 frames), QEMU "), "{line}");
        assert_eq!(window.poll(&mut typed).unwrap(), Poll::Waiting);
    }

    /// With none open, the window still waits for the console to fall silent once, so a drain's
    /// line queued before the step is read first.
    #[test]
    fn the_window_opens_after_the_lines_already_queued() {
        let (mut window, mut typed) = (unopened(), Vec::new());
        window.see(RISE_LINE);
        window.see(FALL_LINE);
        window.step().unwrap();
        assert_eq!(window.poll(&mut typed).unwrap(), Poll::Waiting);
        window.see(RISE_LINE);
        window.silence();
        assert_eq!(window.poll(&mut typed).unwrap(), Poll::Waiting);
        window.see(FALL_LINE);
        assert_eq!(window.poll(&mut typed).unwrap(), Poll::Started);
        // A drain that began after the step ended in its wait, and is the one named.
        assert!(window.drain_line().starts_with("[idle] drain: back to 0 frames in flight "));
        let (mut closed, mut typed) = (unopened(), Vec::new());
        closed.see(RISE_LINE);
        closed.see(FALL_LINE);
        closed.step().unwrap();
        closed.silence();
        assert_eq!(closed.poll(&mut typed).unwrap(), Poll::Started);
        let line = closed.drain_line();
        assert!(
            line.starts_with(
                "[idle] drain: none open at the step and none since (the last: at most 35440 frames)"
            ),
            "{line}"
        );
    }

    /// A kernel that says nothing of the frames in flight never opens the window: the boot and the
    /// login leave far more than the threshold in flight, so no drain ended fails at the bound; so
    /// does a console that never falls silent, and the failure names which.
    #[test]
    fn no_drain_seen_fails_at_the_bound() {
        let (mut quiet, mut typed) = (unopened(), Vec::new());
        quiet.step().unwrap();
        quiet.silence();
        assert_eq!(quiet.poll(&mut typed).unwrap(), Poll::Waiting);
        let (_, cpu, open) = quiet.drain.held.unwrap();
        quiet.drain.held = Some((Instant::now() - DRAIN_WAIT - Duration::from_secs(1), cpu, open));
        let Poll::Failed(why) = quiet.poll(&mut typed).unwrap() else { panic!("the wait did not fail") };
        assert!(why.contains("[idle] drain: no drain ended since the sessions began 31."), "{why}");
        assert!(typed.is_empty());
        // A console never silent is named as the condition unmet, beside the drain's.
        let (mut chatty, mut typed) = (unopened(), Vec::new());
        chatty.see(RISE_LINE);
        chatty.see(FALL_LINE);
        chatty.step().unwrap();
        let (_, cpu, closed) = chatty.drain.held.unwrap();
        chatty.drain.held = Some((Instant::now() - DRAIN_WAIT - Duration::from_secs(1), cpu, closed));
        let Poll::Failed(why) = chatty.poll(&mut typed).unwrap() else { panic!("the wait did not fail") };
        assert!(why.contains("[idle] drain: the console never fell silent for 50 ms 31."), "{why}");
    }

    /// A drain that never ends fails the case at the bound, saying how long it waited and what it
    /// knows of the frames in flight; the window never opens.
    #[test]
    fn a_drain_that_never_ends_fails_at_the_bound() {
        let (mut window, mut typed) = (unopened(), Vec::new());
        window.see(RISE_LINE);
        window.step().unwrap();
        window.silence();
        let (_, cpu, open) = window.drain.held.unwrap();
        window.drain.held = Some((Instant::now() - DRAIN_WAIT - Duration::from_secs(1), cpu, open));
        let Poll::Failed(why) = window.poll(&mut typed).unwrap() else { panic!("the wait did not fail") };
        assert!(why.starts_with("idle: the frames in flight did not come back to 0 within 30 s"), "{why}");
        assert!(why.contains("[idle] drain: a drain still open, over 1024 frames in flight 31."), "{why}");
        assert!(why.contains("over 1024 frames in flight"), "{why}");
        assert!(typed.is_empty());
    }

    /// Only the kernel's bare lines count: a program's line, under `consoled`'s prefix, opens no
    /// drain; a drain opened inside the window is named on the window's own line, which comes
    /// after it ends, and one outside is not.
    #[test]
    fn only_the_kernel_s_lines_open_a_drain_and_one_inside_is_named() {
        let mut program = unopened();
        program.see(&format!("[con 0123456789abcdef] {RISE_LINE}"));
        assert_eq!(program.drain.open, None);
        let (mut open, _dir) = window("inside", 1, LOG, Duration::from_millis(500));
        let end = open.end.take();
        open.see(RISE_LINE);
        open.end = end;
        open.see(RISE_LINE);
        let (summary, over) = open.judge(&Idle::default()).unwrap();
        assert!(summary.ends_with("; 1 drain(s) opened inside the window"), "{summary}");
        assert_eq!(over, None);
        assert!(!open.drain_line().contains("inside"), "{}", open.drain_line());
        let (quiet, _dir) = window("outside", 1, LOG, Duration::from_millis(500));
        let (summary, _) = quiet.judge(&Idle::default()).unwrap();
        assert!(summary.ends_with("exceptions/s: user_ecall 2.0"), "{summary}");
    }

    /// Each ceiling fails the window when its rate is over it; none set, nothing fails.
    #[test]
    fn a_rate_over_its_ceiling_fails() {
        let (window, _dir) = window("ceilings", 2, LOG, Duration::from_millis(500));
        assert_eq!(window.judge(&Idle::default()).unwrap().1, None);
        let tight = Idle {
            interrupts_per_hart: Some(1.5),
            user_ecalls: Some(1.0),
            host_cores: Some(0.4),
            ..Idle::default()
        };
        let why = window.judge(&tight).unwrap().1.unwrap();
        assert!(why.contains("interrupts per hart 2.000/s over its ceiling 1.5"), "{why}");
        assert!(why.contains("user ecalls 2.000/s over its ceiling 1"), "{why}");
        assert!(why.contains("host cores 0.500/s over its ceiling 0.4"), "{why}");
        let loose = Idle {
            interrupts_per_hart: Some(2.0),
            user_ecalls: Some(2.0),
            host_cores: Some(0.5),
            ..Idle::default()
        };
        assert_eq!(window.judge(&loose).unwrap().1, None);
    }

    /// The card's interrupts count as frames in clusters: one taken on two harts within a few traps
    /// is one frame, and so is one taken again further on with no system call from user mode in
    /// between; one after user work is the next. The per-frame ceilings are judged over the span of
    /// exactly `frames` frames, the frame after them ending it, and a window without that frame
    /// fails; a per-frame ceiling needs `frames`.
    #[test]
    fn answer_frames_are_counted_and_judged_over_their_span() {
        let ext = |hart: u32| {
            format!(
                "riscv_cpu_do_interrupt: hart:{hart}, async:1, cause:0000000000000009, epc:0x80200000, tval:0x0, desc=s_external\n"
            )
        };
        let sbi = "riscv_cpu_do_interrupt: hart:0, async:0, cause:0000000000000009, epc:0x80200000, tval:0x0, desc=supervisor_ecall\n";
        let user = "riscv_cpu_do_interrupt: hart:1, async:0, cause:0000000000000008, epc:0x10000, tval:0x0, desc=user_ecall\n";
        // A frame on two harts and taken again after seven SBI calls with no user work between (one
        // frame), then user work and a second frame, then user work and a third.
        let log = format!(
            "{LOG}{}{}{}{}{}{user}{}{}{sbi}{user}{}{}",
            ext(0),
            ext(1),
            sbi.repeat(7),
            ext(3),
            user,
            sbi.repeat(7),
            ext(2),
            sbi.repeat(7),
            ext(1)
        );
        let counts = Counts::parse(&log);
        assert_eq!(counts.frames, 3);
        // From the first frame's first trap to the third's: 4 interrupts, 22 SBI calls, 3 user calls.
        assert_eq!(counts.span(2), Some((4, 22, 3)));
        assert_eq!(counts.span(3), None);
        let (window, _dir) = window("frames", 4, &log, Duration::from_millis(500));
        let span = Idle { frames: Some(2), ..Idle::default() };
        let (summary, over) = window.judge(&span).unwrap();
        assert!(
            summary.ends_with(
                "; 3 answer frames, per frame over the first 2 2.0 interrupts, 11.0 SBI calls, 1.5 user ecalls"
            ),
            "{summary}"
        );
        assert_eq!(over, None);
        let tight = Idle {
            frames: Some(2),
            interrupts_per_frame: Some(1.9),
            sbi_calls_per_frame: Some(10.9),
            user_ecalls_per_frame: Some(1.4),
            ..Idle::default()
        };
        let why = window.judge(&tight).unwrap().1.unwrap();
        assert!(why.contains("idle: interrupts 2.0 a frame over its ceiling 1.9"), "{why}");
        assert!(why.contains("idle: SBI calls 11.0 a frame over its ceiling 10.9"), "{why}");
        assert!(why.contains("idle: user ecalls 1.5 a frame over its ceiling 1.4"), "{why}");
        let loose = Idle {
            frames: Some(2),
            interrupts_per_frame: Some(2.0),
            sbi_calls_per_frame: Some(11.0),
            user_ecalls_per_frame: Some(1.5),
            ..Idle::default()
        };
        assert_eq!(window.judge(&loose).unwrap().1, None);
        // Three frames cannot span three: the frame after them is missing.
        let short = Idle { frames: Some(3), ..loose };
        let why = window.judge(&short).unwrap().1.unwrap();
        assert!(why.contains("idle: 3 answer frames, too few to judge 3"), "{why}");
        let unspanned = Idle { user_ecalls_per_frame: Some(9.0), ..Idle::default() };
        assert!(
            window
                .judge(&unspanned)
                .unwrap_err()
                .to_string()
                .contains("a per-frame ceiling without `frames`")
        );
    }

    /// A window that measured nothing is an error, never a pass under its ceilings: an empty or
    /// missing log, no timer interrupt on any hart, no user system call, no host CPU time. A hart
    /// with none while another took them is a window measured (the kernel arms a timeout whose
    /// payer runs nowhere on one hart, so an idle hart may take none).
    #[test]
    fn a_window_that_measured_nothing_fails() {
        let half = Duration::from_millis(500);
        let nothing = |(w, _dir): (Window, Dir)| w.judge(&Idle::default()).unwrap_err().to_string();
        assert!(nothing(window("empty", 2, "", half)).contains("the log is empty"));
        let no_timer: String =
            LOG.lines().filter(|l| !l.contains("s_timer")).map(|l| format!("{l}\n")).collect();
        assert!(nothing(window("timer", 2, &no_timer, half)).contains("no hart took a timer interrupt"));
        assert!(window("hart", 3, LOG, half).0.judge(&Idle::default()).is_ok(), "hart 2 idle: measured");
        let no_ecall: String =
            LOG.lines().filter(|l| !l.contains("user_ecall")).map(|l| format!("{l}\n")).collect();
        assert!(nothing(window("ecall", 2, &no_ecall, half)).contains("no system call from user mode"));
        assert!(nothing(window("cpu", 2, LOG, Duration::ZERO)).contains("QEMU took no host CPU time"));
        let (mut missing, dir) = window("missing", 2, LOG, half);
        missing.log = missing.log.with_extension("absent");
        assert!(nothing((missing, dir)).contains("no log at"));
        let why = nothing(window("all", 2, "", Duration::ZERO));
        assert!(why.starts_with("the idle window measured nothing: "), "{why}");
    }

    #[test]
    fn the_monitor_is_reached_and_left_by_the_mux_escape() {
        let mut typed = Vec::new();
        monitor(&mut typed, &["log none"]).unwrap();
        assert_eq!(typed, b"\x01clog none\n\x01c");
    }
}
