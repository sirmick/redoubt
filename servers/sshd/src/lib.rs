//! `sshd`'s core: SSH over byte slices on the patched `sunset`, and every decision
//! servers/sshd.md states, behind one trait, [`Platform`]. What is here:
//!
//! - [`Connection`]: one SSH connection. The platform moves its bytes ([`Connection::input`],
//!   [`Connection::output_buf`]) and calls [`Connection::progress`] until it is idle.
//! - [`Platform`]: what the core asks for: `keyd`'s signature over an exchange and its `holds`, the steward's
//!   login, and a session's console, [`Session`].
//! - [`Login`]: the user name's grammar, `principal[+label][.context]`.
//! - [`listener`]: the reads the program's threads ask `ipd` again when its wait runs out.
//!
//! What the core decides, so that no platform has to:
//!
//! - **Keys.** Ed25519 only. A key `keyd` holds is refused, and so is any key when `holds` fails. A client's
//!   query ("would this key do?") is answered from `holds` alone; the steward is asked only once `sunset` has
//!   verified the signature. Passwords and `none` are refused.
//! - **Channels.** One session channel per connection, after login; a second is refused. `exec`, subsystems
//!   and `env` fail (no `exec` on the box; SFTP is later). A labelled session's shell needs a pty (R67).
//!   Forwarding, agent and X11 requests `sunset` refuses itself. The platform is told the kind of each
//!   request the core refuses, [`Refusal`], never its content: an `env` name or value is the client's.
//! - **Raw numbers stay here.** A window size over [`Window::MAX`] reaches the session cut to it. A zero
//!   means no size, as RFC 4254 says (a client whose input is not a terminal sends zeros): a `window-change`
//!   carrying one fails, and a pty asked for with one starts at [`Window::DEFAULT`]. Only the `INT` signal
//!   and `break` reach the session, both as [`Session::interrupt`]; a break's length is never passed on.
//! - **Ending.** When the session ends and its output has gone out, the client gets its exit status, then EOF
//!   and close. When the client closes the channel or the connection ends, the session ends.
//!
//! `sunset`'s exchange hash state (about 4.6 KiB) lives in the [`Connection`], so a platform places
//! the connection statically or on the heap; `sunset` still moves it by value during the exchange, so
//! the thread that calls [`Connection::progress`] needs room for a few copies on its stack.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod console;
pub mod listener;
pub mod slot;

use sunset::ed25519_compact::PublicKey as Ed25519Public;
pub use sunset::event::ExchangeTranscript;
use sunset::event::{Event, ServEvent, ServPubkeyAuth};
use sunset::{ChanData, ChanFail, ChanHandle, ChanNum, OwnedSig, PubKey, Runner, Server, SignKey};

/// An Ed25519 public key.
pub type PublicKey = [u8; 32];
/// An Ed25519 signature.
pub type Signature = [u8; 64];

/// A platform's refusal, or its failure to answer: the core treats both alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused;

/// A terminal's size in characters, each between 1 and [`Window::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub cols: u32,
    pub rows: u32,
}

impl Window {
    /// A pty's size when the client gives none.
    pub const DEFAULT: Window = Window { cols: 80, rows: 24 };
    /// The largest number of columns or rows a session is given.
    pub const MAX: u32 = 1024;

    /// The size a client asks for, each side cut to [`Window::MAX`]; `None` if either is zero.
    fn asked(cols: u32, rows: u32) -> Option<Window> {
        (cols > 0 && rows > 0).then(|| Window { cols: cols.min(Self::MAX), rows: rows.min(Self::MAX) })
    }
}

/// A channel request the core refused, by kind. Its content is the client's and is never passed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Env,
    Exec,
    Subsystem,
    /// A pty asked twice, after the shell, or on a channel not the session's.
    Pty,
    /// A shell asked twice, or on a channel not the session's.
    Shell,
    /// R67: a labelled session's shell asked without a pty.
    ShellWithoutPty,
    /// A window change without a size, or not for a started session's pty.
    WindowChange,
    /// A signal other than `INT`, or before the shell.
    Signal,
    /// A break before the shell.
    Break,
}

impl Refusal {
    /// The request's name in RFC 4254 (and RFC 4335 for `break`); R67's refusal is named apart.
    pub fn name(self) -> &'static str {
        match self {
            Refusal::Env => "env",
            Refusal::Exec => "exec",
            Refusal::Subsystem => "subsystem",
            Refusal::Pty => "pty-req",
            Refusal::Shell => "shell",
            Refusal::ShellWithoutPty => "shell-without-pty",
            Refusal::WindowChange => "window-change",
            Refusal::Signal => "signal",
            Refusal::Break => "break",
        }
    }
}

/// Who a login is for: the SSH user name `principal[+label][.context]`, in that order only, each
/// part a name (servers/sshd.md, "Login"). The steward checks every part again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Login<'a> {
    pub principal: &'a str,
    pub label: Option<&'a str>,
    /// The context; `None` is the principal's default one.
    pub context: Option<&'a str>,
}

impl<'a> Login<'a> {
    /// What a user name that is not a login is sent as: no principal has the empty name, so the
    /// steward refuses it as it refuses an unknown one, on the same path (no enumeration).
    pub const NOBODY: Login<'static> = Login { principal: "", label: None, context: None };
    /// User names that are a terminal's own (`approve@box`): they take no label and no context.
    pub const RESERVED: [&'static str; 1] = ["approve"];

    /// The login a user name asks for, or `None` if it is not one. `alice.work+tax` is not: the
    /// context comes last, and a name holds no `+`. Nor is `approve.x` or `approve+x`.
    pub fn parse(user: &'a str) -> Option<Login<'a>> {
        let (rest, context) = match user.split_once('.') {
            Some((rest, context)) => (rest, Some(context)),
            None => (user, None),
        };
        let (principal, label) = match rest.split_once('+') {
            Some((principal, label)) => (principal, Some(label)),
            None => (rest, None),
        };
        let suffixed = label.is_some() || context.is_some();
        let reserved = suffixed && Self::RESERVED.contains(&principal);
        (name(principal) && label.is_none_or(name) && context.is_none_or(name) && !reserved)
            .then_some(Login { principal, label, context })
    }
}

/// 1 to 64 bytes of `[a-z0-9_-]`, starting with a letter: a principal's, a label's or a
/// context's name.
pub fn name(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() <= 64
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

/// What the core asks of the machine it runs on (servers/sshd.md, "The core and its platforms").
pub trait Platform {
    type Session: Session;

    /// `keyd`'s signature with the host key over the exchange hash it builds from `t`.
    fn sign_exchange(&mut self, t: &ExchangeTranscript<'_>) -> Result<Signature, Refused>;
    /// Whether `keyd` holds `key`.
    fn holds(&mut self, key: &PublicKey) -> Result<bool, Refused>;
    /// The steward's answer to a login with `key`, whose signature has verified.
    fn login(&mut self, who: &Login<'_>, key: &PublicKey) -> Result<Self::Session, Refused>;
    /// The session is over for this connection: its channel has closed or the connection ended.
    fn end(&mut self, session: Self::Session);
    /// The core refused a channel request of this kind.
    fn refused(&mut self, request: Refusal);
}

/// A session's console: bytes each way, the window, the interrupt, and its end.
pub trait Session {
    /// Whether the session carries labels.
    fn labelled(&self) -> bool;
    /// The client asked for a shell; `pty` is the pty's starting size, if it asked for one.
    fn start(&mut self, pty: Option<Window>);
    /// Input from the client; returns how much was taken, which may be less, or none.
    fn input(&mut self, bytes: &[u8]) -> usize;
    /// The client will send no more input.
    fn input_ended(&mut self);
    /// Output for the client, into `buf`; returns its length, 0 when there is none.
    fn output(&mut self, buf: &mut [u8]) -> usize;
    /// The pty's new size.
    fn window(&mut self, w: Window);
    /// The client's `INT` signal or break.
    fn interrupt(&mut self);
    /// The exit status, once the session has ended: it then makes no more output.
    fn ended(&self) -> Option<u32>;
}

/// What [`Connection::progress`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Something happened: call again.
    Busy,
    /// Nothing more until input arrives, output drains, or the session changes.
    Idle,
    /// The connection is over.
    Closed,
}

/// Why a connection failed. The session, if any, has ended.
#[derive(Debug)]
pub enum Error {
    /// `sunset` failed the connection.
    Ssh(sunset::Error),
    /// `keyd` did not sign the exchange: the key exchange fails.
    SignRefused,
}

impl From<sunset::Error> for Error {
    fn from(e: sunset::Error) -> Self { Error::Ssh(e) }
}

/// The input taken from the channel that the session has not taken yet.
const PENDING: usize = 256;
/// The most output taken from the session at once.
const OUT: usize = 512;

/// One SSH connection with at most one session.
pub struct Connection<'a, P: Platform> {
    runner: Runner<'a, Server>,
    host: SignKey,
    session: Option<P::Session>,
    chan: Option<ChanHandle>,
    /// Whether a session channel has been opened on this connection.
    opened: bool,
    pty: Option<Window>,
    started: bool,
    told_eof: bool,
    pending: [u8; PENDING],
    pending_len: usize,
    out: [u8; OUT],
    closed: bool,
}

impl<'a, P: Platform> Connection<'a, P> {
    /// A connection whose host key is `host`, signed by the platform. `inbuf` and `outbuf` each hold
    /// the largest packet `sunset` takes (35,000 bytes).
    pub fn new(inbuf: &'a mut [u8], outbuf: &'a mut [u8], host: &PublicKey) -> Self {
        Connection {
            runner: Runner::new_server(inbuf, outbuf),
            host: SignKey::AgentEd25519(Ed25519Public::new(*host)),
            session: None,
            chan: None,
            opened: false,
            pty: None,
            started: false,
            told_eof: false,
            pending: [0; PENDING],
            pending_len: 0,
            out: [0; OUT],
            closed: false,
        }
    }

    /// Bytes from the client; returns how many were taken.
    pub fn input(&mut self, bytes: &[u8]) -> Result<usize, Error> { Ok(self.runner.input(bytes)?) }

    /// The client's side of the transport has closed.
    pub fn close_input(&mut self) { self.runner.close_input() }

    /// Bytes for the client.
    pub fn output_buf(&mut self) -> &[u8] { self.runner.output_buf() }

    /// `n` bytes of [`Connection::output_buf`] have been sent.
    pub fn consume_output(&mut self, n: usize) { self.runner.consume_output(n) }

    /// Runs the connection and its session as far as they go. On an error or [`Progress::Closed`]
    /// the session has ended, and the platform closes the transport.
    pub fn progress(&mut self, platform: &mut P) -> Result<Progress, Error> {
        if self.closed {
            return Ok(Progress::Closed);
        }
        let r = self.step(platform);
        if !matches!(r, Ok(Progress::Busy | Progress::Idle)) {
            self.end_session(platform);
            self.closed = true;
        }
        r
    }

    fn step(&mut self, platform: &mut P) -> Result<Progress, Error> {
        let mut busy = false;
        loop {
            match self.event(platform)? {
                Progress::Busy => busy = true,
                Progress::Idle => break,
                Progress::Closed => return Ok(Progress::Closed),
            }
        }
        busy |= self.pump(platform)?;
        Ok(if busy { Progress::Busy } else { Progress::Idle })
    }

    /// Handles one of `sunset`'s events: `Idle` when there is none.
    fn event(&mut self, platform: &mut P) -> Result<Progress, Error> {
        let Connection { runner, host, session, chan, opened, pty, started, .. } = self;
        let ev = match runner.progress()? {
            Event::Serv(ev) => ev,
            Event::None => return Ok(Progress::Idle),
            Event::Cli(_) | Event::Progressed => return Ok(Progress::Busy),
        };
        match ev {
            ServEvent::Hostkeys(k) => k.hostkeys(&[host])?,
            ServEvent::SignExchange(s) => {
                let sig = platform.sign_exchange(&s.transcript()?).map_err(|Refused| Error::SignRefused)?;
                s.signed(&OwnedSig::Ed25519(sig))?
            }
            ServEvent::FirstAuth(mut a) => {
                a.set_auth_methods(false, true)?;
                a.reject()?
            }
            ServEvent::PasswordAuth(a) => a.reject()?,
            ServEvent::PubkeyAuth(a) => {
                if pubkey(&a, session, platform) {
                    a.allow()?
                } else {
                    a.reject()?
                }
            }
            ServEvent::Authenticated | ServEvent::PollAgain => (),
            ServEvent::OpenSession(o) => {
                if *opened || session.is_none() {
                    o.reject(ChanFail::SSH_OPEN_ADMINISTRATIVELY_PROHIBITED)?
                } else {
                    *opened = true;
                    *chan = Some(o.accept()?);
                }
            }
            ServEvent::SessionPty(t) => {
                if ours(chan, t.channel()) && !*started && pty.is_none() {
                    let asked = t.pty()?;
                    *pty = Some(Window::asked(asked.cols, asked.rows).unwrap_or(Window::DEFAULT));
                    t.succeed()?
                } else {
                    platform.refused(Refusal::Pty);
                    t.fail()?
                }
            }
            ServEvent::SessionShell(s) => match session {
                // R67: a labelled session's output goes only to a pty channel.
                Some(sess) if ours(chan, s.channel()) && !*started && pty.is_none() && sess.labelled() => {
                    platform.refused(Refusal::ShellWithoutPty);
                    s.fail()?
                }
                Some(sess) if ours(chan, s.channel()) && !*started => {
                    sess.start(*pty);
                    *started = true;
                    s.succeed()?
                }
                _ => {
                    platform.refused(Refusal::Shell);
                    s.fail()?
                }
            },
            ServEvent::SessionExec(e) => {
                platform.refused(Refusal::Exec);
                e.fail()?
            }
            ServEvent::SessionSubsystem(e) => {
                platform.refused(Refusal::Subsystem);
                e.fail()?
            }
            ServEvent::SessionEnv(e) => {
                platform.refused(Refusal::Env);
                e.fail()?
            }
            ServEvent::SessionWinChange(w) => {
                let size = w.size()?;
                match (session.as_mut(), Window::asked(size.cols, size.rows)) {
                    (Some(sess), Some(win)) if ours(chan, w.channel()) && *started && pty.is_some() => {
                        sess.window(win);
                        w.succeed()?
                    }
                    _ => {
                        platform.refused(Refusal::WindowChange);
                        w.fail()?
                    }
                }
            }
            ServEvent::SessionSignal(g) => match session {
                Some(sess) if ours(chan, g.channel()) && *started && g.signal()? == "INT" => {
                    sess.interrupt();
                    g.succeed()?
                }
                _ => {
                    platform.refused(Refusal::Signal);
                    g.fail()?
                }
            },
            ServEvent::SessionBreak(b) => match session {
                Some(sess) if ours(chan, b.channel()) && *started => {
                    sess.interrupt();
                    b.succeed()?
                }
                _ => {
                    platform.refused(Refusal::Break);
                    b.fail()?
                }
            },
            ServEvent::Defunct => return Ok(Progress::Closed),
        }
        Ok(Progress::Busy)
    }

    /// Moves the session's bytes and ends it when it or its channel has ended; true if it did anything.
    fn pump(&mut self, platform: &mut P) -> Result<bool, Error> {
        let (busy, over) = self.exchange()?;
        if over {
            self.end_session(platform);
        }
        Ok(busy || over)
    }

    /// Moves the session's bytes each way; returns whether it did anything, and whether the session
    /// is over: its channel closed, or its exit status sent.
    fn exchange(&mut self) -> Result<(bool, bool), Error> {
        let Connection {
            runner,
            session,
            chan: Some(chan),
            started,
            told_eof,
            pending,
            pending_len,
            out,
            ..
        } = self
        else {
            return Ok((false, false));
        };
        let Some(sess) = session else { return Ok((false, false)) };
        if runner.is_channel_closed(chan) {
            return Ok((false, true));
        }
        let mut busy = false;
        if !*started {
            // Nothing takes input before the shell starts.
            if runner.read_channel_ready().is_some() {
                runner.discard_read_channel(chan)?;
                busy = true;
            }
            return Ok((busy, false));
        }

        // Input, as far as the session takes it.
        loop {
            if *pending_len > 0 {
                let n = sess.input(&pending[..*pending_len]);
                if n == 0 {
                    break;
                }
                pending.copy_within(n..*pending_len, 0);
                *pending_len -= n;
                busy = true;
            } else {
                match runner.read_channel_ready() {
                    None => break,
                    Some((_, ChanData::Normal, _)) => {
                        match runner.read_channel(chan, ChanData::Normal, pending) {
                            Ok(0) | Err(sunset::Error::ChannelEOF) => break,
                            Ok(n) => *pending_len = n,
                            Err(e) => return Err(e.into()),
                        }
                    }
                    // A client has no stderr to send.
                    Some(_) => runner.discard_read_channel(chan)?,
                }
                busy = true;
            }
        }
        if !*told_eof && *pending_len == 0 && runner.is_channel_eof(chan) {
            sess.input_ended();
            *told_eof = true;
            busy = true;
        }

        // Output, as far as the channel takes it. The status is read first, so once it is set and
        // the output is empty, all of it has gone.
        let status = sess.ended();
        let drained = loop {
            let room = match runner.write_channel_ready(chan, ChanData::Normal)? {
                None => break true,
                Some(0) => break false,
                Some(room) => room.min(OUT),
            };
            let n = sess.output(&mut out[..room]);
            if n == 0 {
                break true;
            }
            runner.write_channel(chan, ChanData::Normal, &out[..n])?;
            busy = true;
        };
        if let (true, Some(status)) = (drained, status) {
            match runner.session_exit(chan, status) {
                Ok(()) => return Ok((busy, true)),
                // No room for the status: try again once output has drained.
                Err(sunset::Error::BusySend { .. }) => (),
                Err(e) => return Err(e.into()),
            }
        }
        Ok((busy, false))
    }

    /// Ends the session, if there is one, and lets go of its channel.
    fn end_session(&mut self, platform: &mut P) {
        if let Some(sess) = self.session.take() {
            platform.end(sess);
        }
        if let Some(chan) = self.chan.take() {
            // Only fails for a channel `sunset` has already dropped.
            let _ = self.runner.channel_done(chan);
        }
        self.started = false;
    }
}

/// Whether request channel `num` is the session's.
fn ours(chan: &Option<ChanHandle>, num: ChanNum) -> bool { chan.as_ref().is_some_and(|c| c.num() == num) }

/// Whether a public key login may go on: on a query, whether the key may be tried; on a signed
/// request, whether the steward logged it in, leaving the session in `session`.
fn pubkey<P: Platform>(
    a: &ServPubkeyAuth<'_, '_>,
    session: &mut Option<P::Session>,
    platform: &mut P,
) -> bool {
    let Ok(PubKey::Ed25519(key)) = a.pubkey() else { return false };
    let key = key.key.0;
    if session.is_some() || platform.holds(&key) != Ok(false) {
        return false;
    }
    if !a.signed() {
        return true;
    }
    // A user name that is not a login still asks the steward, as nobody: every refusal after a
    // verified signature takes the one path (servers/sshd.md, "Login").
    let who = a.username().ok().and_then(Login::parse).unwrap_or(Login::NOBODY);
    match platform.login(&who, &key) {
        Ok(s) => {
            *session = Some(s);
            true
        }
        Err(Refused) => false,
    }
}
