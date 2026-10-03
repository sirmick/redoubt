//! `fsd-client`: a `servers` entry that uses an `fsd` started by `init` through the root badges
//! its entry is handed, and prints its verdict on its console. Its first argument names what it
//! checks, and the next the endpoints of the badges it uses:
//!
//! - `boot ENDPOINT`: writes, reads, renames and removes a file and a directory through 9P.
//! - `reboot ENDPOINT`: counts its starts in a file on the volume. The first writes a directory and a file
//!   and notes their qid paths; it and the next five exit with [`REBOOT_EXIT`], so `init`'s sixth exit notice
//!   reboots the machine (servers/init.md, "Restarts and reboots"). The start after the reboot finds six and
//!   reads both back, with the same qid paths.
//! - `read ENDPOINT PATH TEXT...`: each path holds exactly the text after it.
//! - `apart ENDPOINT ENDPOINT`: writes a file of the same name through each, and reads each back.
//! - `corrupt ENDPOINT`: every attach is refused, and `fsd` still answers the next.
//! - `quota ENDPOINT`: mints two roots with a quota each; one fills its quota, and the other still writes.
//! - `restart ENDPOINT PROBE`: writes a file, walks to `PROBE`, which ends an `fsd` built with its test-only
//!   feature `restart-probe`, and reads the file back through a fresh connection.
//! - `labelled ENDPOINT [OWN PEER]`, `outsider ENDPOINT OWN PEER`: under the volume's labels a file is
//!   written, and read back unchanged by the next start; without them the attach is refused. With `OWN` and
//!   `PEER`, the endpoints each receives on and is handed at the other's, the next start sends on `PEER` and
//!   reads back only once the outsider, which waits for that send, was refused and sent back. `labelled` runs
//!   labelled, so it cannot write the console: it exits with its verdict ([`verdict`]), which `init` reports.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_client::file::{Connection, File};
use redoubt_client::fsd::rename;
use redoubt_client::{Error, Lend};
use redoubt_init_programs::Out;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::{DMDIR, mode};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// What a check ends in: a verdict, or an exit its case expects `init` to report.
enum Ends {
    Passed,
    Exit(u32),
}

/// The code `reboot` exits with before the reboot.
const REBOOT_EXIT: u32 = 7;
/// The starts `init` allows before an exit reboots: five restarts, so six starts.
const STARTS_BEFORE_REBOOT: u32 = 6;

fn run(startup: &Startup) -> u32 {
    // A labelled check has no console to say anything on.
    let mut first = startup.args();
    if let (Some("labelled"), Some(at)) = (first.next(), first.next()) {
        return labelled_run(startup, at, first.next().zip(first.next()));
    }
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut args = startup.args();
    let checked = match (args.next(), args.next()) {
        (Some("boot"), Some(at)) => boot(startup, &mut out, at).map(|()| Ends::Passed),
        (Some("reboot"), Some(at)) => reboot(startup, &mut out, at),
        (Some("read"), Some(at)) => read(startup, &mut out, at, args).map(|()| Ends::Passed),
        (Some("apart"), Some(at)) => apart(startup, &mut out, at, args.next()).map(|()| Ends::Passed),
        (Some("corrupt"), Some(at)) => corrupt(startup, &mut out, at).map(|()| Ends::Passed),
        (Some("quota"), Some(at)) => quota(startup, &mut out, at).map(|()| Ends::Passed),
        (Some("outsider"), Some(at)) => {
            outsider(startup, &mut out, at, args.next().zip(args.next())).map(|()| Ends::Passed)
        }
        (Some("restart"), Some(at)) => restart(startup, &mut out, at, args.next()).map(|()| Ends::Passed),
        (check, _) => Err(format!("no such check, or no endpoint: {check:?}")),
    };
    let line = match checked {
        Ok(Ends::Exit(code)) => return code,
        Ok(Ends::Passed) => String::from("fsd-client TEST PASSED\n"),
        Err(why) => format!("fsd-client TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

/// The connection at the root badge the entry is handed at `endpoint`.
fn attach(startup: &Startup, out: &mut Out, endpoint: &str) -> Result<Connection, String> {
    let handle = startup.handle(endpoint).ok_or_else(|| format!("no {endpoint} handle"))?;
    Connection::attach(Endpoint::from_handle(handle), &mut out.lend).map_err(|e| format!("attach: {e:?}"))
}

/// Replaces `path`'s bytes with `data`, creating it if it is not there.
fn put(conn: &Connection, out: &mut Out, dir: &str, name: &str, data: &[u8]) -> Result<(), String> {
    let path = if dir == "/" { format!("/{name}") } else { format!("{dir}/{name}") };
    let Ok(file) = conn.open(&mut out.lend, &path, mode::OWRITE | mode::OTRUNC) else {
        return write_file(conn, out, dir, name, data);
    };
    let n = file.write_at(&mut out.lend, 0, data).map_err(|e| format!("write {path}: {e:?}"))?;
    if n != data.len() {
        return Err(format!("wrote {n} of {} bytes to {path}", data.len()));
    }
    file.close(&mut out.lend).map_err(|e| format!("clunk {path}: {e:?}"))
}

fn write_file(conn: &Connection, out: &mut Out, dir: &str, name: &str, data: &[u8]) -> Result<(), String> {
    let file = conn
        .create(&mut out.lend, dir, name, 0o644, mode::OWRITE)
        .map_err(|e| format!("create {dir} {name}: {e:?}"))?;
    let n = file.write_at(&mut out.lend, 0, data).map_err(|e| format!("write {name}: {e:?}"))?;
    if n != data.len() {
        return Err(format!("wrote {n} of {} bytes to {name}", data.len()));
    }
    file.close(&mut out.lend).map_err(|e| format!("clunk {name}: {e:?}"))
}

fn read_file(conn: &Connection, lend: &mut Lend, path: &str) -> Result<Vec<u8>, String> {
    let file = conn.open(lend, path, mode::OREAD).map_err(|e| format!("open {path}: {e:?}"))?;
    let mut got = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n =
            file.read_at(lend, got.len() as u64, &mut chunk).map_err(|e| format!("read {path}: {e:?}"))?;
        if n == 0 {
            break;
        }
        got.extend_from_slice(&chunk[..n]);
    }
    file.close(lend).map_err(|e| format!("clunk {path}: {e:?}"))?;
    Ok(got)
}

fn dir(conn: &Connection, out: &mut Out, path: &str) -> Result<File, String> {
    conn.open(&mut out.lend, path, mode::OREAD).map_err(|e| format!("open {path}: {e:?}"))
}

/// `fsd-boot`: the volume `fsd` formatted takes a file and a directory, the file reads back, is
/// renamed into the directory and reads back there, and both are removed.
fn boot(startup: &Startup, out: &mut Out, endpoint: &str) -> Result<(), String> {
    let conn = attach(startup, out, endpoint)?;
    let text = b"written through fsd under init\n";
    write_file(&conn, out, "/", "hello", text)?;
    if read_file(&conn, &mut out.lend, "/hello")? != text {
        return Err("hello does not read back".into());
    }
    conn.create(&mut out.lend, "/", "d", DMDIR | 0o755, mode::OREAD)
        .and_then(|d| d.close(&mut out.lend))
        .map_err(|e| format!("mkdir d: {e:?}"))?;
    let (root, d) = (dir(&conn, out, "/")?, dir(&conn, out, "/d")?);
    rename(&mut out.lend, &root, "hello", &d, "moved").map_err(|e| format!("rename: {e:?}"))?;
    if read_file(&conn, &mut out.lend, "/d/moved")? != text {
        return Err("d/moved does not read back".into());
    }
    if conn.stat(&mut out.lend, "/hello").is_ok() {
        return Err("hello is still there after the rename".into());
    }
    for f in [root, d] {
        f.close(&mut out.lend).map_err(|e| format!("clunk: {e:?}"))?;
    }
    conn.remove(&mut out.lend, "/d/moved").map_err(|e| format!("remove d/moved: {e:?}"))?;
    conn.remove(&mut out.lend, "/d").map_err(|e| format!("remove d: {e:?}"))?;
    if conn.stat(&mut out.lend, "/d").is_ok() {
        return Err("d is still there after its remove".into());
    }
    out.say("fsd-client wrote, read, renamed and removed\n").map_err(|e| format!("say: {e:?}"))
}

/// `fsd-reboot`: what the first boot writes is there after the reboot, with the same qid paths.
fn reboot(startup: &Startup, out: &mut Out, endpoint: &str) -> Result<Ends, String> {
    let conn = attach(startup, out, endpoint)?;
    let starts = match read_file(&conn, &mut out.lend, "/starts") {
        Ok(text) => {
            core::str::from_utf8(&text).ok().and_then(|t| t.parse().ok()).ok_or("starts holds no count")?
        }
        Err(_) => 0u32,
    };
    let notes = b"kept across a reboot\n";
    if starts >= STARTS_BEFORE_REBOOT {
        let kept = conn.stat(&mut out.lend, "/kept").map_err(|e| format!("stat kept: {e:?}"))?;
        let file = conn.stat(&mut out.lend, "/kept/notes").map_err(|e| format!("stat kept/notes: {e:?}"))?;
        let noted = read_file(&conn, &mut out.lend, "/qids")?;
        let now = format!("{} {}", kept.qid.path, file.qid.path);
        if noted != now.as_bytes() {
            return Err(format!("qid paths {now}, noted {:?}", String::from_utf8_lossy(&noted)));
        }
        if read_file(&conn, &mut out.lend, "/kept/notes")? != notes {
            return Err("kept/notes does not read back".into());
        }
        out.say(&format!("fsd-client read back kept and kept/notes after the reboot, qid paths {now}\n"))
            .map_err(|e| format!("say: {e:?}"))?;
        return Ok(Ends::Passed);
    }
    if starts == 0 {
        conn.create(&mut out.lend, "/", "kept", DMDIR | 0o755, mode::OREAD)
            .and_then(|d| d.close(&mut out.lend))
            .map_err(|e| format!("mkdir kept: {e:?}"))?;
        write_file(&conn, out, "/kept", "notes", notes)?;
        let kept = conn.stat(&mut out.lend, "/kept").map_err(|e| format!("stat kept: {e:?}"))?;
        let file = conn.stat(&mut out.lend, "/kept/notes").map_err(|e| format!("stat kept/notes: {e:?}"))?;
        let qids = format!("{} {}", kept.qid.path, file.qid.path);
        write_file(&conn, out, "/", "qids", qids.as_bytes())?;
        out.say(&format!("fsd-client wrote kept and kept/notes, qid paths {qids}\n"))
            .map_err(|e| format!("say: {e:?}"))?;
    }
    put(&conn, out, "/", "starts", format!("{}", starts + 1).as_bytes())?;
    Ok(Ends::Exit(REBOOT_EXIT))
}

/// `image-disk`: each path holds exactly the text after it.
fn read<'a>(
    startup: &Startup,
    out: &mut Out,
    endpoint: &str,
    mut pairs: impl Iterator<Item = &'a str>,
) -> Result<(), String> {
    let conn = attach(startup, out, endpoint)?;
    let mut read = 0;
    while let Some(path) = pairs.next() {
        let want = pairs.next().ok_or_else(|| format!("no text for {path}"))?;
        let got = read_file(&conn, &mut out.lend, path)?;
        if got != want.as_bytes() {
            return Err(format!("{path} holds {:?}, not {want:?}", String::from_utf8_lossy(&got)));
        }
        read += 1;
    }
    out.say(&format!("fsd-client read {read} files\n")).map_err(|e| format!("say: {e:?}"))
}

/// `fsd-one-volume`: two `fsd`s, each on a volume of its own, hold a file of the same name
/// apart.
fn apart(startup: &Startup, out: &mut Out, first: &str, second: Option<&str>) -> Result<(), String> {
    let second = second.ok_or("no second endpoint")?;
    let mut conns = Vec::new();
    for at in [first, second] {
        let conn = attach(startup, out, at)?;
        write_file(&conn, out, "/", "which", at.as_bytes())?;
        conns.push(conn);
    }
    for (conn, at) in conns.iter().zip([first, second]) {
        if read_file(conn, &mut out.lend, "/which")? != at.as_bytes() {
            return Err(format!("{at}'s which does not read back"));
        }
    }
    out.say(&format!("fsd-client read {first}'s which and {second}'s apart\n"))
        .map_err(|e| format!("say: {e:?}"))
}

/// `fsd-corrupt-volume`: a volume served as corrupt refuses each attach with an `Rerror`, and
/// the same instance answers again.
fn corrupt(startup: &Startup, out: &mut Out, endpoint: &str) -> Result<(), String> {
    let handle = startup.handle(endpoint).ok_or_else(|| format!("no {endpoint} handle"))?;
    for _ in 0..3 {
        match Connection::attach(Endpoint::from_handle(handle), &mut out.lend) {
            Err(Error::Rerror) => {}
            Err(e) => return Err(format!("attach: {e:?}, not Rerror")),
            Ok(_) => return Err("a corrupt volume was attached".into()),
        }
    }
    out.say("fsd-client was refused at each of 3 attaches\n").map_err(|e| format!("say: {e:?}"))
}

/// Each root's quota in `quota`, in bytes.
const QUOTA: u64 = 64 * 1024;

/// `fsd-quota` (R48): two roots minted at one volume with [`QUOTA`] each; one writes until a
/// write is refused, within its quota, and the other still writes half a quota.
fn quota(startup: &Startup, out: &mut Out, endpoint: &str) -> Result<(), String> {
    let base = attach(startup, out, endpoint)?;
    let mut roots = Vec::new();
    for name in ["hog", "saver"] {
        base.create(&mut out.lend, "/", name, DMDIR | 0o755, mode::OREAD)
            .and_then(|d| d.close(&mut out.lend))
            .map_err(|e| format!("mkdir {name}: {e:?}"))?;
        let (minted, _) = base
            .new_connection(&mut out.lend, &format!("/{name}"), QUOTA)
            .map_err(|e| format!("mint {name}: {e:?}"))?;
        roots.push(Connection::attach(minted, &mut out.lend).map_err(|e| format!("attach {name}: {e:?}"))?);
    }
    let chunk = [7u8; 4096];
    let hog = roots[0]
        .create(&mut out.lend, "/", "full", 0o644, mode::OWRITE)
        .map_err(|e| format!("create full: {e:?}"))?;
    let mut filled = 0u64;
    loop {
        match hog.write_at(&mut out.lend, filled, &chunk) {
            Ok(0) => return Err("a write took nothing".into()),
            Ok(n) => filled += n as u64,
            Err(Error::Rerror) => break,
            Err(e) => return Err(format!("write full: {e:?}")),
        }
        if filled > QUOTA {
            return Err(format!("wrote {filled} bytes past a quota of {QUOTA}"));
        }
    }
    let half = [9u8; 4096];
    let saver = roots[1]
        .create(&mut out.lend, "/", "kept", 0o644, mode::OWRITE)
        .map_err(|e| format!("create kept: {e:?}"))?;
    // A write may take less than it is given (the lend bounds it); each goes on where it ended.
    let mut kept = 0u64;
    while kept < QUOTA / 2 {
        match saver
            .write_at(&mut out.lend, kept, &half[..half.len().min((QUOTA / 2 - kept) as usize)])
            .map_err(|e| format!("write kept at {kept}: {e:?}"))?
        {
            0 => return Err(format!("a write to kept at {kept} took nothing")),
            n => kept += n as u64,
        }
    }
    out.say(&format!(
        "fsd-client filled one root at {filled} bytes, and the other still wrote {}\n",
        QUOTA / 2
    ))
    .map_err(|e| format!("say: {e:?}"))
}

/// `fsd-restart`: a file written, then a walk to `probe` ends `fsd` with the call held, so the
/// call gets `Dead`; a fresh connection, to the instance `init` restarts on the same endpoint,
/// reads the file back.
fn restart(startup: &Startup, out: &mut Out, endpoint: &str, probe: Option<&str>) -> Result<(), String> {
    let probe = probe.ok_or("no probe name")?;
    let text = b"written before fsd's restart\n";
    let old = attach(startup, out, endpoint)?;
    write_file(&old, out, "/", "kept", text)?;
    match old.stat(&mut out.lend, &format!("/{probe}")) {
        Err(Error::Disconnected) => {}
        other => return Err(format!("the probe's walk got {other:?}, not Dead")),
    }
    let fresh = attach(startup, out, endpoint)?;
    if read_file(&fresh, &mut out.lend, "/kept")? != text {
        return Err("kept does not read back after the restart".into());
    }
    // One line, after the new instance answered: init's lines on the exit come before it.
    out.say("fsd-client's call got Dead, and a fresh connection read kept back\n")
        .map_err(|e| format!("say: {e:?}"))
}

/// `fsd-label-check`: a caller without the volume's labels is refused at its attach, a read of
/// the root, so it never holds a fid to walk, stat or write through. It tries once the labelled
/// client's next start, which found its file written, sends on `own`, and sends on `peer` after.
fn outsider(
    startup: &Startup,
    out: &mut Out,
    endpoint: &str,
    ends: Option<(&str, &str)>,
) -> Result<(), String> {
    let (own, peer) = ends.ok_or("no endpoints to wait and answer on")?;
    let own = Endpoint::from_handle(startup.handle(own).ok_or_else(|| format!("no {own} handle"))?);
    let peer = Endpoint::from_handle(startup.handle(peer).ok_or_else(|| format!("no {peer} handle"))?);
    while !matches!(own.receive(FOREVER, 0), Ok(Event::Send(_))) {}
    let handle = startup.handle(endpoint).ok_or_else(|| format!("no {endpoint} handle"))?;
    match Connection::attach(Endpoint::from_handle(handle), &mut out.lend) {
        Err(Error::Rerror) => {}
        Err(e) => return Err(format!("attach: {e:?}, not Rerror")),
        Ok(_) => return Err("attached without the volume's labels".into()),
    }
    out.say("fsd-client without the labels was refused at attach\n").map_err(|e| format!("say: {e:?}"))?;
    peer.send(&[0; 4], &[], None, FOREVER).map_err(|(e, _)| format!("send: {e:?}"))
}

/// The file `labelled` writes and reads back, and what it holds.
const SECRET: (&str, &[u8]) = ("/secret", b"written under the volume's labels\n");

/// What `labelled` exits with. A labelled program cannot write the unlabelled console
/// (servers/consoled.md), so its verdict is its exit code, which `init` reports.
mod verdict {
    pub const WROTE: u32 = 10;
    pub const READ_BACK: u32 = 11;
    pub const NO_HANDLE: u32 = 20;
    pub const ATTACH: u32 = 21;
    pub const WRITE: u32 = 22;
    pub const READ: u32 = 23;
    pub const OUTSIDER: u32 = 24;
}

/// `fsd-label-check`, `fsd-confined-labelled`: a caller whose labels equal the volume's writes a
/// file and exits; its next start reads the file back unchanged and exits again, with `ends` only
/// once the outsider was refused.
fn labelled_run(startup: &Startup, endpoint: &str, ends: Option<(&str, &str)>) -> u32 {
    let Ok(mut lend) = Lend::new(redoubt_init_programs::LEND_PAGES) else {
        return redoubt_init_programs::code::NO_LEND;
    };
    let Some(handle) = startup.handle(endpoint) else { return verdict::NO_HANDLE };
    let Ok(conn) = Connection::attach(Endpoint::from_handle(handle), &mut lend) else {
        return verdict::ATTACH;
    };
    let lend = &mut lend;
    if conn.stat(lend, SECRET.0).is_ok() {
        if let Some((own, peer)) = ends {
            let (Some(own), Some(peer)) = (startup.handle(own), startup.handle(peer)) else {
                return verdict::NO_HANDLE;
            };
            if Endpoint::from_handle(peer).send(&[0; 4], &[], None, FOREVER).is_err()
                || !matches!(Endpoint::from_handle(own).receive(FOREVER, 0), Ok(Event::Send(_)))
            {
                return verdict::OUTSIDER;
            }
        }
        return if read_file(&conn, lend, SECRET.0).as_deref() == Ok(SECRET.1) {
            verdict::READ_BACK
        } else {
            verdict::READ
        };
    }
    let Ok(file) = conn.create(lend, "/", &SECRET.0[1..], 0o644, mode::OWRITE) else { return verdict::WRITE };
    if file.write_at(lend, 0, SECRET.1) != Ok(SECRET.1.len()) || file.close(lend).is_err() {
        return verdict::WRITE;
    }
    verdict::WROTE
}
