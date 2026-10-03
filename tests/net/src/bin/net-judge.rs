//! `net-judge`: the reporter of the net cases under `init` (docs/testbench.md, "The servers'
//! cases under `init`"). A `servers` entry receiving on `judge`, where each client of its case
//! reports on the badge its entry is handed (`redoubt_net_client::badge`). It waits until `ipd`
//! has a link, through a badge of its own at `ipd`, gives each client its turn, and prints
//! `judge TEST PASSED` only when every check it owns passed. A client's report is information,
//! never a verdict on the network: what reached the network is judged by the bench's peers,
//! dials and capture, from outside the guest.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_init_programs::Out;
use redoubt_net_client::{IPD, JUDGE, REPORT, badge, code, event};
use redoubt_rt::abi::{FOREVER, Handles};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Event, Request};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::net_ctl;

redoubt_rt::entry!(run);

/// How long `ipd` may take to have a link, in µs.
const LINK_WAIT: u64 = 20_000_000;

/// The case it judges, its one argument.
#[derive(Clone, Copy)]
enum Case {
    /// `case=tcp` (`net-tcp`), or `case=twice` (the peer self-checks, two rounds): a client
    /// round-trips bytes through the peer, then a listener echoes the bench's dial.
    Tcp { rounds: u32 },
}

fn parse<'a>(mut args: impl Iterator<Item = &'a str>) -> Option<Case> {
    let case = match args.next()? {
        "case=tcp" => Case::Tcp { rounds: 1 },
        "case=twice" => Case::Tcp { rounds: 2 },
        _ => return None,
    };
    args.next().is_none().then_some(case)
}

fn run(startup: &Startup) -> u32 {
    let out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut judge = Judge { out, at: None, backlog: Vec::new(), waiting: Vec::new(), ok: true };
    if let Err(why) = judge.judge(startup) {
        judge.fail(&why);
    }
    judge.say(if judge.ok { "TEST PASSED" } else { "TEST FAILED" });
    redoubt_init_programs::park()
}

struct Judge {
    out: Out,
    /// Where the clients report.
    at: Option<Endpoint>,
    /// Reports not yet waited for: (badge, event, value).
    backlog: Vec<(u64, u64, u64)>,
    /// Clients waiting for their turn: their badge and their held `START`.
    waiting: Vec<(u64, Request)>,
    ok: bool,
}

impl Judge {
    /// One line, `judge ` first. A line the console refused leaves no verdict, which fails the
    /// case.
    fn say(&mut self, line: &str) { let _ = self.out.say(&format!("judge {line}\n")); }

    fn fail(&mut self, why: &str) {
        self.ok = false;
        self.say(&format!("FAIL: {why}"));
    }

    fn check(&mut self, passed: bool, what: &str) {
        if passed {
            self.say(&format!("ok: {what}"));
        } else {
            self.fail(what);
        }
    }

    fn judge(&mut self, startup: &Startup) -> Result<(), String> {
        let case = parse(startup.args()).ok_or("bad arguments")?;
        self.at = Some(Endpoint::from_handle(startup.handle(JUDGE).ok_or("no judge endpoint")?));
        let probe = startup.handle(IPD).ok_or("no badge at ipd")?;
        self.wait_for_link(Endpoint::from_handle(probe))?;
        match case {
            Case::Tcp { rounds } => self.tcp(rounds),
        }
    }

    /// `listen` answers `unreachable` until `ipd` has asked `netd` for the MAC; the judge listens
    /// on its probe port until it does not, then closes that socket. Nothing goes on the wire.
    fn wait_for_link(&mut self, ipd: Endpoint) -> Result<(), String> {
        let nine = Connection::new(ipd);
        let mut lend = Lend::new(1).map_err(|e| format!("probe: {e:?}"))?;
        nine.attach(&mut lend, 0, "").map_err(|e| format!("probe attach: {e:?}"))?;
        nine.walk(&mut lend, 0, 1, "tcp/clone").map_err(|e| format!("probe clone: {e:?}"))?;
        nine.open(&mut lend, 1, mode::OREAD).map_err(|e| format!("probe clone: {e:?}"))?;
        let mut n = [0u8; 4];
        nine.read(&mut lend, 1, 0, &mut n).map_err(|e| format!("probe clone read: {e:?}"))?;
        nine.clunk(&mut lend, 1).map_err(|e| format!("probe clunk: {e:?}"))?;
        let ctl = format!("tcp/{}/ctl", u32::from_le_bytes(n));
        nine.walk(&mut lend, 0, 2, &ctl).map_err(|e| format!("probe ctl: {e:?}"))?;
        nine.open(&mut lend, 2, mode::ORDWR).map_err(|e| format!("probe ctl: {e:?}"))?;
        let listen =
            encode(net_ctl::Message::Listen(net_ctl::Listen { port: badge::PROBE_PORT, backlog: 1 }));
        let started = now();
        let mut tries = 0u32;
        loop {
            tries += 1;
            if nine.write(&mut lend, 2, 0, &listen).is_ok() {
                break;
            }
            if now().saturating_sub(started) > LINK_WAIT {
                return Err(format!("ipd had no link after {tries} tries"));
            }
        }
        let close = encode(net_ctl::Message::Close(net_ctl::Close {}));
        nine.write(&mut lend, 2, 0, &close).map_err(|e| format!("closing the probe: {e:?}"))?;
        nine.clunk(&mut lend, 2).map_err(|e| format!("probe clunk: {e:?}"))?;
        self.say(&format!("ipd has a link ({tries} probes)"));
        Ok(())
    }

    // ---- the clients ----

    /// Gives the client on `badge` its turn, once it has asked for one.
    fn turn(&mut self, badge: u64) -> Result<(), String> {
        loop {
            if let Some(i) = self.waiting.iter().position(|(b, _)| *b == badge) {
                let (_, start) = self.waiting.remove(i);
                return answer(start);
            }
            self.take()?;
        }
    }

    /// Waits for report `what` from the client on `badge`, and returns its value. A client whose
    /// role ends first fails the wait, saying how it ended.
    fn report(&mut self, badge: u64, what: u64) -> Result<u64, String> {
        loop {
            if let Some(i) = self.backlog.iter().position(|(b, w, _)| *b == badge && *w == what) {
                return Ok(self.backlog.remove(i).2);
            }
            if let Some(&(_, _, outcome)) =
                self.backlog.iter().find(|(b, w, _)| *b == badge && *w == event::DONE)
            {
                return Err(format!("client {badge} ended with outcome {outcome} before its report {what}"));
            }
            self.take()?;
        }
    }

    /// The next report: a `START` is held for [`Judge::turn`]; every other one is answered at
    /// once and kept.
    fn take(&mut self) -> Result<(), String> {
        let at = self.at.as_ref().ok_or("no judge endpoint")?;
        loop {
            match at.receive(FOREVER, 0) {
                Ok(Event::Call(request)) => {
                    let (words, badge) = (request.words, request.caller.badge);
                    if words[0] != REPORT {
                        answer(request)?;
                        continue;
                    }
                    if words[1] == event::START {
                        self.waiting.push((badge, request));
                    } else {
                        answer(request)?;
                        self.backlog.push((badge, words[1], words[2]));
                    }
                    return Ok(());
                }
                Ok(Event::Send(delivery)) => {
                    for handle in delivery.handles.as_slice().iter().flatten() {
                        let _ = redoubt_rt::handle::close(*handle);
                    }
                }
                Ok(_) => {}
                Err(e) => return Err(format!("receiving reports: {e:?}")),
            }
        }
    }

    // ---- the cases ----

    /// A client round-trips bytes through the echo peer `rounds` times; then a listener with a
    /// backlog of 2 echoes the bench's dial.
    fn tcp(&mut self, rounds: u32) -> Result<(), String> {
        self.turn(badge::ECHO)?;
        let outcome = self.report(badge::ECHO, event::DONE)?;
        let passed = outcome == u64::from(code::OK);
        self.check(passed, &format!("echo through 10.0.9.100:7, {rounds} round(s): outcome {outcome}"));
        self.turn(badge::LISTEN)?;
        self.report(badge::LISTEN, event::READY)?;
        self.say("listening on 8000 with a backlog of 2");
        let accepted = self.report(badge::LISTEN, event::ACCEPTED)?;
        self.check(accepted == 1, &format!("the listener accepted and echoed the bench's dial ({accepted})"));
        Ok(())
    }
}

/// Answers a report with nothing.
fn answer(request: Request) -> Result<(), String> {
    finish(request, &Outcome { words: [0; 4], send: Handles::new(), close: Handles::new() })
        .map(|_| ())
        .map_err(|e| format!("answering a report: {e:?}"))
}

fn encode(message: net_ctl::Message<'_>) -> Vec<u8> {
    let mut out = [0u8; 16];
    let n = message.encode_file(&mut out).unwrap_or(0);
    out[..n].to_vec()
}

fn now() -> u64 { redoubt_rt::handle::time_now().unwrap_or(u64::MAX) }
