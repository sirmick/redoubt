//! `fsd-client`: a `servers` entry that uses an `fsd` started by `init` through the root badges
//! its entry is handed, and prints its verdict on its console. Its first argument names what it
//! checks, and the next the endpoints of the badges it uses:
//!
//! - `boot ENDPOINT`: writes, reads, renames and removes a file and a directory through 9P.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_client::file::{Connection, File};
use redoubt_client::fsd::rename;
use redoubt_init_programs::Out;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::{DMDIR, mode};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let mut args = startup.args();
    let checked = match (args.next(), args.next()) {
        (Some("boot"), Some(at)) => boot(startup, &mut out, at),
        (check, _) => Err(format!("no such check, or no endpoint: {check:?}")),
    };
    let line = match checked {
        Ok(()) => String::from("fsd-client TEST PASSED\n"),
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
