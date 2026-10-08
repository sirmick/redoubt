//! `erofsd`, the program: size its range, read the volume's superblock and root, then serve the
//! volume over 9P until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-erofsd`'s library, so host tests drive the same code
//! against the runtime's fake kernel (`tests/erofsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_erofsd::{Args, BUDGET, COST, Erofsd, limits, parse_args};
use redoubt_fileserver::range::Blkd;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::{NineServer, refuse_malformed};
use redoubt_rt::server::own_args;
use redoubt_rt::start::say;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// No `buckets=N`, one whose buckets at their caps do not fit the budget, no `endpoint=NAME` or
/// none the startup block holds a handle by, or an argument `erofsd` does not understand
/// (`labels=` malformed, or anything else): it does not guess.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, or a range that would not say its size.
pub const NO_VOLUME: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 6;

/// The line `erofsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "erofsd: the volume does not read as EROFS, and is served as corrupt\n";

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
    // Nine pages lent to each call: the most sectors one read carries, and the message around them.
    let Ok(range) = Blkd::new(Endpoint::from_handle(volume), 9) else { return NO_VOLUME };
    // A volume that does not parse is served as corrupt, not exited on: a damaged medium must
    // not become a restart loop.
    let Ok(erofsd) = Erofsd::new(range, labels) else { return NO_VOLUME };
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    // Test-only: the boot's counts, said on the console (src/stats.rs).
    #[cfg(feature = "boot-stats")]
    let erofsd = {
        let mut erofsd = erofsd;
        if let Some(console) = console_only(startup) {
            erofsd.say_stats(alloc::boxed::Box::new(move |line| say(&console, line)));
        }
        erofsd
    };
    if erofsd.is_corrupt() {
        say(startup, CORRUPT);
    }
    let Ok(mut server) = NineServer::new(erofsd, limits, random) else { return BAD_ARGS };
    // 9P, multiplexed 9P and `ninep_common` in the skeleton's loop; there are no typed
    // operations of `erofsd`'s own.
    server.run(&endpoint, |_, request| refuse_malformed(request))
}

/// Test-only (`boot-stats`): a startup block naming only this one's console, kept for as long as
/// `erofsd` runs, so its counts can be said from inside the server.
#[cfg(feature = "boot-stats")]
fn console_only(startup: &Startup) -> Option<Startup<'static>> {
    let (_, console) = startup.namespace().find(|(path, _)| *path == "/dev/cons")?;
    let block =
        redoubt_rt::startup::StartupBuilder::new(console.index()).namespace("/dev/cons", console).finish();
    Startup::parse(alloc::boxed::Box::leak(block.ok()?.into_boxed_slice())).ok()
}
