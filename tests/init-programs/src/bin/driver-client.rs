//! `driver-client`: a `servers` entry that has `dma-driver` fault while it serves it, then calls
//! it again and prints its verdict: the call held at the fault ends `Dead`, and the next is
//! answered, which only an instance that took its DMA page again does.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use redoubt_init_programs::Out;
use redoubt_init_programs::dma_driver::{FAULT, OK, PING};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let line = match check(startup) {
        Ok(()) => format!("driver-client TEST PASSED\n"),
        Err(why) => format!("driver-client TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

fn call(at: &Endpoint, opcode: u64) -> Result<[u64; 4], Error> {
    Ok(at.call(&[opcode, 0, 0, 0], &[], None, FOREVER).into_result()?.0.words)
}

fn check(startup: &Startup) -> Result<(), String> {
    let at = Endpoint::from_handle(startup.handle("dma-driver").ok_or("no dma-driver handle")?);
    match call(&at, FAULT) {
        Err(Error::Dead) => {}
        other => return Err(format!("the call it faulted on ended {other:?}, not Dead")),
    }
    // Queued on the endpoint until the new instance, its page taken, receives it.
    match call(&at, PING) {
        Ok(OK) => Ok(()),
        other => Err(format!("the new instance answered {other:?}")),
    }
}
