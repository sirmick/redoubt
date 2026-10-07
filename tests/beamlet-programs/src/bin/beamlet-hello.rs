//! `beamlet-hello`: the program a session's `exec` launches from `/boot` (docs/userland/shell.md,
//! "The shell in a session"): it says its arguments on the console its launcher gave it as
//! `/dev/cons`, and exits with [`CODE`], so the case sees both reach the session.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use redoubt_init_programs::Out;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Its exit code, which the session's `exec` reports.
const CODE: u32 = 3;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let args: Vec<&str> = startup.args().collect();
    let _ = out.say(&format!("beamlet-hello: {}\n", args.join(" ")));
    CODE
}
