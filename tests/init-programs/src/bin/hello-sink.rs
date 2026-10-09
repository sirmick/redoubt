//! `hello-sink`: the steward's stand-in in `tests/consrelay-footprint.toml`. It receives on its
//! endpoint (`hello`) and lets every send go, handles and all, so the relay `init` starts beside
//! it takes its hello as sent and serves on. Its word is nothing: the case's verdict is the
//! bench's memory scan of the relay.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::close_delivery;
use redoubt_rt::server::ninep::refuse_malformed;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// Exit code: no `hello` endpoint in its startup block.
const NO_HANDLES: u32 = 2;

fn run(startup: &Startup) -> u32 {
    let Some(own) = startup.handle("hello") else { return NO_HANDLES };
    let own = Endpoint::from_handle(own);
    loop {
        match own.receive(FOREVER, 0) {
            Ok(Event::Send(delivery)) => close_delivery(&delivery),
            Ok(Event::Call(request)) => {
                let _ = refuse_malformed(request);
            }
            _ => {}
        }
    }
}
