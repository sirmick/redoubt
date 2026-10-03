//! `passer`: a `servers` entry that passes a copy of the badge `init` handed it at
//! `orphan-server` on to `keeper`, then waits on its own endpoint; told to, it exits, and `init`
//! restarts it, and the new instance passes a copy of its own badge. The copy the dead instance
//! passed on must die with it (servers/init.md, "Restarts and reboots").

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::passer::EXIT;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Exit codes: what it lacked, and the exit it was told to make.
const NO_HANDLES: u32 = 2;
const NOT_PASSED: u32 = 3;
const TOLD: u32 = 9;

fn run(startup: &Startup) -> u32 {
    let (Some(badge), Some(keeper), Some(own)) =
        (startup.handle("orphan-server"), startup.handle("keeper"), startup.handle("passer"))
    else {
        return NO_HANDLES;
    };
    if Endpoint::from_handle(keeper).send(&[0; 4], &[badge], None, FOREVER).is_err() {
        return NOT_PASSED;
    }
    loop {
        if let Ok(Event::Send(delivery)) = Endpoint::from_handle(own).receive(FOREVER, 0) {
            if delivery.words[0] == EXIT {
                return TOLD;
            }
        }
    }
}
