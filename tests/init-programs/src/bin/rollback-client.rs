//! `rollback-client`: a `servers` entry that fills its own handle table, so the kernel drops the
//! capability in every `new_connection` reply `consoled` sends it, asks for more connections that
//! way than a bucket of `consoled`'s holds, then frees one slot and asks once more
//! (servers/serving.md, "Replies and rollback"). Had `consoled` kept any of the connections
//! whose replies were discarded, the bucket would be full and the last request refused: that it
//! stands is `consoled`'s answer, and the verdict this program prints.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_init_programs::Out;
use redoubt_rt::abi::{Error, MAX_THREADS};
use redoubt_rt::client::{ClientError, Connection};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// More than the connections one of `consoled`'s buckets holds (`MAX_THREADS`).
const DISCARDED: usize = MAX_THREADS + 9;
/// How long a release may take: a live server answers at once.
const TIMEOUT: u64 = 1_000_000;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let line = match check(startup, &mut out) {
        Ok(()) => format!("rollback-client TEST PASSED\n"),
        Err(why) => format!("rollback-client TEST FAILED: {why}\n"),
    };
    match out.say(&line) {
        Ok(()) => redoubt_init_programs::park(),
        Err(_) => redoubt_init_programs::code::NO_CONSOLE,
    }
}

fn check(startup: &Startup, out: &mut Out) -> Result<(), String> {
    let (_, cons) = startup.namespace().find(|(path, _)| *path == "/dev/cons").ok_or("no console")?;
    // `new_connection` needs no session: it goes beside the console's own.
    let console = Connection::new(Endpoint::from_handle(cons));
    // Badges minted from one endpoint until the table is full: a reply's capability has no slot.
    let filler = Endpoint::create().map_err(|e| format!("an endpoint to fill the table with: {e:?}"))?;
    let badge = NonZeroU64::new(1).expect("1 is not 0");
    let mut held = Vec::new();
    while let Ok(copy) = filler.mint(badge, None) {
        held.push(copy);
    }
    // Granted, its capability lost on the way: the reply came, and the kernel had no slot for
    // the capability (kernel/ipc.md, "How a call completes").
    let lost = |r| matches!(r, Err(ClientError::Sys(Error::OutOfMemory)));
    let discarded = (0..DISCARDED).filter(|_| lost(console.new_connection(&mut out.lend, "", 0))).count();
    let free = held.pop().ok_or("no handle minted")?;
    free.close().map_err(|e| format!("free one slot: {e:?}"))?;
    let stood = console.new_connection(&mut out.lend, "", 0);
    for copy in held {
        let _ = copy.close();
    }
    if discarded != DISCARDED {
        return Err(format!(
            "{} of {DISCARDED} requests with the table full were not granted and lost",
            DISCARDED - discarded
        ));
    }
    let (_, id) =
        stood.map_err(|e| format!("refused with one slot free after {DISCARDED} discarded: {e:?}"))?;
    console.disconnect(id, TIMEOUT).map_err(|e| format!("give it back: {e:?}"))
}
