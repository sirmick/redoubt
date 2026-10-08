//! The core against a `sunset` client, in process, with the bytes between them moved by hand, and a
//! platform that records what the core asked of it. Each verdict is what the platform saw, or what
//! the client received, never what the client meant to do.

use std::cell::RefCell;
use std::rc::Rc;

use redoubt_keyd::ssh::{Transcript, exchange_hash};
use redoubt_sshd::{
    Connection, ExchangeTranscript, Login, Platform, Progress, PublicKey, Refusal, Refused, Session,
    Signature, Window,
};
use sunset::ed25519_compact::{KeyPair, Seed};
use sunset::event::{CliEvent, Event};
use sunset::packets::WinChange;
use sunset::{ChanData, ChanHandle, CliServ, CliSessionExit, Client, Pty, Runner, SignKey};

/// The largest packet `sunset` takes.
const BUF: usize = 35_000;

fn host() -> KeyPair { KeyPair::from_seed(Seed::new([7; 32])) }
fn alice() -> KeyPair { KeyPair::from_seed(Seed::new([9; 32])) }
/// A key the stand-in for `keyd` holds.
fn held() -> KeyPair { KeyPair::from_seed(Seed::new([11; 32])) }

/// What the platform was asked and told.
#[derive(Default)]
struct Log {
    holds: usize,
    /// Each login the steward was asked: principal, label and context.
    logins: Vec<(String, Option<String>, Option<String>)>,
    started: Option<Option<Window>>,
    input: Vec<u8>,
    windows: Vec<Window>,
    interrupts: usize,
    ended: usize,
    refused: Vec<Refusal>,
}

/// The stand-in platform: `keyd` holds [`held`] and signs with [`host`] unless told not to; the
/// steward logs in `alice` with [`alice`], with or without a label.
struct Fake {
    log: Rc<RefCell<Log>>,
    sign: bool,
}

impl Platform for Fake {
    type Session = Console;

    fn sign_exchange(&mut self, t: &ExchangeTranscript<'_>) -> Result<Signature, Refused> {
        if !self.sign {
            return Err(Refused);
        }
        let t = Transcript { v_c: t.v_c, v_s: t.v_s, i_c: t.i_c, i_s: t.i_s, q_c: t.q_c, q_s: t.q_s, k: t.k };
        let pair = host();
        Ok(*pair.sk.sign(exchange_hash(&t, &pair.pk).unwrap(), None))
    }

    fn holds(&mut self, key: &PublicKey) -> Result<bool, Refused> {
        self.log.borrow_mut().holds += 1;
        Ok(*key == *held().pk)
    }

    fn login(&mut self, who: &Login<'_>, key: &PublicKey) -> Result<Console, Refused> {
        self.log.borrow_mut().logins.push((
            who.principal.into(),
            who.label.map(Into::into),
            who.context.map(Into::into),
        ));
        if who.principal != "alice" || *key != *alice().pk {
            return Err(Refused);
        }
        Ok(Console { log: self.log.clone(), labelled: who.label.is_some(), out: Vec::new(), status: None })
    }

    fn end(&mut self, _: Console) { self.log.borrow_mut().ended += 1 }

    fn refused(&mut self, request: Refusal) { self.log.borrow_mut().refused.push(request) }
}

/// Echoes its input; `!` ends it with status 7.
struct Console {
    log: Rc<RefCell<Log>>,
    labelled: bool,
    out: Vec<u8>,
    status: Option<u32>,
}

impl Session for Console {
    fn labelled(&self) -> bool { self.labelled }

    fn start(&mut self, pty: Option<Window>) { self.log.borrow_mut().started = Some(pty) }

    fn input(&mut self, bytes: &[u8]) -> usize {
        for &b in bytes {
            if b == b'!' {
                self.status = Some(7);
            } else if self.status.is_none() {
                self.out.push(b);
            }
        }
        self.log.borrow_mut().input.extend_from_slice(bytes);
        bytes.len()
    }

    fn input_ended(&mut self) { self.status.get_or_insert(0); }

    fn output(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.out.len());
        buf[..n].copy_from_slice(&self.out[..n]);
        self.out.drain(..n);
        n
    }

    fn window(&mut self, w: Window) { self.log.borrow_mut().windows.push(w) }

    fn interrupt(&mut self) { self.log.borrow_mut().interrupts += 1 }

    fn ended(&self) -> Option<u32> { self.status.filter(|_| self.out.is_empty()) }
}

/// What the client asks for once it has a session channel.
#[derive(Clone, Copy)]
enum Request {
    Shell,
    Exec,
    Subsystem,
}

/// The client's side: who it is, what it asks for, and what it sends once the request is made.
struct Script {
    user: &'static str,
    key: KeyPair,
    pty: Option<(u32, u32)>,
    request: Request,
    windows: &'static [(u32, u32)],
    signals: &'static [&'static str],
    brk: bool,
    data: &'static [u8],
    second_channel: bool,
}

impl Default for Script {
    fn default() -> Self {
        Script {
            user: "alice",
            key: alice(),
            pty: Some((80, 24)),
            request: Request::Shell,
            windows: &[],
            signals: &[],
            brk: false,
            data: b"",
            second_channel: false,
        }
    }
}

/// What the client saw.
#[derive(Default)]
struct Seen {
    authenticated: bool,
    output: Vec<u8>,
    exit: Option<u32>,
    closed: bool,
    sessions_opened: usize,
}

/// The client's state as it goes.
#[derive(Default)]
struct Cli {
    offered: bool,
    open: bool,
    chan: Option<ChanHandle>,
    requested: bool,
    sent: bool,
}

fn client(r: &mut Runner<Client>, s: &Script, c: &mut Cli, seen: &mut Seen) {
    loop {
        let ev = match r.progress() {
            Ok(Event::Cli(ev)) => ev,
            Ok(Event::None) | Err(_) => break,
            Ok(_) => continue,
        };
        match ev {
            CliEvent::Hostkey(h) => {
                assert_eq!(
                    SignKey::from_agent_pubkey(&h.hostkey().unwrap()).unwrap(),
                    SignKey::AgentEd25519(host().pk)
                );
                h.accept().unwrap()
            }
            CliEvent::Username(u) => u.username(s.user).unwrap(),
            CliEvent::Pubkey(p) if !c.offered => {
                c.offered = true;
                p.pubkey(SignKey::Ed25519(s.key.clone())).unwrap()
            }
            // A refused client has no other method, and gives up.
            CliEvent::Pubkey(p) => drop(p.skip()),
            CliEvent::Password(p) => drop(p.skip()),
            CliEvent::Authenticated => {
                seen.authenticated = true;
                c.open = true;
            }
            CliEvent::SessionOpened(mut o) => {
                seen.sessions_opened += 1;
                if !c.requested {
                    c.requested = true;
                    if let Some((cols, rows)) = s.pty {
                        let term = heapless::String::try_from("xterm").unwrap();
                        o.pty(Pty { term, cols, rows, width: 0, height: 0, modes: heapless::Vec::new() })
                            .unwrap();
                    }
                    match s.request {
                        Request::Shell => o.shell().unwrap(),
                        Request::Exec => o.exec("echo hi").unwrap(),
                        Request::Subsystem => o.subsystem("sftp").unwrap(),
                    }
                }
            }
            CliEvent::SessionExit(CliSessionExit::Status(n)) => seen.exit = Some(n),
            other => panic!("unexpected {other:?}"),
        }
    }
    if c.open {
        c.open = false;
        c.chan = Some(r.open_client_session().unwrap());
        if s.second_channel {
            // Kept open only in `sunset`'s table; the verdict is whether the server confirms it.
            std::mem::forget(r.open_client_session().unwrap());
        }
    }
    let Some(chan) = &c.chan else { return };
    let mut buf = [0; 1024];
    while let Ok(n @ 1..) = r.read_channel(chan, ChanData::Normal, &mut buf) {
        seen.output.extend_from_slice(&buf[..n]);
    }
    if r.is_channel_closed(chan) {
        seen.closed = true;
    }
    if c.requested && !c.sent && seen.sessions_opened > 0 {
        c.sent = true;
        for &(cols, rows) in s.windows {
            r.term_window_change(chan, &WinChange { cols, rows, width: 0, height: 0 }).unwrap();
        }
        for sig in s.signals {
            r.term_signal(chan, sig).unwrap();
        }
        if s.brk {
            r.term_break(chan, 0).unwrap();
        }
        if !s.data.is_empty() {
            assert_eq!(r.write_channel(chan, ChanData::Normal, s.data).unwrap(), s.data.len());
        }
    }
}

/// Moves what `from` has written into `to`, keeping what `to` has not yet taken in `pending`.
fn to_server(from: &mut Runner<Client>, to: &mut Connection<Fake>, pending: &mut Vec<u8>) {
    let out = from.output_buf();
    pending.extend_from_slice(out);
    let n = out.len();
    from.consume_output(n);
    while let Ok(n @ 1..) = to.input(pending) {
        pending.drain(..n);
    }
}

fn to_client<C: CliServ>(from: &mut Connection<Fake>, to: &mut Runner<C>, pending: &mut Vec<u8>) {
    let out = from.output_buf();
    pending.extend_from_slice(out);
    let n = out.len();
    from.consume_output(n);
    while let Ok(n @ 1..) = to.input(pending) {
        pending.drain(..n);
    }
}

struct Run {
    seen: Seen,
    log: Rc<RefCell<Log>>,
    /// The server's last word: `None` if it was still going when the run stopped.
    server: Option<Result<Progress, redoubt_sshd::Error>>,
}

/// Runs `script` until the client's channel closes, the server stops, or nothing moves; then, if
/// `hang_up`, closes the server's input.
fn run_with(script: Script, sign: bool, hang_up: bool) -> Run {
    let log = Rc::new(RefCell::new(Log::default()));
    let mut platform = Fake { log: log.clone(), sign };
    let (mut ci, mut co, mut si, mut so) = (vec![0; BUF], vec![0; BUF], vec![0; BUF], vec![0; BUF]);
    let mut cli = Runner::new_client(&mut ci, &mut co);
    let mut conn = Box::new(Connection::new(&mut si, &mut so, &host().pk));
    let (mut to_srv, mut to_cli) = (Vec::new(), Vec::new());
    let (mut c, mut seen) = (Cli::default(), Seen::default());
    let mut server = None;
    'run: for _ in 0..300 {
        client(&mut cli, &script, &mut c, &mut seen);
        if seen.closed {
            break;
        }
        to_server(&mut cli, &mut conn, &mut to_srv);
        for _ in 0..1000 {
            match conn.progress(&mut platform) {
                Ok(Progress::Busy) => continue,
                Ok(Progress::Idle) => break,
                last => {
                    server = Some(last);
                    break 'run;
                }
            }
        }
        to_client(&mut conn, &mut cli, &mut to_cli);
    }
    if hang_up && server.is_none() {
        conn.close_input();
        server = Some(conn.progress(&mut platform));
    }
    Run { seen, log, server }
}

fn run(script: Script) -> Run { run_with(script, true, false) }

#[test]
fn a_login_runs_a_session_and_ends_with_its_status() {
    let r = run(Script { data: b"hello!", ..Script::default() });
    let log = r.log.borrow();
    assert!(r.seen.authenticated);
    assert_eq!(log.logins, [("alice".to_string(), None, None)]);
    assert_eq!(log.started, Some(Some(Window { cols: 80, rows: 24 })));
    assert_eq!(log.input, b"hello!");
    assert_eq!(r.seen.output, b"hello");
    assert_eq!(r.seen.exit, Some(7));
    assert!(r.seen.closed);
    assert_eq!(log.ended, 1);
}

#[test]
fn a_query_never_reaches_the_steward() {
    let r = run(Script::default());
    let log = r.log.borrow();
    // sunset's client asks with the key, then signs with it: holds is asked both times, the
    // steward once, after the signature.
    assert!(r.seen.authenticated);
    assert_eq!(log.holds, 2);
    assert_eq!(log.logins.len(), 1);
}

#[test]
fn a_key_keyd_holds_is_refused_before_the_steward() {
    let r = run(Script { key: held(), ..Script::default() });
    assert!(!r.seen.authenticated);
    assert_eq!(r.log.borrow().holds, 1);
    assert!(r.log.borrow().logins.is_empty());
}

#[test]
fn the_stewards_refusal_is_a_refused_login() {
    let r = run(Script { user: "bob", ..Script::default() });
    assert!(!r.seen.authenticated);
    assert_eq!(r.log.borrow().logins, [("bob".to_string(), None, None)]);
    assert!(r.log.borrow().started.is_none());
}

/// A user name that is not a login is still asked of the steward, as nobody, once its signature
/// has verified: every refusal after the signature takes the one path (no enumeration).
#[test]
fn a_user_name_outside_the_grammar_reaches_the_steward_as_nobody() {
    for user in [
        "Alice",
        "1alice",
        "alice+",
        "+x",
        "alice+b+c",
        "alice.work+tax",
        "alice:x",
        "approve.x",
        "al ice",
        "",
    ] {
        let r = run(Script { user, ..Script::default() });
        assert!(!r.seen.authenticated, "{user:?}");
        assert_eq!(r.log.borrow().logins, [(String::new(), None, None)], "{user:?}");
    }
}

#[test]
fn a_context_reaches_the_steward_with_its_principal_and_label() {
    let r = run(Script { user: "alice.work", ..Script::default() });
    assert_eq!(r.log.borrow().logins, [("alice".to_string(), None, Some("work".to_string()))]);
    let r = run(Script { user: "alice+secrets.work", ..Script::default() });
    let want = ("alice".to_string(), Some("secrets".to_string()), Some("work".to_string()));
    assert_eq!(r.log.borrow().logins, [want]);
}

#[test]
fn the_login_grammar() {
    let login = |principal, label, context| Some(Login { principal, label, context });
    assert_eq!(Login::parse("alice"), login("alice", None, None));
    assert_eq!(Login::parse("approve"), login("approve", None, None), "a reserved name, bare");
    assert_eq!(Login::parse("alice+secrets"), login("alice", Some("secrets"), None));
    assert_eq!(Login::parse("alice.work"), login("alice", None, Some("work")));
    assert_eq!(Login::parse("alice+tax.work"), login("alice", Some("tax"), Some("work")));
    assert_eq!(Login::parse("a1_-+b2.c3_-"), login("a1_-", Some("b2"), Some("c3_-")));
    // One order only, one of each, no `:` (scp's separator), no case folding.
    for bad in [
        "",
        "Alice",
        "1a",
        "a+",
        "+a",
        "a+b+c",
        "a+1",
        "a.",
        ".a",
        "a.b.c",
        "a.b+c",
        "a.B",
        "a:b",
        "a+b:c",
        "a b",
        "é",
        "approve+x",
        "approve.x",
        "approve+x.y",
    ] {
        assert_eq!(Login::parse(bad), None, "{bad:?}");
    }
    let long = "a".repeat(64);
    assert!(Login::parse(&format!("{long}+{long}.{long}")).is_some());
    for over in [format!("{long}a"), format!("a+{long}a"), format!("a.{long}a")] {
        assert!(Login::parse(&over).is_none(), "{over}");
    }
}

#[test]
fn a_labelled_login_reaches_its_shell_only_with_a_pty() {
    let r = run(Script { user: "alice+secrets", ..Script::default() });
    assert_eq!(r.log.borrow().logins, [("alice".to_string(), Some("secrets".to_string()), None)]);
    assert!(r.log.borrow().started.is_some());

    // R67: without a pty, the shell is refused and nothing the client sends reaches the session.
    let r = run(Script { user: "alice+secrets", pty: None, data: b"hello", ..Script::default() });
    assert!(r.seen.authenticated);
    assert!(r.log.borrow().started.is_none());
    assert!(r.log.borrow().input.is_empty());
    assert_eq!(r.log.borrow().refused, [Refusal::ShellWithoutPty]);
}

#[test]
fn an_unlabelled_login_may_have_a_shell_without_a_pty() {
    let r = run(Script { pty: None, data: b"x!", ..Script::default() });
    assert_eq!(r.log.borrow().started, Some(None));
    assert_eq!(r.seen.exit, Some(7));
}

#[test]
fn exec_and_subsystems_are_refused() {
    for (request, refusal) in [(Request::Exec, Refusal::Exec), (Request::Subsystem, Refusal::Subsystem)] {
        for user in ["alice", "alice+secrets"] {
            let r = run(Script { user, request, data: b"hello", ..Script::default() });
            assert!(r.seen.authenticated);
            assert!(r.log.borrow().started.is_none());
            assert!(r.log.borrow().input.is_empty());
            assert_eq!(r.log.borrow().refused, [refusal]);
        }
    }
}

#[test]
fn a_window_change_without_a_size_is_refused() {
    let r = run(Script { windows: &[(132, 43), (0, 43), (132, 0), (0, 0), (1024, 1)], ..Script::default() });
    assert_eq!(r.log.borrow().windows, [Window { cols: 132, rows: 43 }, Window { cols: 1024, rows: 1 }]);
    assert_eq!(r.log.borrow().refused, [Refusal::WindowChange; 3]);
}

#[test]
fn a_window_change_before_the_shell_is_refused() {
    // No shell starts: the request comes before any session to take it.
    let r = run(Script { request: Request::Exec, windows: &[(132, 43)], ..Script::default() });
    assert!(r.log.borrow().windows.is_empty());
    assert_eq!(r.log.borrow().refused, [Refusal::Exec, Refusal::WindowChange]);
}

#[test]
fn a_refusal_is_named_by_its_kind() {
    let names = [
        Refusal::Env,
        Refusal::Exec,
        Refusal::Subsystem,
        Refusal::Pty,
        Refusal::Shell,
        Refusal::ShellWithoutPty,
        Refusal::WindowChange,
        Refusal::Signal,
        Refusal::Break,
    ]
    .map(Refusal::name);
    assert_eq!(
        names,
        [
            "env",
            "exec",
            "subsystem",
            "pty-req",
            "shell",
            "shell-without-pty",
            "window-change",
            "signal",
            "break"
        ]
    );
}

#[test]
fn a_window_change_over_the_largest_is_cut_to_it() {
    let r = run(Script { windows: &[(1025, 43), (132, 1025), (u32::MAX, u32::MAX)], ..Script::default() });
    let max = Window::MAX;
    assert_eq!(
        r.log.borrow().windows,
        [Window { cols: max, rows: 43 }, Window { cols: 132, rows: max }, Window { cols: max, rows: max }]
    );
}

#[test]
fn a_pty_without_a_size_starts_at_the_default() {
    for pty in [(0, 0), (80, 0), (0, 24)] {
        let r = run(Script { pty: Some(pty), ..Script::default() });
        assert_eq!(r.log.borrow().started, Some(Some(Window::DEFAULT)), "{pty:?}");
    }
}

#[test]
fn a_pty_over_the_largest_is_cut_to_it() {
    let r = run(Script { pty: Some((5000, 24)), ..Script::default() });
    assert_eq!(r.log.borrow().started, Some(Some(Window { cols: Window::MAX, rows: 24 })));
    let r = run(Script { pty: Some((u32::MAX, u32::MAX)), ..Script::default() });
    assert_eq!(r.log.borrow().started, Some(Some(Window { cols: Window::MAX, rows: Window::MAX })));
}

#[test]
fn only_int_and_break_interrupt() {
    let r = run(Script { signals: &["TERM", "KILL", "INT", "int", "SIGINT"], ..Script::default() });
    assert_eq!(r.log.borrow().interrupts, 1);
    assert_eq!(r.log.borrow().refused, [Refusal::Signal; 4]);
    let r = run(Script { brk: true, ..Script::default() });
    assert_eq!(r.log.borrow().interrupts, 1);
    // Only the requests the core names are told; a shell asked and given is not one.
    assert!(r.log.borrow().refused.is_empty());
}

#[test]
fn a_second_session_channel_is_refused() {
    let r = run(Script { second_channel: true, ..Script::default() });
    assert!(r.seen.authenticated);
    assert_eq!(r.seen.sessions_opened, 1);
    assert_eq!(r.log.borrow().logins.len(), 1);
}

#[test]
fn a_refused_signature_fails_the_exchange() {
    let r = run_with(Script::default(), false, false);
    assert!(matches!(r.server, Some(Err(redoubt_sshd::Error::SignRefused))));
    assert!(!r.seen.authenticated);
    assert!(r.log.borrow().logins.is_empty());
}

#[test]
fn a_dropped_connection_ends_the_session() {
    // The session never ends by itself: the console waits for more input.
    let r = run_with(Script { data: b"hello", ..Script::default() }, true, true);
    assert!(r.log.borrow().started.is_some());
    assert_eq!(r.seen.exit, None);
    assert!(matches!(r.server, Some(Ok(Progress::Closed))));
    assert_eq!(r.log.borrow().ended, 1);
}
