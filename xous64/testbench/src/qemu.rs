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
    Pass,
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

pub fn run(machine: &Machine, boot: &Boot, smp: u32, loader: &Path, bundle: &Path, log: &Path) -> Result<Verdict> {
    let compile = |patterns: &mut dyn Iterator<Item = &str>| -> Result<Vec<Regex>> {
        patterns.map(|p| Regex::new(p).with_context(|| format!("bad regular expression {p:?}"))).collect()
    };
    let expect = compile(&mut boot.expect.iter().map(String::as_str))?;
    let forbid = compile(&mut boot.forbid.iter().map(String::as_str).chain(ALWAYS_FORBIDDEN.iter().copied()))?;
    let mut inputs = boot
        .input
        .iter()
        .map(|i| Ok((Regex::new(&i.after)?, i.send.as_str())))
        .collect::<Result<Vec<_>>>()?;

    let mut qemu = Command::new(machine.qemu);
    qemu.args(machine.qemu_args)
        .args(["-smp", &smp.to_string()])
        .args(["-display", "none", "-monitor", "none", "-serial", "stdio"])
        .arg("-kernel")
        .arg(loader)
        .arg("-initrd")
        .arg(bundle)
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
    Ok(Verdict::Pass)
}
