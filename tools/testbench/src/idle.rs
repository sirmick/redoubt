//! A boot at rest, measured from outside (`[idle]`, docs/testbench.md "The machine at rest"):
//! over a session's `idle` step, the interrupts and exceptions each hart takes, from QEMU's own
//! log, and QEMU's host CPU time. The log is turned on for that window alone, through the monitor
//! that `-nographic` multiplexes on the console (Ctrl-A c), so the command line booted is
//! launch's, unchanged; outside the window every system call would be logged.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use crate::case::Idle;

/// QEMU's mux escape, then `c`: the console's input goes to the monitor, or back.
const SWITCH: &[u8] = b"\x01c";

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
}

impl Window {
    /// The window of QEMU `pid`, a guest of `harts` harts, whose log goes to `log`.
    pub fn new(pid: u32, harts: u32, log: PathBuf) -> Result<Window> {
        let getconf =
            std::process::Command::new("getconf").arg("CLK_TCK").output().context("running getconf")?;
        let ticks: u64 =
            String::from_utf8_lossy(&getconf.stdout).trim().parse().context("getconf CLK_TCK")?;
        ensure!(ticks > 0, "no clock tick rate");
        Ok(Window { pid, harts, log, ticks, start: None, end: None })
    }

    /// Turn QEMU's interrupt log on, through the monitor on `console`, and take its CPU time.
    pub fn start(&mut self, console: &mut dyn Write) -> Result<()> {
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

    /// The window's measures as one line, and why it fails `ceilings`, if it does. A window that
    /// measured nothing is an error, never a pass: no log, a hart with no timer interrupt, no
    /// system call from user mode, or no host CPU time.
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
        let summary = counts.summary(seconds, cores);
        // The floors. Every hart of every window measured so far took timer interrupts (0.6 to
        // 1.2 a second, nine windows of 60 s on both widths), so a hart with none was not logged.
        let mut nothing = Vec::new();
        if text.is_empty() {
            nothing.push("the log is empty".to_string());
        }
        for hart in 0..u64::from(self.harts) {
            if counts.interrupts.get(&hart).and_then(|by| by.get("s_timer")).is_none_or(|n| *n == 0) {
                nothing.push(format!("hart {hart} took no timer interrupt"));
            }
        }
        if counts.exceptions.get("user_ecall").is_none_or(|n| *n == 0) {
            nothing.push("no system call from user mode".to_string());
        }
        if cores <= 0.0 {
            nothing.push("QEMU took no host CPU time".to_string());
        }
        ensure!(nothing.is_empty(), "the idle window measured nothing: {} ({summary})", nothing.join("; "));
        let busiest = counts.interrupts.values().map(|by| by.values().sum::<u64>()).max().unwrap_or(0);
        let ecalls = counts.exceptions.get("user_ecall").copied().unwrap_or(0);
        let over = [
            ("interrupts per hart", busiest as f64 / seconds, ceilings.interrupts_per_hart),
            ("user ecalls", ecalls as f64 / seconds, ceilings.user_ecalls),
            ("host cores", cores, ceilings.host_cores),
        ]
        .into_iter()
        .filter_map(|(what, rate, ceiling)| {
            ceiling.filter(|c| rate > *c).map(|c| format!("idle: {what} {rate:.3}/s over its ceiling {c}"))
        })
        .collect::<Vec<_>>();
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
}

impl Counts {
    fn parse(log: &str) -> Counts {
        let mut counts = Counts::default();
        for line in log.lines().filter(|l| l.starts_with("riscv_cpu_do_interrupt:")) {
            let field = |key: &str| line.split([',', ' ']).find_map(|part| part.strip_prefix(key));
            let (Some(hart), Some(kind), Some(name)) = (field("hart:"), field("async:"), field("desc="))
            else {
                continue;
            };
            let Ok(hart) = hart.parse() else { continue };
            let by_name = match kind {
                "1" => counts.interrupts.entry(hart).or_default(),
                _ => &mut counts.exceptions,
            };
            *by_name.entry(name.to_string()).or_default() += 1;
        }
        counts
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
        };
        (window, Dir(dir))
    }

    /// Each ceiling fails the window when its rate is over it; none set, nothing fails.
    #[test]
    fn a_rate_over_its_ceiling_fails() {
        let (window, _dir) = window("ceilings", 2, LOG, Duration::from_millis(500));
        assert_eq!(window.judge(&Idle::default()).unwrap().1, None);
        let tight = Idle { interrupts_per_hart: Some(1.5), user_ecalls: Some(1.0), host_cores: Some(0.4) };
        let why = window.judge(&tight).unwrap().1.unwrap();
        assert!(why.contains("interrupts per hart 2.000/s over its ceiling 1.5"), "{why}");
        assert!(why.contains("user ecalls 2.000/s over its ceiling 1"), "{why}");
        assert!(why.contains("host cores 0.500/s over its ceiling 0.4"), "{why}");
        let loose = Idle { interrupts_per_hart: Some(2.0), user_ecalls: Some(2.0), host_cores: Some(0.5) };
        assert_eq!(window.judge(&loose).unwrap().1, None);
    }

    /// A window that measured nothing is an error, never a pass under its ceilings: an empty or
    /// missing log, a hart with no timer interrupt, no user system call, no host CPU time.
    #[test]
    fn a_window_that_measured_nothing_fails() {
        let half = Duration::from_millis(500);
        let nothing = |(w, _dir): (Window, Dir)| w.judge(&Idle::default()).unwrap_err().to_string();
        assert!(nothing(window("empty", 2, "", half)).contains("the log is empty"));
        assert!(nothing(window("hart", 3, LOG, half)).contains("hart 2 took no timer interrupt"));
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
