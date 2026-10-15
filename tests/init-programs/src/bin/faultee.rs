//! `faultee`: a `servers` entry that answers [`redoubt_init_programs::faultee`]'s `PING` and
//! faults while it serves a `FAULT`, so that the kernel prints its kill line then and `init`
//! restarts it (servers/init.md, "Restarts and reboots"). It prints nothing: `half-rest`, its
//! caller, is the reporter.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::faultee::{FAULT, OK, PING};
use redoubt_rt::abi::{FOREVER, Handles};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// Exit code: the startup block named no endpoint to receive on.
const NO_ENDPOINT: u32 = 2;

fn serve(startup: &Startup) -> u32 {
    let Some(endpoint) = startup.handle("faultee") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(endpoint);
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // `receive` made the call this thread's current one: its caller is blamed and
                // gets `Dead` (kernel/ipc.md R4b).
                if request.words[0] == FAULT {
                    panic!("faultee faults while it serves its client");
                }
                let words = if request.words[0] == PING { OK } else { MALFORMED };
                let _ = finish(request, &Outcome { words, send: Handles::new(), close: Handles::new() });
            }
            Ok(_) => {}
            Err(_) => redoubt_init_programs::park(),
        }
    }
}
