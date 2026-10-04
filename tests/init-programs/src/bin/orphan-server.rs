//! `orphan-server`: the server of `tests/launcher-orphan.toml`, a 9P server with an empty root on
//! the runtime's serving library, started by the tester in `init`'s place. Beside 9P and
//! `ninep_common` it answers [`redoubt_init_programs::orphan::COUNT`] with the minted connections
//! its own tables hold, which is the case's verdict: a server's word on what it still serves, never
//! a client's.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use redoubt_init_programs::Empty;
use redoubt_init_programs::orphan::{COUNT, OK};
use redoubt_rt::abi::{FOREVER, Handles};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::server::{Limits, MALFORMED};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// Exit codes: what it lacked to start.
const NO_ENDPOINT: u32 = 2;
const NO_RANDOM: u32 = 3;
const BAD_LIMITS: u32 = 4;

/// A few connections per bucket: the tester's, the launcher's and the child's.
const LIMITS: Limits = Limits { buckets: 2, in_flight: 0, files: 4, state: 4, requests: 0, pages: 0 };

fn serve(startup: &Startup) -> u32 {
    let Some(endpoint) = startup.handle("orphan-server") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(endpoint);
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(Empty, LIMITS, random) else { return BAD_LIMITS };
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                let _ = server.serve_with(request, |s, request| {
                    let words = match request.words[0] {
                        COUNT => [OK[0], s.connections() as u64, 0, 0],
                        _ => MALFORMED,
                    };
                    let mut close = Handles::new();
                    for handle in request.handles.as_slice().iter().flatten() {
                        let _ = close.push(*handle);
                    }
                    finish(request, &Outcome { words, send: Handles::new(), close }).map(|_| ())
                });
            }
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            Ok(_) => {}
            Err(_) => redoubt_init_programs::park(),
        }
    }
}
