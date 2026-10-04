//! `aio-reader`: the client of `aio-many-reads` (servers/serving.md, "Multiplexed connections";
//! userland/native.md, "Many requests at once"), a `servers` entry under the real `init` that
//! reads an `fsd` file through the client library's hub. Its first argument names its part:
//!
//! - `burst ENDPOINT OWN PEER [two]`: writes `/aio`, then keeps 64 reads of it outstanding on one thread,
//!   sent in one batch and left uncollected while it tells the second program (a send on `PEER`) and waits on
//!   `OWN` for that program's read to be answered. Then it collects them all. With `two`, the reads are
//!   spread over two connections (the second minted from the first), each with a waiter thread whose
//!   completion call is always parked, so they are answered as they come, and the thread idles in `receive`
//!   on `OWN`; without, it has one connection and waits in its completion call itself.
//! - `second ENDPOINT OWN PEER`: waits on `OWN` for the burst, reads `/aio` by one call per request while the
//!   burst's 64 reads are held at `fsd`, and sends its result to `PEER`.
//!
//! Each prints its verdict on its console, and the burst, the case's reporter, `TEST PASSED`; it
//! prints how many threads it ran: itself and its waiters.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_client::aio::{COMPLETION_PAGES, Conn, Hub, Outcome};
use redoubt_client::file::Connection;
use redoubt_init_programs::Out;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::ninep::Body;

redoubt_rt::entry!(run);

/// Reads kept outstanding at once.
const READS: usize = 64;
/// The bytes each reads: `/aio` holds `READS` of them.
const EACH: usize = 64;
/// How long the burst waits for the second program's answer (µs).
const SECOND_WAIT: u64 = 20_000_000;
/// Word 0 of the burst's send to the second program, and of its answer.
const GO: u64 = 1;
const READ_OK: u64 = 2;
/// The badge the burst's waiters wake it with.
const WAITER_BADGE: u64 = 0x5a17;

/// The byte at `at` of `/aio`.
fn byte(at: usize) -> u8 { (at % 251) as u8 }

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut args = startup.args();
    // The burst is the case's reporter: only its console says TEST PASSED.
    let line = match (args.next(), args.next(), args.next(), args.next()) {
        (Some("burst"), Some(at), Some(own), Some(peer)) => {
            match burst(startup, &mut out, at, own, peer, args.next() == Some("two")) {
                Ok(verdict) => format!("aio-reader {verdict}\naio-reader TEST PASSED\n"),
                Err(why) => format!("aio-reader TEST FAILED: {why}\n"),
            }
        }
        (Some("second"), Some(at), Some(own), Some(peer)) => match second(startup, &mut out, at, own, peer) {
            Ok(verdict) => format!("aio-reader {verdict}\n"),
            Err(why) => format!("aio-reader TEST FAILED: {why}\n"),
        },
        (part, ..) => format!("aio-reader TEST FAILED: no such part, or no endpoints: {part:?}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

fn endpoint(startup: &Startup, name: &str) -> Result<Endpoint, String> {
    startup.handle(name).map(Endpoint::from_handle).ok_or_else(|| format!("no {name} handle"))
}

/// A connection on `endpoint` with `/aio` open for reading: its fid.
fn open_aio(conn: &Connection, out: &mut Out) -> Result<u32, String> {
    let file = conn.open(&mut out.lend, "/aio", mode::OREAD).map_err(|e| format!("open /aio: {e:?}"))?;
    let fid = file.fid();
    // The fid stays open for the hub's reads; its file is never closed, so it is never clunked.
    core::mem::forget(file);
    Ok(fid)
}

fn burst(
    startup: &Startup,
    out: &mut Out,
    at: &str,
    own: &str,
    peer: &str,
    two: bool,
) -> Result<String, String> {
    let (own, peer) = (endpoint(startup, own)?, endpoint(startup, peer)?);
    let first = endpoint(startup, at)?;
    let conn = Connection::attach(Endpoint::from_handle(first.handle()), &mut out.lend)
        .map_err(|e| format!("attach: {e:?}"))?;
    let data: Vec<u8> = (0..READS * EACH).map(byte).collect();
    let file = conn
        .create(&mut out.lend, "/", "aio", 0o644, mode::OWRITE)
        .map_err(|e| format!("create /aio: {e:?}"))?;
    // A write is as long as its lend allows: the rest goes in the next.
    let mut done = 0;
    while done < data.len() {
        let n = file
            .write_at(&mut out.lend, done as u64, &data[done..])
            .map_err(|e| format!("write /aio: {e:?}"))?;
        if n == 0 {
            return Err(format!("wrote {done} of {} bytes", data.len()));
        }
        done += n;
    }
    file.close(&mut out.lend).map_err(|e| format!("clunk /aio: {e:?}"))?;
    // Each connection with its fid of `/aio`: the first, and with `two` one minted from it.
    let mut conns = Vec::new();
    conns.push((first, open_aio(&conn, out)?));
    if two {
        let (minted, _) = conn.new_connection(&mut out.lend, "/", 0).map_err(|e| format!("mint: {e:?}"))?;
        let second = Connection::attach(Endpoint::from_handle(minted.handle()), &mut out.lend)
            .map_err(|e| format!("attach the second: {e:?}"))?;
        conns.push((minted, open_aio(&second, out)?));
    }
    let mut hub = Hub::new();
    let mut fids: Vec<(Conn, u32)> = Vec::new();
    for (i, (ep, fid)) in conns.into_iter().enumerate() {
        let c = hub.connect(ep).map_err(|e| format!("session: {e:?}"))?;
        if two {
            let badge = NonZeroU64::new(WAITER_BADGE + i as u64).ok_or("badge")?;
            hub.spawn_waiter(c, &own, badge).map_err(|e| format!("waiter: {e:?}"))?;
        }
        fids.push((c, fid));
    }
    // 64 reads in one batch, each of its own part of `/aio`, left uncollected.
    let buffers: Result<Vec<Buffer>, String> =
        (0..READS).map(|_| Buffer::new(1).map_err(|e| format!("buffer: {e:?}"))).collect();
    let mut tags = Vec::new();
    hub.batch(|hub| -> Result<(), String> {
        for (k, buffer) in buffers?.into_iter().enumerate() {
            let (c, fid) = fids[k % fids.len()];
            let read = Body::Tread { fid, offset: (k * EACH) as u64, count: EACH as u32 };
            let tag = hub.submit(c, read, Some(buffer)).map_err(|e| format!("submit {k}: {e:?}"))?;
            tags.push((c, tag, k));
        }
        Ok(())
    })?;
    // The second program reads while all 64 are held at `fsd`.
    peer.send(&[GO, 0, 0, 0], &[], None, FOREVER).map_err(|(e, _)| format!("go: {e:?}"))?;
    let second_read = loop {
        match own.receive(SECOND_WAIT, COMPLETION_PAGES) {
            Ok(Event::Send(delivery)) => match hub.deliver(delivery) {
                Some(d) if d.words[0] == READ_OK => break d.words[1] == 1,
                // A waiter's wake-up arrived first: the hub has its buffer.
                _ => continue,
            },
            Ok(_) => continue,
            Err(e) => return Err(format!("no word from the second program: {e:?}")),
        }
    };
    if !second_read {
        return Err(String::from("the second program's read failed"));
    }
    // Now collect them all: what the waiters already handed over first, then what is still to come.
    let mut answered = 0;
    loop {
        while let Some(done) = hub.completed() {
            let k = tags.iter().find(|(c, t, _)| *c == done.conn && *t == done.tag).map(|e| e.2);
            let (Some(k), Outcome::Read(n), Some(buffer)) = (k, &done.outcome, &done.buffer) else {
                return Err(format!("read {}: {:?}", done.tag, done.outcome));
            };
            if buffer[..*n] != data[k * EACH..(k + 1) * EACH] {
                return Err(format!("read {k} brought the wrong bytes"));
            }
            answered += 1;
        }
        if answered == READS {
            break;
        }
        if !two {
            hub.wait(fids[0].0, 1_000_000).map_err(|e| format!("wait: {e:?}"))?;
        } else if let Event::Send(delivery) =
            own.receive(SECOND_WAIT, COMPLETION_PAGES).map_err(|e| format!("receive: {e:?}"))?
        {
            let _ = hub.deliver(delivery);
        }
    }
    Ok(format!(
        "burst: {answered} of {READS} reads answered on {} connection(s); the second program read during the burst; 1 thread, {} waiters",
        fids.len(),
        hub.waiters()
    ))
}

fn second(startup: &Startup, out: &mut Out, at: &str, own: &str, peer: &str) -> Result<String, String> {
    let (own, peer) = (endpoint(startup, own)?, endpoint(startup, peer)?);
    while !matches!(own.receive(FOREVER, 0), Ok(Event::Send(d)) if d.words[0] == GO) {}
    let conn =
        Connection::attach(endpoint(startup, at)?, &mut out.lend).map_err(|e| format!("attach: {e:?}"))?;
    let file = conn.open(&mut out.lend, "/aio", mode::OREAD).map_err(|e| format!("open: {e:?}"))?;
    let mut got = [0u8; EACH];
    let n = file.read_at(&mut out.lend, 0, &mut got).map_err(|e| format!("read: {e:?}"))?;
    let ok = n == EACH && got.iter().enumerate().all(|(i, b)| *b == byte(i));
    peer.send(&[READ_OK, u64::from(ok), 0, 0], &[], None, FOREVER)
        .map_err(|(e, _)| format!("answer: {e:?}"))?;
    if !ok {
        return Err(format!("read {n} bytes, not /aio's first {EACH}"));
    }
    Ok(String::from("second: read answered during the burst"))
}
