//! `quitter`: a `servers` entry that exits at its start with code 7, each time `init` starts it,
//! so that it cannot stay up and `init` reboots the machine (servers/init.md, "Restarts and
//! reboots").

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// The code its exit lines carry, so a case can tell its exits from any other.
const CODE: u32 = 7;

fn run(_startup: &Startup) -> u32 { CODE }
