//! `restart-client`: a `servers` entry that calls `restartee` through the badge the manifest
//! hands it, across the restart `restart-faulter` causes, and prints its verdict on its own
//! console; it is unlabelled, since only an unlabelled caller may write the console
//! (servers/consoled.md R69). What it judges is the system's answers: the kernel's `Dead`, the
//! new instance's refusal of an id the old one minted, `consoled`'s refusal of the dead
//! instance's console connection, and the serving library's malformed answer to a reply the
//! kernel rejected.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use redoubt_client::file::Connection;
use redoubt_client::{Error, Lend};
use redoubt_init_programs::restartee::{CONSOLE, MISREPLY, OK, PING, WAIT};
use redoubt_init_programs::{LEND_PAGES, Out};
use redoubt_rt::abi::{self, FOREVER, Handle};
use redoubt_rt::client;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// How long a release may take: a live server answers at once.
const TIMEOUT: u64 = 1_000_000;
/// The fid it writes restartee's console through.
const CONS_FID: u32 = 1;

/// Its console is opened only for the verdict, after the checks: `consoled` charges the files of
/// every console `init` minted to one share with `init`'s own, and that share has room for
/// `init`'s and one more program's, not also the fid on restartee's console.
fn run(startup: &Startup) -> u32 {
    let Ok(mut lend) = Lend::new(LEND_PAGES) else { return redoubt_init_programs::code::NO_LEND };
    let checked = check(startup, &mut lend);
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let line = match checked {
        Ok(()) => format!("restart-client TEST PASSED\n"),
        Err(why) => format!("restart-client TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

/// One of `restartee`'s own requests: the reply's words and the handle it brought, if any.
fn call(at: &Endpoint, opcode: u64) -> Result<([u64; 4], Option<Handle>), abi::Error> {
    let (reply, _) = at.call(&[opcode, 0, 0, 0], &[], None, FOREVER).into_result()?;
    let handle = reply.handles.as_slice().iter().flatten().next().copied();
    Ok((reply.words, handle))
}

fn check(startup: &Startup, lend: &mut Lend) -> Result<(), String> {
    let at = Endpoint::from_handle(startup.handle("restartee").ok_or("no restartee handle")?);
    let conn =
        Connection::attach(Endpoint::from_handle(at.handle()), lend).map_err(|e| format!("attach: {e:?}"))?;
    let (_, first) = conn.new_connection(lend, "", 0).map_err(|e| format!("mint: {e:?}"))?;
    let (_, second) = conn.new_connection(lend, "", 0).map_err(|e| format!("mint again: {e:?}"))?;
    // An id this instance minted is its own to release.
    conn.disconnect(first, TIMEOUT).map_err(|e| format!("release the first id: {e:?}"))?;

    let (_, copy) = call(&at, CONSOLE).map_err(|e| format!("ask for its console: {e:?}"))?;
    // A session of its own on the server's connection, and a fid in it.
    let cons =
        client::Connection::new(Endpoint::from_handle(copy.ok_or("its console came without a handle")?));
    cons.version(lend).map_err(|e| format!("version its console: {e:?}"))?;
    cons.attach(lend, CONS_FID, "").map_err(|e| format!("attach to its console: {e:?}"))?;
    let checked = across(&at, &cons, second, lend);
    if checked.is_err() {
        // Its fid would keep the verdict off the console.
        let _ = cons.clunk(lend, CONS_FID);
    }
    checked
}

/// The checks after the console copy is attached, through restartee's restart.
fn across(at: &Endpoint, cons: &client::Connection, second: u64, lend: &mut Lend) -> Result<(), String> {
    cons.open(lend, CONS_FID, mode::OWRITE).map_err(|e| format!("open its console: {e:?}"))?;
    cons.write(lend, CONS_FID, 0, b"restart-client writes through restartee's console\n")
        .map_err(|e| format!("write through its console: {e:?}"))?;

    let (words, _) = call(at, MISREPLY).map_err(|e| format!("misreply: {e:?}"))?;
    if words != MALFORMED {
        return Err(format!("a rejected reply came back as {words:?}, not malformed"));
    }
    if call(at, PING).map_err(|e| format!("ping after the misreply: {e:?}"))?.0 != OK {
        return Err("no answer after the misreply".into());
    }

    // Held until `restart-faulter`'s call arrives and restartee faults serving it.
    match call(at, WAIT) {
        Err(abi::Error::Dead) => {}
        other => return Err(format!("the call held at the fault ended {other:?}, not Dead")),
    }
    // Queued on the endpoint until the new instance takes it.
    if call(at, PING).map_err(|e| format!("ping the new instance: {e:?}"))?.0 != OK {
        return Err("the new instance did not answer".into());
    }
    let again = Connection::attach(Endpoint::from_handle(at.handle()), lend)
        .map_err(|e| format!("attach to the new instance: {e:?}"))?;
    match again.disconnect(second, TIMEOUT) {
        Err(Error::Rerror) => {}
        other => return Err(format!("the old instance's id was not refused: {other:?}")),
    }
    match cons.write(lend, CONS_FID, 0, b"restart-client writes through a dead console\n") {
        Err(_) => Ok(()),
        Ok(_) => Err("the dead instance's console connection still took a line".into()),
    }
}
