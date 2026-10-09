//! `beamlet-hello`: the program a session's `exec` launches from `/boot` (docs/userland/shell.md,
//! "The shell in a session"): it says its arguments on its standard output, `/dev/stdout`, which the
//! session draws on its console, and exits with [`CODE`], so the case sees both reach the session.
//! It has no console of its own (docs/userland/native.md, "Standard input and output, and pipes").

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use redoubt_client::Lend;
use redoubt_client::ns::Namespace;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Its exit code, which the session's `exec` reports.
const CODE: u32 = 3;
/// No lend, or no standard output to write.
const NO_STDOUT: u32 = 2;

fn run(startup: &Startup) -> u32 {
    let Ok(mut lend) = Lend::new(1) else { return NO_STDOUT };
    let Ok(ns) = Namespace::from_startup(startup, &mut lend) else { return NO_STDOUT };
    let Some((conn, rest)) = ns.lookup("/dev/stdout") else { return NO_STDOUT };
    let Ok(out) = conn.open(&mut lend, rest, mode::OWRITE) else { return NO_STDOUT };
    let args: Vec<&str> = startup.args().collect();
    let line = format!("beamlet-hello: {}\n", args.join(" "));
    let mut rest = line.as_bytes();
    while !rest.is_empty() {
        match out.write_at(&mut lend, 0, rest) {
            Ok(n) if n > 0 => rest = &rest[n..],
            _ => return NO_STDOUT,
        }
    }
    CODE
}
