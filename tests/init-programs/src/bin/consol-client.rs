//! `consol-client`: a `servers` entry that asks its console, `consoled`, for `consol`'s `size`,
//! then makes `resize` calls that each give up after a while, more of them than its share of
//! `consoled`'s parked calls holds, and asks `size` again (servers/consoled.md, "The `consol`
//! protocol"). Each `resize` is held, since a UART never changes size, and is freed when it is
//! abandoned: were one left behind, the next would be refused at once rather than held. It says
//! what the console answered, and its verdict last.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;

use alloc::format;

use redoubt_client::{Error, typed};
use redoubt_init_programs::Out;
use redoubt_rt::abi::Error as Sys;
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::consol::{self, Message, Reply, Resize};

redoubt_rt::entry!(run);

/// How many `resize` calls it makes: more than the one parked call a lone connection's share of
/// `consoled`'s 2 a bucket holds.
const RESIZES: u32 = 3;
/// How long each waits before it gives up (µs).
const GIVE_UP_US: u64 = 200_000;

fn run(startup: &Startup) -> u32 {
    let mut out = match Out::open(startup) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let verdict = check(&mut out);
    let line = match &verdict {
        Ok(()) => format!("consol-client TEST PASSED\n"),
        Err(why) => format!("consol-client TEST FAILED: {why}\n"),
    };
    if out.say(&line).is_err() {
        return redoubt_init_programs::code::NO_CONSOLE;
    }
    redoubt_init_programs::park()
}

fn check(out: &mut Out) -> Result<(), alloc::string::String> {
    let size = |out: &mut Out| match out.console.size(&mut out.lend) {
        Ok(Some(size)) => Ok(size),
        other => Err(format!("size answered {other:?}")),
    };
    let (cols, rows) = size(out)?;
    out.say(&format!("consol-client: size {cols} by {rows}\n")).map_err(|e| format!("{e:?}"))?;
    for i in 0..RESIZES {
        let endpoint = out.console.file().connection().endpoint();
        let waited = typed::call_within::<consol::Protocol, _>(
            endpoint,
            &mut out.lend,
            &Message::Resize(Resize {}),
            &[],
            GIVE_UP_US,
            |reply, _| matches!(reply, Reply::Resize(_)),
        );
        match waited {
            Err(Error::Sys(Sys::Timeout)) => {}
            other => return Err(format!("resize {i} answered {other:?}, not held")),
        }
    }
    out.say(&format!("consol-client: {RESIZES} resize calls waited and were freed\n"))
        .map_err(|e| format!("{e:?}"))?;
    if size(out)? != (cols, rows) {
        return Err(format!("the size changed"));
    }
    Ok(())
}
