//! `beamlet-caller`: the second program of the serving case (docs/userland/beamlet.md,
//! "Natives": `serve/1`): it calls the endpoint a session's VM serves, the named handle `service`,
//! twice, and says each reply's first two words. The VM answers the first; the second it never
//! answers, and the serve thread ends it at its deadline with status 1 (`malformed`). Then a third
//! call carries both answers to the VM, which says them: the case's verdict is read from the VM's
//! console alone, since two programs' consoles have no order between them, and the third call
//! itself shows the VM still serves past the thread's deadline.

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

/// The opcodes of its calls: the VM answers the first and holds the second; the third reports.
const ANSWERED: u64 = 41;
const HELD: u64 = 42;
const REPORT: u64 = 43;
/// A call the kernel or the library refused, in the report, in place of a reply's words.
const REFUSED: u64 = u64::MAX;

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
    // Each answer's status and second word, for the report.
    let mut seen = [REFUSED; 4];
    for (i, opcode) in [ANSWERED, HELD].into_iter().enumerate() {
        let line = match service.call(&[opcode, 0, 0, 0], &[], None, FOREVER).into_result() {
            Ok((reply, _)) => {
                seen[2 * i] = reply.words[0];
                seen[2 * i + 1] = reply.words[1];
                format!("beamlet-caller: {opcode} answered {} {}\n", reply.words[0], reply.words[1])
            }
            Err(e) => format!("beamlet-caller: {opcode} refused {e:?}\n"),
        };
        let _ = out.say(&line);
    }
    let report = [REPORT, seen[0], seen[1], seen[2]];
    match service.call(&report, &[], None, FOREVER).into_result() {
        Ok(_) => 0,
        Err(e) => {
            let _ = out.say(&format!("beamlet-caller: {REPORT} refused {e:?}\n"));
            1
        }
    }
}
