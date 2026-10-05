//! `verityd`, the program: check its range at `blkd` against the root `init` gave it, then serve
//! the volume on `blkd`'s protocol until its endpoint is destroyed (docs/servers/verityd.md).
//!
//! Everything it can do is in `redoubt-verityd`'s library, so host tests drive the same code
//! against a fake range.
//!
//! A volume that fails the start check is not exited on: `verityd` says why on its console,
//! answers `info`, and fails every read, so a bad medium never becomes a restart loop.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;
use redoubt_verityd::blkd::Blkd;
use redoubt_verityd::server::Said;
use redoubt_verityd::{Args, Verityd, parse_args};

redoubt_rt::entry!(serve);

/// An argument `verityd` does not take, one missing, or no handle by the name `endpoint=` gives.
pub const BAD_ARGS: u32 = 4;
/// No `volume` handle, or no memory to call it with.
pub const NO_VOLUME: u32 = 5;

/// The startup-block name of the range at `blkd`.
const VOLUME: &str = "volume";

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

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Ok(Args { endpoint, labels, root, geometry }) = parse_args(startup.args()) else { return BAD_ARGS };
    let Some(endpoint) = startup.handle(endpoint).map(Endpoint::from_handle) else { return BAD_ARGS };
    let Some(volume) = startup.handle(VOLUME) else { return NO_VOLUME };
    let Ok(range) = Blkd::new(Endpoint::from_handle(volume)) else { return NO_VOLUME };
    let mut server = Verityd::new(range, geometry, &root, labels);
    let tell = |line: Said| say(startup, &alloc::format!("{line}\n"));
    if let Some(line) = server.take_line() {
        tell(line);
    }
    // The handler answers every call; what it returns is dropped.
    redoubt_rt::server::serve(&endpoint, |request| server.serve(request, tell))
}
