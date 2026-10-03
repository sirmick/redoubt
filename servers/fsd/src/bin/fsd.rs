//! `fsd`, the program: size and mount its range at `blkd`, then serve the volume over 9P, with
//! its typed operations, until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-fsd`'s library, so host tests drive the same code against
//! the runtime's fake kernel (`tests/fsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_fsd::blkd::Blkd;
use redoubt_fsd::typed::{Fsds, Typed};
use redoubt_fsd::{BUDGET, COST, Fsd, limits, mount, parse_labels};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::own_args;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint for `fsd` to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// No `buckets=N`, one whose buckets at their caps do not fit the budget, or an argument `fsd`
/// does not understand (`labels=` malformed, or anything else): it does not guess.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, a range `blkd` would not size, or one of fewer than four blocks.
pub const NO_VOLUME: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 6;

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("fsd") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(handle);
    let args: Vec<&str> = startup.args().collect();
    let Ok(buckets) = redoubt_rt::server::buckets(&args) else { return BAD_ARGS };
    let Ok(labels) = parse_labels(own_args(&args)) else { return BAD_ARGS };
    let limits = limits(buckets);
    if !limits.fits(&COST, BUDGET) {
        return BAD_ARGS;
    }
    let Some(volume) = startup.handle("volume") else { return NO_VOLUME };
    let Ok(range) = Blkd::new(Endpoint::from_handle(volume)) else { return NO_VOLUME };
    let Ok(mounted) = mount(range) else { return NO_VOLUME };
    // A range that does not mount is served as corrupt, not exited on: a damaged medium must
    // not become a restart loop.
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(Fsd::new(mounted, labels), limits, random) else { return BAD_ARGS };
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // 9P and `ninep_common` in the skeleton; the four typed operations are ours. The
                // shared finish path answers a rejected reply, so an error leaves no open call.
                let _ =
                    server.serve_with(request, |s, request| serve_call::<Fsds, _>(&mut Typed(s), request));
            }
            // Nothing here is sent one-way: drop it, and close what it brought.
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            // No call is ever held open here.
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
    }
}
