//! `restart-faulter`: a labelled `servers` entry whose one call `restartee` faults while it
//! serves, so that `init`'s fault line names its labels. It cannot write the console, being
//! labelled (servers/consoled.md R69), so it says nothing: its call ends `Dead`, and it parks.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::restartee::FAULT;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// It was handed no badge at `restartee`.
const NO_HANDLE: u32 = 2;

fn run(startup: &Startup) -> u32 {
    let Some(at) = startup.handle("restartee") else { return NO_HANDLE };
    let _ = Endpoint::from_handle(at).call(&[FAULT, 0, 0, 0], &[], None, FOREVER);
    redoubt_init_programs::park()
}
