//! `orphan-child`: the child C of `tests/launcher-orphan.toml`. It receives the connection its
//! launcher minted for it on its own endpoint (`launcher`), attaches through it and says so to
//! its tester (`tester`); when the tester says to, after its launcher is gone, it tries the
//! connection again and says whether `orphan-server` refused it. Its word is only a trace: the
//! case's verdict is the server's.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_client::Lend;
use redoubt_init_programs::LEND_PAGES;
use redoubt_init_programs::orphan::{AGAIN, ATTACHED, TRIED};
use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Connection;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Exit codes: what it lacked.
const NO_HANDLES: u32 = 2;
const NO_LEND: u32 = 3;
const NO_CONNECTION: u32 = 4;

/// The fids it attaches with: one before its launcher's end, one after.
const BEFORE: u32 = 0;
const AFTER: u32 = 1;

fn run(startup: &Startup) -> u32 {
    let (Some(own), Some(tester)) = (startup.handle("launcher"), startup.handle("tester")) else {
        return NO_HANDLES;
    };
    let (own, tester) = (Endpoint::from_handle(own), Endpoint::from_handle(tester));
    let Ok(mut lend) = Lend::new(LEND_PAGES) else { return NO_LEND };
    let conn = loop {
        if let Ok(Event::Send(delivery)) = own.receive(FOREVER, 0) {
            if let Some(handle) = delivery.handles.as_slice().iter().flatten().next() {
                break Connection::new(Endpoint::from_handle(*handle));
            }
        }
    };
    let attached = conn.version(&mut lend).is_ok() && conn.attach(&mut lend, BEFORE, "").is_ok();
    if tester.send(&[ATTACHED, u64::from(attached), 0, 0], &[], None, FOREVER).is_err() {
        return NO_CONNECTION;
    }
    loop {
        if let Ok(Event::Send(delivery)) = own.receive(FOREVER, 0) {
            if delivery.words[0] == AGAIN {
                break;
            }
        }
    }
    let refused = conn.attach(&mut lend, AFTER, "").is_err();
    let _ = tester.send(&[TRIED, u64::from(refused), 0, 0], &[], None, FOREVER);
    redoubt_init_programs::park()
}
