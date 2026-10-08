//! `littlefsd`, the program: size and mount its range at `blkd`, then serve the volume over 9P, with
//! its typed operations, until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-littlefsd`'s library, so host tests drive the same code against
//! the runtime's fake kernel (`tests/littlefsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_fileserver::range::Blkd;
use redoubt_littlefsd::typed::{Littlefsds, Typed};
use redoubt_littlefsd::{Args, BUDGET, COST, Littlefsd, limits, mount, parse_args};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::own_args;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::start::say;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// No `buckets=N`, one whose buckets at their caps do not fit the budget, no `endpoint=NAME` or
/// none the startup block holds a handle by, or an argument `littlefsd` does not understand (`labels=`
/// malformed, or anything else): it does not guess.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, a range `blkd` would not size, or one of fewer than four blocks.
pub const NO_VOLUME: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 6;

/// The line `littlefsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "littlefsd: the volume does not mount, and is served as corrupt\n";

/// The endpoint `littlefsd` receives on: the startup block's handle `endpoint=` names. One place, so
/// where the name comes from can change without touching the rest.
fn receive_endpoint(startup: &Startup, name: &str) -> Option<Endpoint> {
    startup.handle(name).map(Endpoint::from_handle)
}

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let args: Vec<&str> = startup.args().collect();
    let Ok(buckets) = redoubt_rt::server::buckets(&args) else { return BAD_ARGS };
    let Ok(Args { endpoint: name, labels }) = parse_args(own_args(&args)) else { return BAD_ARGS };
    let Some(endpoint) = receive_endpoint(startup, name) else { return BAD_ARGS };
    let limits = limits(buckets);
    if !limits.fits(&COST, BUDGET) {
        return BAD_ARGS;
    }
    let Some(volume) = startup.handle("volume") else { return NO_VOLUME };
    // Two pages lent to each call: a block of sectors, and the message around it.
    let Ok(range) = Blkd::new(Endpoint::from_handle(volume), 2) else { return NO_VOLUME };
    // Test-only: R47 tried from inside, before anything is served (libs/fileserver/src/probe.rs).
    #[cfg(feature = "one-volume-probe")]
    let range = {
        let mut range = range;
        say(startup, &redoubt_fileserver::probe::verdict(startup, name, &mut range));
        range
    };
    let Ok(mounted) = mount(range) else { return NO_VOLUME };
    // A range that does not mount is served as corrupt, not exited on: a damaged medium must
    // not become a restart loop.
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let littlefsd = Littlefsd::new(mounted, labels);
    // Test-only: the boot's counts, said on the console (src/stats.rs).
    #[cfg(feature = "boot-stats")]
    let littlefsd = {
        let mut littlefsd = littlefsd;
        if let Some(console) = console_only(startup) {
            littlefsd.say_stats(alloc::boxed::Box::new(move |line| say(&console, line)));
        }
        littlefsd
    };
    if littlefsd.is_corrupt() {
        say(startup, CORRUPT);
    }
    let Ok(mut server) = NineServer::new(littlefsd, limits, random) else { return BAD_ARGS };
    // 9P, multiplexed 9P and `ninep_common` in the skeleton's loop; the four typed operations
    // are ours.
    server.run(&endpoint, |s, request| serve_call::<Littlefsds, _>(&mut Typed(s), request))
}

/// Test-only (`boot-stats`): a startup block naming only this one's console, kept for as long as
/// `littlefsd` runs, so its counts can be said from inside the server.
#[cfg(feature = "boot-stats")]
fn console_only(startup: &Startup) -> Option<Startup<'static>> {
    let (_, console) = startup.namespace().find(|(path, _)| *path == "/dev/cons")?;
    let block =
        redoubt_rt::startup::StartupBuilder::new(console.index()).namespace("/dev/cons", console).finish();
    Startup::parse(alloc::boxed::Box::leak(block.ok()?.into_boxed_slice())).ok()
}
