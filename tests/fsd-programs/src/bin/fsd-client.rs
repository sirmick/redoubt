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
//! - `corrupt ENDPOINT`: every attach is refused, and `fsd` still answers the next.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_client::Error;
use redoubt_client::file::{Connection, File};
use redoubt_client::fsd::rename;
use redoubt_init_programs::Out;
use redoubt_rt::handle::Endpoint;
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
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut args = startup.args();
    let checked = match (args.next(), args.next()) {
        (Some("boot"), Some(at)) => boot(startup, &mut out, at).map(|()| Ends::Passed),
        (Some("reboot"), Some(at)) => reboot(startup, &mut out, at),
        (Some("read"), Some(at)) => read(startup, &mut out, at, args).map(|()| Ends::Passed),
        (Some("corrupt"), Some(at)) => corrupt(startup, &mut out, at).map(|()| Ends::Passed),
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

fn read_file(conn: &Connection, out: &mut Out, path: &str) -> Result<Vec<u8>, String> {
    let file = conn.open(&mut out.lend, path, mode::OREAD).map_err(|e| format!("open {path}: {e:?}"))?;
    let mut got = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = file
            .read_at(&mut out.lend, got.len() as u64, &mut chunk)
            .map_err(|e| format!("read {path}: {e:?}"))?;
        if n == 0 {
            break;
        }
        got.extend_from_slice(&chunk[..n]);
    }
    file.close(&mut out.lend).map_err(|e| format!("clunk {path}: {e:?}"))?;
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
    if read_file(&conn, out, "/hello")? != text {
        return Err("hello does not read back".into());
    }
    conn.create(&mut out.lend, "/", "d", DMDIR | 0o755, mode::OREAD)
        .and_then(|d| d.close(&mut out.lend))
        .map_err(|e| format!("mkdir d: {e:?}"))?;
    let (root, d) = (dir(&conn, out, "/")?, dir(&conn, out, "/d")?);
    rename(&mut out.lend, &root, "hello", &d, "moved").map_err(|e| format!("rename: {e:?}"))?;
    if read_file(&conn, out, "/d/moved")? != text {
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
    let starts = match read_file(&conn, out, "/starts") {
        Ok(text) => {
            core::str::from_utf8(&text).ok().and_then(|t| t.parse().ok()).ok_or("starts holds no count")?
        }
        Err(_) => 0u32,
    };
    let notes = b"kept across a reboot\n";
    if starts >= STARTS_BEFORE_REBOOT {
        let kept = conn.stat(&mut out.lend, "/kept").map_err(|e| format!("stat kept: {e:?}"))?;
        let file = conn.stat(&mut out.lend, "/kept/notes").map_err(|e| format!("stat kept/notes: {e:?}"))?;
        let noted = read_file(&conn, out, "/qids")?;
        let now = format!("{} {}", kept.qid.path, file.qid.path);
        if noted != now.as_bytes() {
            return Err(format!("qid paths {now}, noted {:?}", String::from_utf8_lossy(&noted)));
        }
        if read_file(&conn, out, "/kept/notes")? != notes {
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
        let got = read_file(&conn, out, path)?;
        if got != want.as_bytes() {
            return Err(format!("{path} holds {:?}, not {want:?}", String::from_utf8_lossy(&got)));
        }
        read += 1;
    }
    out.say(&format!("fsd-client read {read} files\n")).map_err(|e| format!("say: {e:?}"))
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
