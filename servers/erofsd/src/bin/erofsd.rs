//! `erofsd`, the program: size its range, read the volume's superblock and root, then serve the
//! volume over 9P until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-erofsd`'s library, so host tests drive the same code
//! against the runtime's fake kernel (`tests/erofsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_erofsd::{BUDGET, COST, Erofsd, limits};
use redoubt_fileserver::program::{Started, start};
use redoubt_rt::server::ninep::{NineServer, refuse_malformed};
use redoubt_rt::start::say;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

pub use redoubt_fileserver::program::{BAD_ARGS, NO_RANDOM, NO_VOLUME};

/// The line `erofsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "erofsd: the volume does not read as EROFS, and is served as corrupt\n";

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    // Nine pages lent to each call: the most sectors one read carries, and the message around them.
    let fits = |buckets| limits(buckets).fits(&COST, BUDGET);
    let Started { endpoint, labels, buckets, range } = match start(startup, 9, fits) {
        Ok(started) => started,
        Err(code) => return code,
    };
    let limits = limits(buckets);
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
