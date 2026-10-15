//! `half-rest`: a `servers` entry that writes half a line on its console, has `faultee` die
//! serving its call, so that the kernel's kill line comes while the line is unfinished, and
//! writes the rest once the instance `init` restarts answers, which is after the kill line
//! (servers/consoled.md, "Started by `init`"; kernel/devices.md, "The console's one writer").
//! The console's bytes are the verdict: `[con N] half` on a line of its own, the kill line, then
//! `[con N] rest` with its prefix again.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::faultee::{FAULT, OK, PING};
use redoubt_init_programs::{Out, code};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Exit codes past the library's: the startup block named no `faultee`; the call it faults on
/// ended otherwise than `Dead`; the restarted instance did not answer.
const NO_FAULTEE: u32 = 4;
const NOT_DEAD: u32 = 5;
const NOT_BACK: u32 = 6;

fn call(at: &Endpoint, opcode: u64) -> Result<[u64; 4], Error> {
    at.call(&[opcode, 0, 0, 0], &[], None, FOREVER).into_result().map(|(reply, _)| reply.words)
}

fn run(startup: &Startup) -> u32 {
    let Some(at) = startup.handle("faultee") else { return NO_FAULTEE };
    let at = Endpoint::from_handle(at);
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    if out.say("half").is_err() {
        return code::NO_CONSOLE;
    }
    // `faultee` faults serving this call: the kernel ends it, prints its kill line, and the
    // call gets `Dead` (kernel/ipc.md R4b).
    if call(&at, FAULT) != Err(Error::Dead) {
        return NOT_DEAD;
    }
    // This call waits for the instance `init` restarts once it has the exit notice, which the
    // kernel produces after the kill line (servers/init.md, "Restarts and reboots").
    if call(&at, PING) != Ok(OK) {
        return NOT_BACK;
    }
    if out.say("rest\n").is_err() || out.say("half-rest TEST PASSED\n").is_err() {
        return code::NO_CONSOLE;
    }
    redoubt_init_programs::park()
}
