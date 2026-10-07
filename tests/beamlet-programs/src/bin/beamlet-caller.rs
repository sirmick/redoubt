//! `beamlet-caller`: the second program of the serving case (docs/userland/beamlet.md,
//! "Natives": `serve/1`): it calls the endpoint a session's VM serves, the named handle `service`,
//! twice, and says each reply's first two words. The VM answers the first; the second it never
//! answers, and the serve thread ends it at its deadline with status 1 (`malformed`).

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;

use redoubt_init_programs::Out;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// The opcodes of its two calls: the VM answers the first and holds the second.
const ANSWERED: u64 = 41;
const HELD: u64 = 42;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let Some(service) = startup.handle("service") else {
        let _ = out.say("beamlet-caller: no service\n");
        return 1;
    };
    let service = Endpoint::from_handle(service);
    for opcode in [ANSWERED, HELD] {
        let line = match service.call(&[opcode, 0, 0, 0], &[], None, FOREVER).into_result() {
            Ok((reply, _)) => {
                format!("beamlet-caller: {opcode} answered {} {}\n", reply.words[0], reply.words[1])
            }
            Err(e) => format!("beamlet-caller: {opcode} refused {e:?}\n"),
        };
        let _ = out.say(&line);
    }
    0
}
