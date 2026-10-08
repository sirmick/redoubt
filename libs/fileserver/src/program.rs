//! What the volume servers' programs share before they serve: their exit codes, and [`start`],
//! which reads the startup block `init` wrote (servers/init.md, "The boot manifest") and opens
//! the range.

use alloc::vec::Vec;

use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::{buckets, own_args};
use redoubt_rt::startup::Startup;

use crate::args::{Args, parse_args};
use crate::range::Blkd;

/// No `buckets=N`, one whose buckets at their caps do not fit the budget, no `endpoint=NAME` or
/// none the startup block holds a handle by, or an argument the server does not understand
/// (`labels=` malformed, or anything else): it does not guess.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, or a range the server cannot serve: one it cannot size, or too small for
/// its format.
pub const NO_VOLUME: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 6;

/// What a volume server starts with.
pub struct Started {
    /// The endpoint it receives on.
    pub endpoint: Endpoint,
    /// The volume's label set.
    pub labels: Vec<u64>,
    /// The manifest's `buckets=N`.
    pub buckets: u32,
    /// Its range, lending `pages` to each call.
    pub range: Blkd,
}

/// Reads the arguments and handles in `startup`, refusing with [`BAD_ARGS`] what [`parse_args`]
/// or `buckets=` refuse, an endpoint the block does not hold, and a bucket count `fits` says the
/// budget cannot hold; and with [`NO_VOLUME`] a block without its `volume`. Built with
/// `one-volume-probe`, it tries R47 from inside before anything is served, and says what it
/// found (src/probe.rs).
pub fn start(startup: &Startup, pages: usize, fits: impl FnOnce(u32) -> bool) -> Result<Started, u32> {
    let args: Vec<&str> = startup.args().collect();
    let buckets = buckets(&args).map_err(|_| BAD_ARGS)?;
    let Args { endpoint: name, labels } = parse_args(own_args(&args)).map_err(|_| BAD_ARGS)?;
    let endpoint = startup.handle(name).map(Endpoint::from_handle).ok_or(BAD_ARGS)?;
    if !fits(buckets) {
        return Err(BAD_ARGS);
    }
    let volume = startup.handle("volume").ok_or(NO_VOLUME)?;
    let range = Blkd::new(Endpoint::from_handle(volume), pages).map_err(|_| NO_VOLUME)?;
    #[cfg(feature = "one-volume-probe")]
    let range = {
        let mut range = range;
        redoubt_rt::start::say(startup, &crate::probe::verdict(startup, name, &mut range));
        range
    };
    Ok(Started { endpoint, labels, buckets, range })
}
