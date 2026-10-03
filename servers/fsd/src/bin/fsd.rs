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
use redoubt_fsd::{Args, BUDGET, COST, Fsd, limits, mount, parse_args};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::{NineServer, mode};
use redoubt_rt::server::own_args;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// No `buckets=N`, one whose buckets at their caps do not fit the budget, no `endpoint=NAME` or
/// none the startup block holds a handle by, or an argument `fsd` does not understand (`labels=`
/// malformed, or anything else): it does not guess.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, a range `blkd` would not size, or one of fewer than four blocks.
pub const NO_VOLUME: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 6;

/// The line `fsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "fsd: the volume does not mount, and is served as corrupt\n";

/// Says `line` on the console `init` gave this instance, if it has one; a console that fails is
/// not retried, since serving the volume matters more than the line.
fn say(startup: &Startup, line: &str) {
    let Some((_, console)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") else { return };
    let Ok(mut lend) = Lend::new(1) else { return };
    let console = Connection::new(Endpoint::from_handle(console));
    let _ = console
        .attach(&mut lend, 0, "")
        .and_then(|_| console.open(&mut lend, 0, mode::OWRITE))
        .and_then(|_| console.write(&mut lend, 0, 0, line.as_bytes()));
    let _ = console.clunk(&mut lend, 0);
}

/// The endpoint `fsd` receives on: the startup block's handle `endpoint=` names. One place, so
/// where the name comes from can change without touching the rest.
fn receive_endpoint(startup: &Startup, name: &str) -> Option<Endpoint> {
    startup.handle(name).map(Endpoint::from_handle)
}

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let args: Vec<&str> = startup.args().collect();
    let Ok(buckets) = redoubt_rt::server::buckets(&args) else { return BAD_ARGS };
    let Ok(Args { endpoint, labels }) = parse_args(own_args(&args)) else { return BAD_ARGS };
    let Some(endpoint) = receive_endpoint(startup, endpoint) else { return BAD_ARGS };
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
    let fsd = Fsd::new(mounted, labels);
    if fsd.is_corrupt() {
        say(startup, CORRUPT);
    }
    let Ok(mut server) = NineServer::new(fsd, limits, random) else { return BAD_ARGS };
    // 9P and `ninep_common` in the skeleton; the four typed operations are ours.
    redoubt_rt::server::serve(&endpoint, |request| {
        server.serve_with(request, |s, request| serve_call::<Fsds, _>(&mut Typed(s), request))
    })
}
