//! `walfsd`, the program: size and mount its range at `blkd`, then serve the volume over 9P, with
//! its typed operations, until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-walfsd`'s library, so host tests drive the same code against
//! the runtime's fake kernel (`tests/walfsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_fileserver::program::{Started, start};
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::start::say;
use redoubt_rt::startup::Startup;
use redoubt_walfsd::typed::{Typed, Walfsds};
use redoubt_walfsd::{COST, Walfsd, limits, mount};

redoubt_rt::entry!(serve);

pub use redoubt_fileserver::program::{BAD_ARGS, NO_RANDOM, NO_VOLUME};

/// The line `walfsd` says when it serves its volume as corrupt.
pub const CORRUPT: &str = "walfsd: the volume does not mount, and is served as corrupt\n";
/// How the line starts that says why, after [`CORRUPT`]: the step, walfs's error and the range's
/// reason (`redoubt_walfsd::volume::Unmounted`).
pub const UNMOUNTED: &str = "walfsd: why it does not mount: ";

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    // Two pages lent to each call: a block of sectors, and the message around it.
    let fits = |buckets, budget| limits(buckets).fits(&COST, budget);
    let Started { endpoint, labels, buckets, range } = match start(startup, 2, fits) {
        Ok(started) => started,
        Err(code) => return code,
    };
    let limits = limits(buckets);
    let Ok(mounted) = mount(range) else { return NO_VOLUME };
    // A range that does not mount is served as corrupt, not exited on: a damaged medium must
    // not become a restart loop.
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    // The console, for the one line a poisoned volume says while serving.
    let console = startup.namespace().find(|(path, _)| *path == "/dev/cons").map(|(_, console)| console);
    let walfsd = Walfsd::new(mounted, labels, console);
    if walfsd.is_corrupt() {
        say(startup, CORRUPT);
        if let Some(unmounted) = walfsd.unmounted() {
            say(startup, &alloc::format!("{UNMOUNTED}{unmounted}\n"));
        }
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
    #[cfg(feature = "session-count")]
    return server.run_around(&endpoint, count::Sessions { startup, open: 0 });
    #[cfg(not(feature = "session-count"))]
    server.run(&endpoint, |s, request| serve_call::<Walfsds, _>(&mut Typed(s), request))
}

/// Test-only (feature `session-count`): the skeleton's loop as `run` serves it, saying on the
/// console how many multiplexed sessions are open whenever the number changes.
#[cfg(feature = "session-count")]
mod count {
    use redoubt_rt::ipc::Request;
    use redoubt_rt::server::ninep::{Around, NineServer};
    use redoubt_walfsd::{Range, Walfsd};

    use super::{Startup, Typed, Walfsds, say, serve_call};

    pub struct Sessions<'a> {
        pub startup: &'a Startup<'a>,
        /// The number last said.
        pub open: usize,
    }

    impl<R: Range> Around<Walfsd<R>> for Sessions<'_> {
        fn call(&mut self, server: &mut NineServer<Walfsd<R>>, request: Request, _: u64) {
            let _ = server.serve_with(request, |s, request| {
                serve_call::<Walfsds, _>(&mut Typed(s), request).map(|()| None)
            });
        }

        fn turn(&mut self, server: &mut NineServer<Walfsd<R>>, _: u64) {
            let open = server.sessions();
            if open != self.open {
                self.open = open;
                let noun = if open == 1 { "session" } else { "sessions" };
                say(self.startup, &alloc::format!("walfsd: {open} {noun} open\n"));
            }
        }
    }
}
