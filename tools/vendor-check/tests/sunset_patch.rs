//! `vendor/patches/sunset.patch` does what docs/servers/sshd.md says it does, seen through
//! `sunset`'s public API: a sunset client and the patched sunset server, in process, with the bytes
//! between them moved by hand.
//!
//! - A public-only host key is signed outside `sunset`: the server hands out the exchange's parts, `keyd`'s
//!   own `exchange_hash` builds the hash from them, and the client, which computes the hash itself, accepts
//!   the signature. So `keyd`'s hash and the one SSH signs agree.
//! - A signature over anything else is refused before it is sent.
//! - The peer's `KEXINIT` is kept up to 4 KiB, and a longer one is refused.
//! - A client's `pty-req` gives the server the terminal's name and starting size, and its `window-change`,
//!   `signal` and `break` reach the server as events; the client can send a `signal`.
//! - The server tells a key query from a signed request (`signed()`).
//! - The server ends a session with its exit status, then EOF, then close, and cannot write to it after; when
//!   its output is full it fails `BusySend`, and once the output drains it sends the status once.

use redoubt_keyd::ssh::{Transcript, exchange_hash};
use sunset::ed25519_compact::{KeyPair, Seed};
use sunset::event::{CliEvent, Event, ServEvent};
use sunset::packets::WinChange;
use sunset::{
    ChanData, ChanHandle, CliServ, CliSessionExit, Client, Error, OwnedSig, Pty, Runner, Server, SignKey,
};

/// The largest packet `sunset` takes, as its `config::SSH_MAX_PACKET` says.
const BUF: usize = 35_000;

/// A server output buffer the channel data a client's window allows fills, leaving no room for an exit
/// status.
const SRV_OUT: usize = 1024;

/// The host key, whose private half only the test's stand-in for `keyd` uses.
fn host_pair() -> KeyPair { KeyPair::from_seed(Seed::new([7; 32])) }

/// Moves what `from` has written into `to`, keeping what `to` has not yet taken in `pending`.
fn pump<A: CliServ, B: CliServ>(from: &mut Runner<A>, to: &mut Runner<B>, pending: &mut Vec<u8>) {
    let out = from.output_buf();
    pending.extend_from_slice(out);
    let n = out.len();
    from.consume_output(n);
    while !pending.is_empty() {
        let n = to.input(pending).unwrap();
        if n == 0 {
            break;
        }
        pending.drain(..n);
    }
}

/// How the server's stand-in for `keyd` signs.
#[derive(Clone, Copy, PartialEq)]
enum Signer {
    /// `sunset` holds the whole key and signs itself, as published.
    InSunset,
    /// `keyd`'s hash over the parts `sunset` hands out.
    Keyd,
    /// A signature over a hash that is not this exchange's.
    Wrong,
}

/// What the server saw.
#[derive(Default)]
struct Seen {
    exchanges: Vec<Parts>,
    authenticated: bool,
    shell: bool,
    pty: Option<(String, u32, u32)>,
    window: Option<(u32, u32)>,
    signal: Option<String>,
    break_ms: Option<u32>,
    /// `signed()` for each public key the client offered, in order.
    pubkey_signed: Vec<bool>,
    /// Whether a write after `session_exit()` failed.
    write_after_exit_failed: bool,
    /// Each `session_exit()` call's result: whether it was `BusySend`.
    exit_busy: Vec<bool>,
    /// The client's end of the session: its exit status, whether the channel had its EOF when that
    /// arrived, and whether it is closed.
    exit: Option<u32>,
    exits: usize,
    eof_at_exit: Option<bool>,
    closed: bool,
    /// Whether `signed()` refused the signature as not over this exchange.
    refused: bool,
}

/// One exchange's parts, as handed out.
struct Parts {
    v_c: Vec<u8>,
    v_s: Vec<u8>,
    i_c: Vec<u8>,
    i_s: Vec<u8>,
    q_c: Vec<u8>,
    q_s: Vec<u8>,
    k: Vec<u8>,
}

/// Runs the server's events until it has none; false once the connection has failed.
fn serve(r: &mut Runner<Server>, signer: Signer, chan: &mut Option<ChanHandle>, seen: &mut Seen) -> bool {
    let pair = host_pair();
    let in_sunset = SignKey::Ed25519(pair.clone());
    let outside = SignKey::AgentEd25519(pair.pk);
    loop {
        let ev = match r.progress() {
            Ok(Event::Serv(ev)) => ev,
            Ok(Event::None) => return true,
            Ok(_) => continue,
            Err(_) => return false,
        };
        let r = match ev {
            ServEvent::Hostkeys(k) => {
                k.hostkeys(&[if signer == Signer::InSunset { &in_sunset } else { &outside }])
            }
            ServEvent::SignExchange(s) => {
                let t = s.transcript().unwrap();
                seen.exchanges.push(Parts {
                    v_c: t.v_c.to_vec(),
                    v_s: t.v_s.to_vec(),
                    i_c: t.i_c.to_vec(),
                    i_s: t.i_s.to_vec(),
                    q_c: t.q_c.to_vec(),
                    q_s: t.q_s.to_vec(),
                    k: t.k.to_vec(),
                });
                let parts = Transcript {
                    v_c: t.v_c,
                    v_s: t.v_s,
                    i_c: t.i_c,
                    i_s: t.i_s,
                    q_c: t.q_c,
                    q_s: t.q_s,
                    k: t.k,
                };
                let mut hash = exchange_hash(&parts, &pair.pk).unwrap();
                if signer == Signer::Wrong {
                    hash[0] ^= 1;
                }
                let sig = pair.sk.sign(hash, None);
                let r = s.signed(&OwnedSig::Ed25519(*sig));
                seen.refused = matches!(r, Err(Error::BadSig));
                r
            }
            ServEvent::FirstAuth(a) => a.reject(),
            ServEvent::PubkeyAuth(a) => {
                seen.pubkey_signed.push(a.signed());
                a.allow()
            }
            ServEvent::Authenticated => {
                seen.authenticated = true;
                Ok(())
            }
            ServEvent::OpenSession(o) => o.accept().map(|c| *chan = Some(c)),
            ServEvent::SessionPty(t) => {
                let pty = t.pty().unwrap();
                seen.pty = Some((pty.term.to_string(), pty.cols, pty.rows));
                t.succeed()
            }
            ServEvent::SessionShell(s) => {
                seen.shell = true;
                s.succeed()
            }
            ServEvent::SessionWinChange(w) => {
                let size = w.size().unwrap();
                seen.window = Some((size.cols, size.rows));
                w.succeed()
            }
            ServEvent::SessionSignal(g) => {
                seen.signal = Some(g.signal().unwrap().to_string());
                g.succeed()
            }
            ServEvent::SessionBreak(b) => {
                seen.break_ms = Some(b.length().unwrap());
                b.succeed()
            }
            other => panic!("unexpected {other:?}"),
        };
        if r.is_err() {
            return false;
        }
    }
}

/// A client's progress through the connection.
#[derive(Default)]
struct ClientState {
    chan: Option<ChanHandle>,
    offered_key: bool,
    open: bool,
    shell: bool,
    sent: bool,
    exit: Option<u32>,
    exits: usize,
    eof_at_exit: Option<bool>,
}

/// Runs the client's events until it has none, then sends what the test sends once it has a shell.
fn client(r: &mut Runner<Client>, c: &mut ClientState) {
    let host = host_pair().pk;
    loop {
        let ev = match r.progress().unwrap() {
            Event::Cli(ev) => ev,
            Event::None => break,
            _ => continue,
        };
        match ev {
            CliEvent::Hostkey(h) => {
                let offered = h.hostkey().unwrap();
                assert_eq!(SignKey::from_agent_pubkey(&offered).unwrap(), SignKey::AgentEd25519(host));
                h.accept().unwrap()
            }
            CliEvent::Username(u) => u.username("alice").unwrap(),
            CliEvent::Password(p) => p.skip().unwrap(),
            CliEvent::Pubkey(p) if !c.offered_key => {
                c.offered_key = true;
                p.pubkey(SignKey::Ed25519(KeyPair::from_seed(Seed::new([9; 32])))).unwrap()
            }
            CliEvent::Pubkey(p) => p.skip().unwrap(),
            CliEvent::Authenticated => c.open = true,
            CliEvent::SessionExit(e) => match e {
                CliSessionExit::Status(n) => {
                    c.exit = Some(n);
                    c.exits += 1;
                }
                other => panic!("unexpected {other:?}"),
            },
            CliEvent::SessionOpened(mut o) => {
                let term = heapless::String::try_from("xterm-256color").unwrap();
                o.pty(Pty { term, cols: 80, rows: 24, width: 0, height: 0, modes: heapless::Vec::new() })
                    .unwrap();
                o.shell().unwrap();
                c.shell = true;
            }
            other => panic!("unexpected {other:?}"),
        }
        if let (Some(_), None, Some(chan)) = (c.exit, c.eof_at_exit, &c.chan) {
            c.eof_at_exit = Some(r.is_channel_eof(chan));
        }
    }
    if c.open {
        c.open = false;
        c.chan = Some(r.open_client_session().unwrap());
    }
    if let Some(chan) = &c.chan {
        // The server's data is only filler: taken, so that what follows it arrives.
        while let Ok(1..) = r.read_channel(chan, ChanData::Normal, &mut [0; 1024]) {}
    }
    if let (true, false, Some(chan)) = (c.shell, c.sent, &c.chan) {
        c.sent = true;
        r.term_window_change(chan, &WinChange { cols: 132, rows: 43, width: 0, height: 0 }).unwrap();
        r.term_signal(chan, "INT").unwrap();
        r.term_break(chan, 1000).unwrap();
    }
}

/// A connection run until the client's session has closed, or it fails, or stalls. Once the server has
/// seen a break it ends the session with exit status 3.
fn connect(signer: Signer) -> Seen { connect_with(signer, BUF, false) }

/// [`connect`], with `srv_out` bytes of output buffer for the server; with `fill`, the server first writes
/// channel data until it can write no more.
fn connect_with(signer: Signer, srv_out: usize, fill: bool) -> Seen {
    let (mut ci, mut co, mut si, mut so) = (vec![0; BUF], vec![0; BUF], vec![0; BUF], vec![0; srv_out]);
    let mut cli = Runner::new_client(&mut ci, &mut co);
    let mut srv = Runner::new_server(&mut si, &mut so);
    let (mut to_srv, mut to_cli) = (Vec::new(), Vec::new());
    let mut c = ClientState::default();
    let mut srv_chan = None;
    let mut seen = Seen::default();
    for _ in 0..200 {
        client(&mut cli, &mut c);
        if !serve(&mut srv, signer, &mut srv_chan, &mut seen) {
            break;
        }
        if let (Some(_), Some(chan), false) = (seen.break_ms, &srv_chan, seen.write_after_exit_failed) {
            if fill && seen.exit_busy.is_empty() {
                while srv.write_channel(chan, ChanData::Normal, &[b'x'; 64]).unwrap() > 0 {}
            }
            match srv.session_exit(chan, 3) {
                Ok(()) => {
                    seen.exit_busy.push(false);
                    seen.write_after_exit_failed =
                        srv.write_channel(chan, ChanData::Normal, b"late").is_err();
                }
                Err(Error::BusySend { .. }) => seen.exit_busy.push(true),
                Err(e) => panic!("unexpected {e:?}"),
            }
        }
        if let Some(chan) = &c.chan {
            if cli.is_channel_closed(chan) {
                seen.closed = true;
                break;
            }
        }
        pump(&mut cli, &mut srv, &mut to_srv);
        pump(&mut srv, &mut cli, &mut to_cli);
    }
    (seen.exit, seen.exits, seen.eof_at_exit) = (c.exit, c.exits, c.eof_at_exit);
    seen
}

#[test]
fn keyds_hash_is_the_one_ssh_signs() {
    let seen = connect(Signer::Keyd);
    // The client checked the signature over its own hash, or it would not have gone on.
    assert!(seen.authenticated && seen.shell);
    assert!(!seen.refused);
    assert_eq!(seen.exchanges.len(), 1);
    let p = &seen.exchanges[0];
    assert!(p.v_c.starts_with(b"SSH-2.0-") && p.v_s.starts_with(b"SSH-2.0-"));
    assert!(!p.v_c.ends_with(b"\n") && !p.v_s.ends_with(b"\n"));
    // Both payloads begin with SSH_MSG_KEXINIT.
    assert_eq!((p.i_c[0], p.i_s[0]), (20, 20));
    assert_ne!(p.i_c, p.i_s);
    assert_eq!((p.q_c.len(), p.q_s.len()), (32, 32));
    // K as an mpint body: no leading zero unless the next byte's top bit is set.
    assert!(!p.k.is_empty() && p.k.len() <= 33);
    assert!(p.k[0] != 0 || p.k[1] & 0x80 != 0);
    assert!(p.k[0] & 0x80 == 0);
}

#[test]
fn a_whole_key_is_still_signed_in_sunset() {
    let seen = connect(Signer::InSunset);
    assert!(seen.authenticated && seen.shell);
    assert!(seen.exchanges.is_empty());
}

#[test]
fn a_signature_over_another_hash_is_refused_before_it_is_sent() {
    let seen = connect(Signer::Wrong);
    assert_eq!(seen.exchanges.len(), 1);
    assert!(seen.refused);
    assert!(!seen.authenticated);
}

#[test]
fn pty_window_change_signal_and_break_reach_the_server() {
    let seen = connect(Signer::Keyd);
    assert_eq!(seen.pty, Some(("xterm-256color".to_string(), 80, 24)));
    assert_eq!(seen.window, Some((132, 43)));
    assert_eq!(seen.signal.as_deref(), Some("INT"));
    assert_eq!(seen.break_ms, Some(1000));
}

#[test]
fn a_key_query_and_a_signed_request_are_told_apart() {
    let seen = connect(Signer::Keyd);
    // sunset's client asks whether the key would do before it signs.
    assert_eq!(seen.pubkey_signed, [false, true]);
}

#[test]
fn a_session_ends_with_its_status_then_eof_then_close() {
    let seen = connect(Signer::Keyd);
    assert!(seen.write_after_exit_failed);
    assert_eq!(seen.exit, Some(3));
    assert_eq!(seen.exits, 1);
    assert_eq!(seen.exit_busy, [false]);
    assert_eq!(seen.eof_at_exit, Some(false));
    assert!(seen.closed);
}

#[test]
fn a_session_ended_on_a_full_output_ends_once_it_drains() {
    let seen = connect_with(Signer::Keyd, SRV_OUT, true);
    assert_eq!(seen.exit_busy.first(), Some(&true));
    assert_eq!(seen.exit_busy.last(), Some(&false));
    assert!(seen.write_after_exit_failed);
    assert_eq!(seen.exit, Some(3));
    assert_eq!(seen.exits, 1);
    assert!(seen.closed);
}

/// A client `KEXINIT` payload of exactly `len` bytes, offering the algorithms `sunset` takes and
/// padded out in its language list, which `sunset` ignores.
fn kexinit(len: usize) -> Vec<u8> {
    let payload = |pad: usize| {
        let mut p = vec![20u8];
        p.extend([0u8; 16]);
        let lang = "x".repeat(pad);
        let lists = ["curve25519-sha256", "ssh-ed25519", "aes256-ctr", "aes256-ctr", "hmac-sha2-256"];
        for list in lists.iter().chain(&["hmac-sha2-256", "none", "none", lang.as_str(), ""]) {
            p.extend((list.len() as u32).to_be_bytes());
            p.extend(list.as_bytes());
        }
        p.push(0);
        p.extend([0u8; 4]);
        p
    };
    let p = payload(len - payload(0).len());
    assert_eq!(p.len(), len);
    p
}

/// Whether the server takes a first `KEXINIT` payload of `len` bytes, sent in the clear.
fn server_takes_kexinit(len: usize) -> bool {
    let (mut si, mut so) = (vec![0; BUF], vec![0; BUF]);
    let mut srv = Runner::new_server(&mut si, &mut so);
    while !matches!(srv.progress().unwrap(), Event::None) {}
    let payload = kexinit(len);
    // RFC 4253 section 6: at least 4 bytes of padding, the whole a multiple of 8.
    let mut pad = 8 - (5 + payload.len()) % 8;
    if pad < 4 {
        pad += 8;
    }
    let mut wire = b"SSH-2.0-test\r\n".to_vec();
    wire.extend(((1 + payload.len() + pad) as u32).to_be_bytes());
    wire.push(pad as u8);
    wire.extend(&payload);
    wire.extend(vec![0; pad]);
    let mut at = 0;
    while at < wire.len() {
        let n = srv.input(&wire[at..]).unwrap();
        at += n;
        loop {
            match srv.progress() {
                Ok(Event::None) => break,
                Ok(_) => {}
                Err(Error::BigPacket { .. }) => return false,
                Err(e) => panic!("unexpected {e:?}"),
            }
        }
    }
    true
}

#[test]
fn a_peer_kexinit_over_4_kib_is_refused() {
    assert!(server_takes_kexinit(4096));
    assert!(!server_takes_kexinit(4097));
}
