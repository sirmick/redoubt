//! `littlefsd`, the program: size and mount its range at `blkd`, then serve the volume over 9P, with
//! its typed operations, until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-littlefsd`'s library, so host tests drive the same code against
//! the runtime's fake kernel (`tests/littlefsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_fileserver::program::{Started, start};
use redoubt_littlefsd::typed::{Littlefsds, Typed};
use redoubt_littlefsd::{BUDGET, COST, Littlefsd, limits, mount};
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::start::say;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

pub use redoubt_fileserver::program::{BAD_ARGS, NO_RANDOM, NO_VOLUME};

/// The line `littlefsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "littlefsd: the volume does not mount, and is served as corrupt\n";

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    // Two pages lent to each call: a block of sectors, and the message around it.
    let fits = |buckets| limits(buckets).fits(&COST, BUDGET);
    let Started { endpoint, labels, buckets, range } = match start(startup, 2, fits) {
        Ok(started) => started,
        Err(code) => return code,
    };
    let limits = limits(buckets);
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
