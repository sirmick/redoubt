//! `bootfsd`, the program: build the table from the `public` list in its arguments, let its
//! launcher fill it with `add` and end setup with `seal`, then serve `/boot` read-only over 9P
//! until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-bootfsd`'s library, so host tests drive the same code
//! against the runtime's fake kernel (`tests/bootfsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_bootfsd::server::{BUDGET, BootFs, Bootfs, COST, limits};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::own_args;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint for `bootfsd` to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// No `buckets=N` in the arguments, or one whose buckets at their caps do not fit the budget or
/// the open-call headroom: the manifest sized this server wrongly, and it does not guess.
pub const BAD_LIMITS: u32 = 4;
/// The `public` list in the arguments was refused
/// ([`redoubt_bootfsd::SetupError`]): too many entries, one without a canonical `LENGTH:` before
/// its name, a name that is not one path component, the same name twice, lengths past
/// `MAX_BYTES` together, or no memory for them. `bootfsd` does not start on any of them, because a `/boot`
/// that is not what the manifest named is worse than none (TENETS.md 2, fail closed and loudly).
pub const BAD_PUBLIC_LIST: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27). A server that cannot get one does not start.
pub const NO_RANDOM: u32 = 6;

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("bootfsd") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(handle);
    let args: Vec<&str> = startup.args().collect();
    let Ok(buckets) = redoubt_rt::server::buckets(&args) else { return BAD_LIMITS };
    let limits = limits(buckets);
    let Ok(fs) = BootFs::new(own_args(&args)) else { return BAD_PUBLIC_LIST };
    if !limits.fits(&COST, BUDGET) {
        return BAD_LIMITS;
    }
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(fs, limits, random) else { return BAD_LIMITS };
    // 9P, multiplexed 9P and `ninep_common` in the skeleton's loop; `add` and `seal` are ours.
    server.run(&endpoint, |s, request| serve_call::<Bootfs, _>(&mut s.fs, request))
}
