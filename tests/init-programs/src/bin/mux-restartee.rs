//! `mux-restartee`: a `servers` entry that holds a multiplexed session at `walfsd` through the root
//! badge its entry is handed, for `init-restart-mux` (servers/serving.md, "Multiplexed connections",
//! the end; servers/init.md, "Restarts and reboots"). `init` hands every instance the same badge.
//!
//! - **The first instance** finds no `witness` on the volume: it makes it, holding [`TEXT`], opens its
//!   session, sends a read of it, and exits with code 0 with the read's answer pending and no completion call
//!   parked, as a client that dies between two completion calls does. `init` restarts it at once, well inside
//!   the session bound.
//! - **The next instance** finds `witness`, so it is the successor. Its first completion call, of hold 0,
//!   must open a session of its own (`OPENED`), not be parked on its predecessor's and collect that one's
//!   pending read. It then reads and writes through its session, says what came back, and stays up, so `init`
//!   starts no third instance.
//!
//! The case's verdict is `walfsd`'s own count of open sessions (its test-only feature
//! `session-count`); this program's lines say what the successor saw.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_init_programs::Out;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Words};
use redoubt_rt::server::ninep::{COLLECT_WAIT, OPENED, collect_words, mode};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::ninep::{Body, ErrorName, Message, message_size};

redoubt_rt::entry!(run);

/// The handle the entry is handed, by its endpoint's name.
const WALFSD: &str = "walfsd:data";
/// The file whose presence tells the successor from the first instance.
const WITNESS: &str = "witness";
/// What the first instance writes, and the successor reads back.
const TEXT: &[u8] = b"hello";
/// What the successor writes after it.
const MORE: &[u8] = b"world";
const ROOT_FID: u32 = 0;
const FILE_FID: u32 = 1;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let line = match instance(startup) {
        // The first instance's exit is the point: `init` restarts it.
        Ok(None) => return 0,
        Ok(Some(saw)) => format!("{saw}mux-restartee TEST PASSED\n"),
        Err(why) => format!("mux-restartee TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

/// `None` for the first instance, ready to exit; the successor's line otherwise.
fn instance(startup: &Startup) -> Result<Option<String>, String> {
    let handle = startup.handle(WALFSD).ok_or_else(|| format!("no {WALFSD} handle"))?;
    let ep = Endpoint::from_handle(handle);
    let conn = Connection::new(Endpoint::from_handle(handle));
    let mut lend = Lend::new(1).map_err(|e| format!("lend: {e:?}"))?;
    let lend = &mut lend;
    // A client versions its connection before it attaches: for the successor, the `Tversion`
    // that ends whatever its predecessor left on the badge.
    conn.version(lend).map_err(|e| format!("version: {e:?}"))?;
    conn.attach(lend, ROOT_FID, "").map_err(|e| format!("attach: {e:?}"))?;
    match conn.walk(lend, ROOT_FID, FILE_FID, WITNESS) {
        Err(ClientError::Rerror(ErrorName::NotFound)) => first(&ep, &conn, lend).map(|()| None),
        Ok(_) => successor(&ep, &conn, lend).map(Some),
        Err(e) => Err(format!("walk to {WITNESS}: {e:?}")),
    }
}

/// Makes the witness, opens a session and leaves a read of it pending.
fn first(ep: &Endpoint, conn: &Connection, lend: &mut Lend) -> Result<(), String> {
    conn.walk(lend, ROOT_FID, FILE_FID, "").map_err(|e| format!("clone the root: {e:?}"))?;
    conn.create(lend, FILE_FID, WITNESS, 0o644, mode::ORDWR).map_err(|e| format!("create: {e:?}"))?;
    if conn.write(lend, FILE_FID, 0, TEXT) != Ok(TEXT.len()) {
        return Err("the witness was not written whole".into());
    }
    let (words, _) = collect(ep, 0)?;
    if words != [0, 0, OPENED, 0] {
        return Err(format!("the first completion call answered {words:?}, not opened"));
    }
    send(ep, &[Message { tag: 0, body: Body::Tread { fid: FILE_FID, offset: 0, count: 64 } }])?;
    // A call behind the send on the same endpoint: answered, the send was taken before it, so the
    // read is pending at the server when the instance exits.
    conn.stat(lend, FILE_FID).map(|_| ()).map_err(|e| format!("stat: {e:?}"))
}

/// The successor: a session of its own at its first completion call, then a read and a write
/// through it.
fn successor(ep: &Endpoint, conn: &Connection, lend: &mut Lend) -> Result<String, String> {
    conn.open(lend, FILE_FID, mode::ORDWR).map_err(|e| format!("open: {e:?}"))?;
    let (words, got) = collect(ep, 0)?;
    if words != [0, 0, OPENED, 0] {
        let met = answers(&got).map_or_else(|e| e, |a| format!("{a:?}"));
        return Err(format!(
            "the first completion call answered {words:?} ({met}): the predecessor's session"
        ));
    }
    let read = Message { tag: 0, body: Body::Tread { fid: FILE_FID, offset: 0, count: 64 } };
    let write =
        Message { tag: 1, body: Body::Twrite { fid: FILE_FID, offset: TEXT.len() as u64, data: MORE } };
    send(ep, &[read, write])?;
    let mut answered = Vec::new();
    while answered.len() < 2 {
        let (words, got) = collect(ep, COLLECT_WAIT)?;
        if words[0] != 0 || words[2] != 0 {
            return Err(format!("a completion call answered {words:?}"));
        }
        answered.extend(answers(&got)?);
    }
    answered.sort_by_key(|(tag, _)| *tag);
    match answered.as_slice() {
        [(0, Got::Data(data)), (1, Got::Wrote(n))] if data.as_slice() == TEXT && *n == MORE.len() => {
            Ok(format!(
                "mux-restartee: the successor's first completion call opened its own session; read {} bytes, wrote {n}\n",
                data.len()
            ))
        }
        other => Err(format!("answers {other:?}")),
    }
}

/// A completion call the server may hold `hold` µs: its reply words and the answers' bytes.
fn collect(ep: &Endpoint, hold: u64) -> Result<(Words, Vec<u8>), String> {
    let lend = Buffer::new(1).map_err(|e| format!("buffer: {e:?}"))?;
    let outcome = ep.call(&collect_words(hold), &[], Some(lend), FOREVER);
    let (reply, buffer) = outcome.into_result().map_err(|e| format!("completion call: {e:?}"))?;
    let n = (reply.words[1] as usize).min(buffer.as_ref().map_or(0, |b| b.len()));
    Ok((reply.words, buffer.map_or_else(Vec::new, |b| b[..n].to_vec())))
}

/// Sends `requests` end to end in one page.
fn send(ep: &Endpoint, requests: &[Message<'_>]) -> Result<(), String> {
    let mut page = Buffer::new(1).map_err(|e| format!("buffer: {e:?}"))?;
    let mut n = 0;
    for message in requests {
        n += message.encode(&mut page[n..]).map_err(|e| format!("encode: {e:?}"))?;
    }
    ep.send(&[0, n as u64, 0, 0], &[], Some(page), FOREVER).map_err(|(e, _)| format!("send: {e:?}"))
}

/// One answer: a read's data or a write's count.
#[derive(Debug)]
enum Got {
    Data(Vec<u8>),
    Wrote(usize),
}

/// The R-messages of a completion call's answer, by tag.
fn answers(mut bytes: &[u8]) -> Result<Vec<(u16, Got)>, String> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let size = message_size(bytes).map_err(|e| format!("an answer does not frame: {e:?}"))?;
        let message =
            Message::decode(&bytes[..size]).map_err(|e| format!("an answer does not decode: {e:?}"))?;
        out.push(match message.body {
            Body::Rread { data } => (message.tag, Got::Data(data.to_vec())),
            Body::Rwrite { count } => (message.tag, Got::Wrote(count as usize)),
            other => return Err(format!("tag {} answered {other:?}", message.tag)),
        });
        bytes = &bytes[size..];
    }
    Ok(out)
}
