//! `walfsd`, the program: size and mount its range at `blkd`, then serve the volume over 9P, with
//! its typed operations, until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-walfsd`'s library, so host tests drive the same code against
//! the runtime's fake kernel (`tests/walfsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::own_args;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::start::say;
use redoubt_rt::startup::Startup;
use redoubt_walfsd::blkd::Blkd;
use redoubt_walfsd::typed::{Typed, Walfsds};
use redoubt_walfsd::{Args, BUDGET, COST, Walfsd, limits, mount, parse_args};

redoubt_rt::entry!(serve);

/// No `buckets=N`, one whose buckets at their caps do not fit the budget, no `endpoint=NAME` or
/// none the startup block holds a handle by, or an argument `walfsd` does not understand
/// (`labels=` malformed, or anything else): it does not guess.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, a range `blkd` would not size, or one too small for a walfs volume.
pub const NO_VOLUME: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 6;

/// The line `walfsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "walfsd: the volume does not mount, and is served as corrupt\n";

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let args: Vec<&str> = startup.args().collect();
    let Ok(buckets) = redoubt_rt::server::buckets(&args) else { return BAD_ARGS };
    let Ok(Args { endpoint: name, labels }) = parse_args(own_args(&args)) else { return BAD_ARGS };
    let Some(endpoint) = startup.handle(name).map(Endpoint::from_handle) else { return BAD_ARGS };
    let limits = limits(buckets);
    if !limits.fits(&COST, BUDGET) {
        return BAD_ARGS;
    }
    let Some(volume) = startup.handle("volume") else { return NO_VOLUME };
    let Ok(range) = Blkd::new(Endpoint::from_handle(volume)) else { return NO_VOLUME };
    // Test-only: R47 tried from inside, before anything is served (src/one_volume.rs).
    #[cfg(feature = "one-volume-probe")]
    let range = {
        let mut range = range;
        say(startup, &redoubt_walfsd::one_volume::verdict(startup, name, &mut range));
        range
    };
    let Ok(mounted) = mount(range) else { return NO_VOLUME };
    // A range that does not mount is served as corrupt, not exited on: a damaged medium must
    // not become a restart loop.
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let walfsd = Walfsd::new(mounted, labels);
    if walfsd.is_corrupt() {
        say(startup, CORRUPT);
    }
    // Test-only: the volume checked whole at every start (src/cut.rs).
    #[cfg(feature = "cut-after-write")]
    let walfsd = {
        let mut walfsd = walfsd;
        if let Some(line) = walfsd.checked() {
            say(startup, &line);
        }
        walfsd
    };
    let Ok(mut server) = NineServer::new(walfsd, limits, random) else { return BAD_ARGS };
    // 9P, multiplexed 9P and `ninep_common` in the skeleton's loop; the four typed operations
    // are ours.
    server.run(&endpoint, |s, request| serve_call::<Walfsds, _>(&mut Typed(s), request))
}
