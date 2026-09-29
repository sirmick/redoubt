//! The scripted line console a session gets on the host: no shell. Commands, `;` between them:
//! `echo WORDS`, `sleep SECONDS` (at most 10), `tty` (whether the channel has a pty), `labels`
//! (the session's labels), `exit N`. The session ends with `exit`, or with status 0 once the
//! client's input has ended and every command has run. It logs each window change and interrupt,
//! and each 0x03 byte apart from the interrupts the core passes on (`INT`, `break`).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use redoubt_sshd::{Session, Window};

use crate::Log;

/// The longest line kept; the rest of a longer one is dropped.
const MAX_LINE: usize = 1024;

pub struct Console {
    log: Log,
    /// The login's user name, `principal` or `principal+label`.
    name: String,
    /// The session's labels as `{a, b}`.
    labels: String,
    labelled: bool,
    pty: Option<Window>,
    line: Vec<u8>,
    commands: VecDeque<String>,
    asleep_until: Option<Instant>,
    input_ended: bool,
    out: Vec<u8>,
    status: Option<u32>,
}

impl Console {
    pub fn new(log: Log, name: String, labels: String, labelled: bool) -> Self {
        Console {
            log,
            name,
            labels,
            labelled,
            pty: None,
            line: Vec::new(),
            commands: VecDeque::new(),
            asleep_until: None,
            input_ended: false,
            out: Vec::new(),
            status: None,
        }
    }

    /// Runs commands until one sleeps, or none is left.
    fn run(&mut self) {
        while self.status.is_none() {
            if self.asleep_until.is_some_and(|t| Instant::now() < t) {
                return;
            }
            self.asleep_until = None;
            let Some(command) = self.commands.pop_front() else { break };
            let (name, args) = command.trim().split_once(' ').unwrap_or((command.trim(), ""));
            let reply = match name {
                "" => continue,
                "echo" => args.to_string(),
                "tty" => if self.pty.is_some() { "pty" } else { "not a tty" }.into(),
                "labels" => self.labels.clone(),
                "sleep" => match args.parse::<f64>() {
                    Ok(s) if (0.0..=10.0).contains(&s) => {
                        self.asleep_until = Some(Instant::now() + Duration::from_secs_f64(s));
                        continue;
                    }
                    _ => format!("sleep: bad time {args:?}"),
                },
                "exit" => match args.parse::<u32>() {
                    Ok(n) => {
                        self.status = Some(n);
                        continue;
                    }
                    Err(_) if args.is_empty() => {
                        self.status = Some(0);
                        continue;
                    }
                    Err(_) => format!("exit: bad status {args:?}"),
                },
                _ => format!("unknown command {name:?}"),
            };
            self.out.extend_from_slice(reply.as_bytes());
            self.out.push(b'\n');
        }
        if self.input_ended && self.commands.is_empty() && self.asleep_until.is_none() {
            self.status.get_or_insert(0);
        }
    }

    /// The login's user name.
    pub fn name(&self) -> &str { &self.name }
}

impl Session for Console {
    fn labelled(&self) -> bool { self.labelled }

    fn start(&mut self, pty: Option<Window>) {
        self.pty = pty;
        match pty {
            Some(w) => self.log.line(&format!("console started with a pty, {}x{}", w.cols, w.rows)),
            None => self.log.line("console started without a pty"),
        }
    }

    fn input(&mut self, bytes: &[u8]) -> usize {
        for &b in bytes {
            match b {
                0x03 => self.log.line("console interrupt byte"),
                b'\n' | b'\r' => {
                    let line = String::from_utf8_lossy(&self.line).into_owned();
                    self.commands.extend(line.split(';').map(String::from));
                    self.line.clear();
                }
                _ if self.line.len() < MAX_LINE => self.line.push(b),
                _ => (),
            }
        }
        bytes.len()
    }

    fn input_ended(&mut self) {
        self.log.line("console input ended");
        self.input_ended = true;
    }

    fn output(&mut self, buf: &mut [u8]) -> usize {
        self.run();
        let n = buf.len().min(self.out.len());
        buf[..n].copy_from_slice(&self.out[..n]);
        self.out.drain(..n);
        n
    }

    fn window(&mut self, w: Window) {
        self.pty = Some(w);
        self.log.line(&format!("console window {}x{}", w.cols, w.rows));
    }

    fn interrupt(&mut self) { self.log.line("console interrupt") }

    fn ended(&self) -> Option<u32> { self.status.filter(|_| self.out.is_empty()) }
}
