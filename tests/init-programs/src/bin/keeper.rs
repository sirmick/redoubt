//! `keeper`: a `servers` entry that keeps the copy `passer` passes it of `passer`'s badge at
//! `orphan-server`, and tells `passer` to exit. It asks the server through that copy until the
//! kernel refuses it as a bad handle, gone with the instance that passed it on since `init`
//! stamped the badge with that instance's budget; then it takes the copy the restarted `passer`
//! passes, which is answered.
//! The kernel's and the server's answers are its verdict.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use redoubt_init_programs::Out;
use redoubt_init_programs::orphan::{COUNT, OK};
use redoubt_init_programs::passer::EXIT;
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::{Endpoint, sleep};
use redoubt_rt::ipc::Event;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// How far apart, in microseconds, it asks through the old copy while `init` restarts `passer`.
const APART: u64 = 10_000;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let line = match check(startup) {
        Ok(()) => format!("keeper TEST PASSED\n"),
        Err(why) => format!("keeper TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

/// The next badge `passer` passes on.
fn passed(own: &Endpoint) -> Result<Handle, String> {
    loop {
        match own.receive(FOREVER, 0) {
            Ok(Event::Send(delivery)) => {
                if let Some(handle) = delivery.handles.as_slice().iter().flatten().next() {
                    return Ok(*handle);
                }
            }
            Ok(_) => {}
            Err(e) => return Err(format!("receive: {e:?}")),
        }
    }
}

/// `orphan-server` asked through `badge`.
fn count(badge: Handle) -> Result<[u64; 4], Error> {
    Ok(Endpoint::from_handle(badge).call(&[COUNT, 0, 0, 0], &[], None, FOREVER).into_result()?.0.words)
}

fn check(startup: &Startup) -> Result<(), String> {
    let own = Endpoint::from_handle(startup.handle("keeper").ok_or("no keeper endpoint")?);
    let passer = Endpoint::from_handle(startup.handle("passer").ok_or("no passer handle")?);
    let old = passed(&own)?;
    match count(old) {
        Ok(words) if words[0] == OK[0] => {}
        other => return Err(format!("the first copy was not answered: {other:?}")),
    }
    passer.send(&[EXIT, 0, 0, 0], &[], None, FOREVER).map_err(|e| format!("tell passer: {e:?}"))?;
    // Asked before the new copy is taken, which could land in the old one's slot: a copy arrives
    // only when this program receives it.
    // Answered until `init` destroys the dead instance's budget; then the sweep has closed the
    // copy here, and the call is refused as a bad handle. A call the sweep caught at the server
    // ends `Dead`, and the next one sees the handle gone. The case's timeout bounds the wait.
    loop {
        match count(old) {
            Err(Error::BadHandle) => break,
            Ok(_) | Err(Error::Dead) => sleep(APART).map_err(|e| format!("sleep: {e:?}"))?,
            Err(e) => return Err(format!("the old copy failed otherwise: {e:?}")),
        }
    }
    let new = passed(&own)?;
    match count(new) {
        Ok(words) if words[0] == OK[0] => Ok(()),
        other => Err(format!("the new instance's copy was not answered: {other:?}")),
    }
}
