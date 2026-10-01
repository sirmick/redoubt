//! `redoubt-sshd-host`: `sshd`'s core on the build host, for the bench (servers/sshd.md, "The core
//! and its platforms"). `ssh` starts it as its `ProxyCommand`, one connection per process:
//!
//! ```text
//! redoubt-sshd-host --host-key tests/keys/loopback-host --login alice=tests/keys/alice.pub \
//!     --log server.log
//! ```
//!
//! - **Transport:** standard input and output. The log file gets the server's own lines.
//! - **Signer:** `keyd`'s own server code in this process, given the host key in `keyd`'s argument form,
//!   `name,ssh_host,seed`, and asked through its protocol as `sshd` asks it on the box. It shares this
//!   process's memory, so this tests the protocol and the login flow, not key separation.
//! - **Logins:** a fixed table. Principal `P` logs in with its `--login` key, as `P` (no labels) or `P+L`
//!   (labels `{P-L}`); anything else is refused.
//! - **Console:** a scripted line console, no shell ([`console`]).

mod console;
mod keyfile;

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use console::Console;
use redoubt_keyd::keys::Keys;
use redoubt_keyd::server::{BUDGET, COST, KeyServer, answer_with, limits};
use redoubt_rt::abi::{Error, Handle, Labels, ReceivedHandles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::minted::Minter;
use redoubt_rt::wire::proto::keyd::{Holds, Message, Reply, SignSshExchange};
use redoubt_rt::wire::typed::opcode;
use redoubt_sshd::{
    Connection, ExchangeTranscript, Login, Platform, Progress, PublicKey, Refusal, Refused, Signature,
};

/// The largest packet `sunset` takes.
const BUF: usize = 35_000;
/// The host key's root badge: the first key in `keyd`'s arguments (servers/keyd.md).
const HOST_BADGE: u64 = 1;
/// How long the loop waits for input before it runs the session again (a console may sleep).
const TICK: Duration = Duration::from_millis(10);

/// The server's log: one line per event, for a case's `server_log`.
#[derive(Clone)]
pub struct Log(Rc<RefCell<File>>);

impl Log {
    /// Appends to `path`: every session of a case runs its own server, and all log to one file.
    fn append(path: &str) -> Result<Log> {
        let file =
            OpenOptions::new().create(true).append(true).open(path).with_context(|| path.to_string())?;
        Ok(Log(Rc::new(RefCell::new(file))))
    }

    pub fn line(&self, text: &str) {
        // One write per line, which an appending file keeps whole: `writeln!` writes the text and
        // the newline apart, and another server's line could land between them. A lost log line
        // fails the case that looks for it; the connection goes on.
        let _ = self.0.borrow_mut().write_all(format!("{text}\n").as_bytes());
    }
}

/// The host platform: `keyd` in process, the login table, and the console.
struct Host {
    keyd: KeyServer,
    logins: Vec<(String, PublicKey)>,
    log: Log,
}

/// `keyd` needs a kernel only to grant, which `sshd` never asks.
struct NoKernel;

impl Minter for NoKernel {
    fn mint(&mut self, _: std::num::NonZeroU64) -> Result<Handle, Error> { Err(Error::NotPermitted) }

    fn random(&mut self) -> Result<u64, Error> { Err(Error::NotPermitted) }
}

impl Host {
    /// One request to `keyd` through its protocol, with the host key's badge.
    fn ask(&mut self, request: &Message<'_>) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; 64 * 1024];
        let words = request.encode(&mut buf).ok()?;
        let caller = Caller { badge: HOST_BADGE, account: 0, labels: Labels::from_slice(&[]).ok()? };
        let outcome =
            answer_with(&mut self.keyd, &caller, &words, &ReceivedHandles::new(), &mut buf, &mut NoKernel);
        match Reply::decode(opcode(&words).ok()?, &outcome.words, &buf, 0) {
            Ok(Ok(Reply::SignSshExchange(r))) => Some(r.signature.to_vec()),
            Ok(Ok(Reply::Holds(r))) => Some(vec![r.held as u8]),
            _ => None,
        }
    }
}

impl Platform for Host {
    type Session = Console;

    fn sign_exchange(&mut self, t: &ExchangeTranscript<'_>) -> Result<Signature, Refused> {
        let m = SignSshExchange {
            v_c: t.v_c,
            v_s: t.v_s,
            i_c: t.i_c,
            i_s: t.i_s,
            q_c: t.q_c,
            q_s: t.q_s,
            k: t.k,
        };
        let signature = self.ask(&Message::SignSshExchange(m)).and_then(|s| s.try_into().ok());
        self.log.line(if signature.is_some() {
            "keyd signed the exchange"
        } else {
            "keyd refused the exchange"
        });
        signature.ok_or(Refused)
    }

    fn holds(&mut self, key: &PublicKey) -> Result<bool, Refused> {
        match self.ask(&Message::Holds(Holds { key })).as_deref() {
            Some([0]) => Ok(false),
            Some([1]) => {
                self.log.line("keyd holds the key offered");
                Ok(true)
            }
            _ => Err(Refused),
        }
    }

    fn login(&mut self, who: &Login<'_>, key: &PublicKey) -> Result<Console, Refused> {
        let name = match who.label {
            Some(label) => format!("{}+{label}", who.principal),
            None => who.principal.to_string(),
        };
        if !self.logins.iter().any(|(p, k)| p == who.principal && k == key) {
            self.log.line(&format!("login {name}: refused"));
            return Err(Refused);
        }
        let labels = match who.label {
            Some(label) => format!("{{{}-{label}}}", who.principal),
            None => "{}".into(),
        };
        self.log.line(&format!("login {name}: labels {labels}"));
        Ok(Console::new(self.log.clone(), name, labels, who.label.is_some()))
    }

    fn end(&mut self, session: Console) { self.log.line(&format!("session {} ended", session.name())) }

    fn refused(&mut self, request: Refusal) { self.log.line(&format!("request {} refused", request.name())) }
}

fn main() -> Result<()> {
    let (mut host_key, mut logins, mut log) = (None, Vec::new(), None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().with_context(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--host-key" => host_key = Some(value),
            "--login" => {
                let (principal, path) = value.split_once('=').context("--login PRINCIPAL=KEY.pub")?;
                let key = keyfile::public(&std::fs::read_to_string(path).with_context(|| path.to_string())?)?;
                logins.push((principal.to_string(), key));
            }
            "--log" => log = Some(Log::append(&value)?),
            _ => bail!("unknown argument {arg}"),
        }
    }
    let host_key = host_key.context("--host-key is required")?;
    let seed = keyfile::seed(&std::fs::read_to_string(&host_key).with_context(|| host_key.clone())?)?;
    let keys = Keys::from_args([keyfile::host_key_arg("host", &seed)].iter().map(String::as_str))
        .map_err(|e| anyhow::anyhow!("keyd refused the host key: {e:?}"))?;
    let public = *keys.get(0).context("no host key")?.public();
    let keyd =
        KeyServer::new(keys, limits(16), &COST, BUDGET, 1).map_err(|_| anyhow::anyhow!("keyd's limits"))?;
    let log = log.context("--log is required")?;
    let mut host = Host { keyd, logins, log: log.clone() };
    let result = serve(&mut host, &public);
    if let Err(e) = &result {
        log.line(&format!("connection failed: {e:#}"));
    }
    result
}

/// Serves one connection on standard input and output until it closes.
fn serve(host: &mut Host, public: &PublicKey) -> Result<()> {
    let (mut inbuf, mut outbuf) = (vec![0; BUF], vec![0; BUF]);
    let mut conn = Box::new(Connection::new(&mut inbuf, &mut outbuf, public));
    // Standard input is read on its own thread, so the loop can run the console while it waits.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut stdin = std::io::stdin().lock();
        while let Ok(n @ 1..) = stdin.read(&mut buf) {
            if tx.send(buf[..n].to_vec()).is_err() {
                return;
            }
        }
    });
    let mut stdout = std::io::stdout().lock();
    let mut pending = Vec::new();
    loop {
        match rx.recv_timeout(TICK) {
            Ok(bytes) => pending.extend_from_slice(&bytes),
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) if pending.is_empty() => conn.close_input(),
            Err(mpsc::RecvTimeoutError::Disconnected) => (),
        }
        loop {
            let n = conn.input(&pending).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            pending.drain(..n);
            let progress = conn.progress(host).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let out = conn.output_buf();
            let written = out.len();
            stdout.write_all(out)?;
            stdout.flush()?;
            conn.consume_output(written);
            match progress {
                Progress::Busy => continue,
                Progress::Idle if n > 0 && !pending.is_empty() => continue,
                Progress::Idle => break,
                Progress::Closed => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Log;

    /// Two servers appending to one log, as a case's sessions do: every line arrives whole, none
    /// split and none joined to another.
    #[test]
    fn servers_sharing_a_log_keep_its_lines_whole() {
        const LINES: usize = 2000;
        let path = std::env::temp_dir().join(format!("redoubt-sshd-host-log-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path = path.to_str().unwrap().to_string();
        std::thread::scope(|scope| {
            for server in ["alice+one", "alice+two"] {
                let path = &path;
                scope.spawn(move || {
                    let log = Log::append(path).unwrap();
                    for i in 0..LINES {
                        log.line(&format!("login {server}: line {i} of a session long enough to split"));
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2 * LINES, "a line was split or joined");
        for server in ["alice+one", "alice+two"] {
            for i in 0..LINES {
                let want = format!("login {server}: line {i} of a session long enough to split");
                assert!(lines.contains(&want.as_str()), "missing whole: {want}");
            }
        }
    }
}
