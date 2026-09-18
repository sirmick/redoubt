//! Running one boot under QEMU and judging its console output.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use regex::Regex;

use crate::case::{Boot, ALWAYS_FORBIDDEN};
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
            .arg(self.bundle);
        qemu
    }

    /// Boot with the console on this terminal. Ctrl-A X quits QEMU.
    pub fn run_interactive(&self) -> Result<()> {
        let status = self.qemu().arg("-nographic").status().with_context(|| format!("starting {}", self.machine.qemu))?;
        anyhow::ensure!(status.success(), "{} exited with {status}", self.machine.qemu);
        Ok(())
    }
}

pub fn run(image: &Image, boot: &Boot, log: &Path) -> Result<Verdict> {
    let machine = image.machine;
    let compile = |patterns: &mut dyn Iterator<Item = &str>| -> Result<Vec<Regex>> {
        patterns.map(|p| Regex::new(p).with_context(|| format!("bad regular expression {p:?}"))).collect()
    };
    let expect = compile(&mut boot.expect.iter().map(String::as_str))?;
    let defaults = if boot.default_forbid { ALWAYS_FORBIDDEN } else { &[] };
    let forbid = compile(&mut boot.forbid.iter().map(String::as_str).chain(defaults.iter().copied()))?;
    let capture = compile(&mut boot.distinct_across_boots.iter().map(String::as_str))?;
    let mut captured: Vec<Option<String>> = vec![None; capture.len()];
    let mut inputs = boot
        .input
        .iter()
        .map(|i| Ok((Regex::new(&i.after)?, i.send.as_str())))
        .collect::<Result<Vec<_>>>()?;

    let mut qemu = image.qemu();
    qemu.args(["-display", "none", "-monitor", "none", "-serial", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut guest = Guest(qemu.spawn().with_context(|| format!("starting {}", machine.qemu))?);
    let mut stdin = guest.0.stdin.take().unwrap();
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

    let mut log = std::fs::File::create(log).with_context(|| format!("creating {}", log.display()))?;
    let deadline = Instant::now() + Duration::from_secs(boot.timeout_secs);
    let mut next = 0;
    while next < expect.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let line = match rx.recv_timeout(remaining) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Ok(Verdict::Fail(format!("timed out waiting for /{}/", expect[next])));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(Verdict::Fail(format!("guest exited while waiting for /{}/", expect[next])));
            }
        };
        writeln!(log, "{line}")?;

        if let Some(pattern) = forbid.iter().find(|p| p.is_match(&line)) {
            return Ok(Verdict::Fail(format!("forbidden output /{pattern}/: {line}")));
        }
        for (pattern, slot) in capture.iter().zip(captured.iter_mut()).filter(|(_, slot)| slot.is_none()) {
            *slot = pattern.captures(&line).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
        }
        if expect[next].is_match(&line) {
            next += 1;
        }
        let mut pending = Vec::new();
        for (after, send) in inputs.drain(..) {
            if after.is_match(&line) {
                stdin.write_all(send.as_bytes())?;
                stdin.flush()?;
            } else {
                pending.push((after, send));
            }
        }
        inputs = pending;
    }
    Ok(Verdict::Pass(captured))
}
