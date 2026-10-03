//! `con-forger`: a `servers` entry that prints each of its arguments on its console as a line of
//! its own: `init`'s lines, a verdict, a `[con N]` prefix of another connection, whatever its
//! case's manifest gives it. Each must come out starting with its own connection's id, never bare
//! and never as another's (servers/consoled.md, "Started by `init`").

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;

use redoubt_init_programs::Out;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    for line in startup.args() {
        if out.say(&format!("{line}\n")).is_err() {
            return redoubt_init_programs::code::NO_CONSOLE;
        }
    }
    redoubt_init_programs::park()
}
