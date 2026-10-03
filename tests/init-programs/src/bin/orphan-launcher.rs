//! `orphan-launcher`: the launcher L of `tests/launcher-orphan.toml`. Through the connection its
//! tester granted it at `orphan-server` (`server`) it mints a connection for its child, and hands
//! it to the child on the child's endpoint (`child`); the child runs in a budget that is not
//! L's, so L's end does not end it. Then it tells its tester (`tester`), and faults when the
//! tester answers: the tester's `Job::wait` releases L's grants, and the child is the orphan.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_client::Lend;
use redoubt_init_programs::LEND_PAGES;
use redoubt_init_programs::orphan::MINTED;
use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Connection;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Exit codes: each step that failed, which the tester reports from the exit notice.
const NO_HANDLES: u32 = 2;
const NO_LEND: u32 = 3;
const NOT_MINTED: u32 = 4;
const NOT_HANDED: u32 = 5;
const NO_ANSWER: u32 = 6;

fn run(startup: &Startup) -> u32 {
    let (Some(server), Some(child), Some(tester)) =
        (startup.handle("server"), startup.handle("child"), startup.handle("tester"))
    else {
        return NO_HANDLES;
    };
    let Ok(mut lend) = Lend::new(LEND_PAGES) else { return NO_LEND };
    // `new_connection` needs no session: it is `ninep_common`, beside 9P.
    let Ok((minted, _)) = Connection::new(Endpoint::from_handle(server)).new_connection(&mut lend, "", 0)
    else {
        return NOT_MINTED;
    };
    if Endpoint::from_handle(child).send(&[0; 4], &[minted.handle()], None, FOREVER).is_err() {
        return NOT_HANDED;
    }
    let _ = minted.close();
    if Endpoint::from_handle(tester).call(&[MINTED, 0, 0, 0], &[], None, FOREVER).into_result().is_err() {
        return NO_ANSWER;
    }
    // A panic with no call held is an exit; a fault is what the case is about.
    overrun(0) as u32
}

/// Recurses until it runs off the end of its stack, which faults: nothing is mapped below it.
fn overrun(depth: u64) -> u64 {
    let frame = core::hint::black_box([depth; 64]);
    if depth == u64::MAX {
        return frame[0];
    }
    overrun(depth + 1).wrapping_add(frame[63])
}
