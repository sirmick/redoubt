//! `boot-reader`: a `servers` entry that reads one public entry through `bootfsd`, as a program
//! started by `init` does, and prints its verdict on its console. Its arguments are the entry's
//! name and the text it must hold; its handle `bootfsd` is the root badge the manifest hands it.
//!
//! `init` starts it before it pushes the public entries and seals `bootfsd`, and `/boot` shows
//! nothing until then, so it asks again, for a bounded time, while the entry is not there yet.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use redoubt_client::file::Connection;
use redoubt_init_programs::Out;
use redoubt_rt::handle::{Endpoint, sleep};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// How often, and how far apart in microseconds, it asks for the entry before giving up.
const TRIES: u32 = 200;
const APART: u64 = 10_000;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let line = match read(startup, &mut out) {
        Ok(()) => format!("boot-reader TEST PASSED\n"),
        Err(why) => format!("boot-reader TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_rt::exit::OK,
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

fn read(startup: &Startup, out: &mut Out) -> Result<(), alloc::string::String> {
    let mut args = startup.args();
    let (Some(name), Some(want)) = (args.next(), args.next()) else {
        return Err("no entry name and text in its arguments".into());
    };
    let handle = startup.handle("bootfsd").ok_or("no bootfsd handle")?;
    let boot = Connection::attach(Endpoint::from_handle(handle), &mut out.lend)
        .map_err(|e| format!("attach to bootfsd: {e:?}"))?;
    let mut tries = 0;
    let file = loop {
        match boot.open(&mut out.lend, name, mode::OREAD) {
            Ok(file) => break file,
            Err(e) if tries == TRIES => return Err(format!("open {name}: {e:?}")),
            Err(_) => {
                tries += 1;
                sleep(APART).map_err(|e| format!("sleep: {e:?}"))?;
            }
        }
    };
    let mut got = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let n =
            file.read_at(&mut out.lend, got.len() as u64, &mut chunk).map_err(|e| format!("read: {e:?}"))?;
        if n == 0 {
            break;
        }
        got.extend_from_slice(&chunk[..n]);
        if got.len() > want.len() {
            break;
        }
    }
    if got != want.as_bytes() {
        return Err(format!("{name} holds {:?}, not {want:?}", alloc::string::String::from_utf8_lossy(&got)));
    }
    Ok(())
}
