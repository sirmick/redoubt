//! `walfsd-client`: a `servers` entry that uses a `walfsd` started by `init` through the root badge
//! its entry is handed, and prints its verdict on its console, for the checks of `walfsd`'s own
//! cases; the cases that mirror `littlefsd`'s, the quota's among them, run `littlefsd-client`, a
//! client of either server.
//! Its first argument names what it checks, and the next the endpoint of the badge it uses:
//!
//! - `cut ENDPOINT`: writes a file of three blocks, arms `walfsd`'s cut (its test-only feature
//!   `cut-after-write`) at a block write drawn from the kernel's randomness, then rewrites the file and
//!   renames it; `walfsd` ends at the cut, the call in flight gets `Dead` and the old connection's next one
//!   is refused, and through a fresh connection to the restarted one the file is as before the operation the
//!   cut fell in or as after it, never between.
//! - `flipped ENDPOINT DAMAGED WHOLE TEXT`: `DAMAGED`, whose data block the case's disk had a bit flipped in
//!   after the pack, is `corrupt` read over 9P and copied by the typed call; `WHOLE` still reads back as
//!   `TEXT`.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_client::file::Connection;
use redoubt_client::littlefsd::{copy_file, rename};
use redoubt_client::{Error, Lend, Name};
use redoubt_init_programs::Out;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::littlefsd::ErrorCode;

redoubt_rt::entry!(run);

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut args = startup.args();
    let checked = match (args.next(), args.next()) {
        (Some("cut"), Some(at)) => cut(startup, &mut out, at),
        (Some("flipped"), Some(at)) => flipped(startup, &mut out, at, args),
        (check, _) => Err(format!("no such check, or no endpoint: {check:?}")),
    };
    let line = match checked {
        Ok(()) => String::from("walfsd-client TEST PASSED\n"),
        Err(why) => format!("walfsd-client TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

/// The connection at the root badge the entry is handed at `endpoint`.
fn attach(startup: &Startup, lend: &mut Lend, endpoint: &str) -> Result<Connection, String> {
    let handle = startup.handle(endpoint).ok_or_else(|| format!("no {endpoint} handle"))?;
    Connection::attach(Endpoint::from_handle(handle), lend).map_err(|e| format!("attach: {e:?}"))
}

/// What the file at `path` holds, read whole.
fn read_file(conn: &Connection, lend: &mut Lend, path: &str) -> Result<Vec<u8>, Error> {
    let file = conn.open(lend, path, mode::OREAD)?;
    let mut got = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        match file.read_at(lend, got.len() as u64, &mut chunk) {
            Ok(0) => break,
            Ok(n) => got.extend_from_slice(&chunk[..n]),
            Err(e) => {
                let _ = file.close(lend);
                return Err(e);
            }
        }
    }
    file.close(lend)?;
    Ok(got)
}

/// The bytes `cut` rewrites: three blocks, one transaction (servers/walfsd.md, "Atomicity").
const SPAN: usize = 3 * 4096;
/// The block writes the rewrite and the rename take on the bench's fresh 4 MiB volume:
/// `CUT_WRITES` in servers/walfsd/src/server_tests.rs, where a host test counts them. The cut is
/// drawn from 1 to this, so it always falls inside the operations.
const CUT_WRITES: u64 = 20;
/// Pages lent for the rewrite: its three blocks in one `Twrite`.
const SPAN_PAGES: usize = 4;

/// `walfsd-power-loss` (R50): `f` holds three blocks of `a`; the cut is armed at block write N,
/// then `f` is rewritten with `b` in one `Twrite` and renamed to `g`. `walfsd` ends at the cut;
/// through a fresh connection to the instance `init` restarts, `f` holds `a` (before the write),
/// `f` holds `b` (after it, before the rename), or `g` holds `b` (after both): the same file, the
/// same length, never a mix of the two contents.
fn cut(startup: &Startup, out: &mut Out, endpoint: &str) -> Result<(), String> {
    let mut lend = Lend::new(SPAN_PAGES).map_err(|e| format!("lend: {e:?}"))?;
    let lend = &mut lend;
    let conn = attach(startup, lend, endpoint)?;
    let f = conn.create(lend, "/", "f", 0o644, mode::OWRITE).map_err(|e| format!("create f: {e:?}"))?;
    if f.write_at(lend, 0, &[b'a'; SPAN]) != Ok(SPAN) {
        return Err("f was not written whole in one Twrite".into());
    }
    let ino = f.stat(lend).map_err(|e| format!("stat f: {e:?}"))?.qid.path;
    f.close(lend).map_err(|e| format!("clunk f: {e:?}"))?;
    let root = conn.open(lend, "/", mode::OREAD).map_err(|e| format!("open /: {e:?}"))?;
    let f = conn.open(lend, "/f", mode::OWRITE).map_err(|e| format!("open f: {e:?}"))?;
    let n = 1 + redoubt_rt::handle::random_u64().map_err(|e| format!("random: {e:?}"))? % CUT_WRITES;
    // The walk arms the cut, and finds nothing.
    match conn.stat(lend, &format!("/walfsd-cut-{n}")) {
        Err(Error::Rerror(Name::NotFound)) => {}
        other => return Err(format!("arming the cut got {other:?}")),
    }
    let wrote = f.write_at(lend, 0, &[b'b'; SPAN]);
    let renamed = rename(lend, &root, "f", &root, "g");
    if wrote.err() != Some(Error::Disconnected) && renamed.err() != Some(Error::Disconnected) {
        return Err(format!("no cut at write {n}: neither the write nor the rename got Dead"));
    }
    // The old connection was the ended instance's: the restarted one knows none of its fids and
    // refuses its next call, so only a fresh connection reads.
    if let Ok(stat) = conn.stat(lend, "/f") {
        return Err(format!("the old connection outlived its instance: {stat:?}"));
    }
    let fresh = attach(startup, lend, endpoint)?;
    let (at_f, at_g) = (read_file(&fresh, lend, "/f"), read_file(&fresh, lend, "/g"));
    let (state, path) = match (at_f.as_deref(), at_g.as_deref()) {
        (Ok(a), Err(_)) if a == [b'a'; SPAN] => ("before", "/f"),
        (Ok(b), Err(_)) if b == [b'b'; SPAN] => ("written", "/f"),
        (Err(_), Ok(b)) if b == [b'b'; SPAN] => ("renamed", "/g"),
        _ => return Err(format!("cut at write {n}: neither before nor after: f {at_f:?}, g {at_g:?}")),
    };
    let stat = fresh.stat(lend, path).map_err(|e| format!("stat {path}: {e:?}"))?;
    if stat.qid.path != ino || stat.length != SPAN as u64 {
        return Err(format!("{path} is inode {} of {} bytes, not inode {ino}", stat.qid.path, stat.length));
    }
    out.say(&format!("walfsd-client cut after block write {n}: found {state}, the same file\n"))
        .map_err(|e| format!("say: {e:?}"))
}

/// `walfsd-flipped-block` (R49): the damaged file reads as `corrupt`, and its copy too:
/// walfsd's word from the block's hash; the whole file reads back.
fn flipped<'a>(
    startup: &Startup,
    out: &mut Out,
    endpoint: &str,
    mut args: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    let (damaged, whole, text) = match (args.next(), args.next(), args.next()) {
        (Some(d), Some(w), Some(t)) => (d, w, t),
        _ => return Err("no damaged file, whole file and text".into()),
    };
    let lend = &mut out.lend;
    let conn = attach(startup, lend, endpoint)?;
    match read_file(&conn, lend, damaged) {
        Err(Error::Rerror(Name::Corrupt)) => {}
        other => return Err(format!("{damaged} read as {other:?}")),
    }
    let src = conn.open(lend, damaged, mode::OREAD).map_err(|e| format!("open {damaged}: {e:?}"))?;
    let root = conn.open(lend, "/", mode::OREAD).map_err(|e| format!("open /: {e:?}"))?;
    match copy_file(lend, &src, &root, "copy") {
        Err(Error::Server(code)) if code == ErrorCode::Corrupt.code() => {}
        other => return Err(format!("the copy of {damaged} got {other:?}, not corrupt")),
    }
    let got = read_file(&conn, lend, whole).map_err(|e| format!("read {whole}: {e:?}"))?;
    if got != text.as_bytes() {
        return Err(format!("{whole} holds {:?}, not {text:?}", String::from_utf8_lossy(&got)));
    }
    out.say(&format!("walfsd-client: {damaged} is corrupt, read and copied, and {whole} reads back\n"))
        .map_err(|e| format!("say: {e:?}"))
}
