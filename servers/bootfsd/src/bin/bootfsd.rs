//! `bootfsd`, the program: build the table from the `public` list in its arguments, let its
//! launcher fill it with `add` and end setup with `seal`, then serve `/boot` read-only over 9P
//! until its endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-bootfsd`'s library, so host tests drive the same code
//! against the runtime's fake kernel (`tests/bootfsd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_bootfsd::server::{BUDGET, BootFs, Bootfs, COST, LIMITS};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint for `bootfsd` to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// The limits in this build do not fit the budget or the open-call headroom: a build-time
/// mistake, caught at the only moment it can be.
pub const BAD_LIMITS: u32 = 4;
/// The `public` list in the arguments was refused
/// ([`redoubt_bootfsd::SetupError`]): too many names, a name that is not one path component, or
/// the same name twice. `bootfsd` does not start on any of them, because a `/boot` that is not
/// what the manifest named is worse than none (TENETS.md 2, fail closed and loudly).
pub const BAD_PUBLIC_LIST: u32 = 5;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27). A server that cannot get one does not start.
pub const NO_RANDOM: u32 = 6;

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("bootfsd") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(handle);
    let Ok(fs) = BootFs::new(startup.args()) else { return BAD_PUBLIC_LIST };
    if !LIMITS.fits(&COST, BUDGET) {
        return BAD_LIMITS;
    }
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(fs, LIMITS, random) else { return BAD_LIMITS };
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // 9P and `ninep_common` in the skeleton; `add` and `seal` are ours.
                // The shared finish path completes a rejected reply with a handle-free
                // refusal (or exits under R4b), so an error leaves no open call here.
                let _ = server.serve_with(request, |s, request| serve_call::<Bootfs, _>(&mut s.fs, request));
            }
            // Nothing here is sent one-way: drop it, and close what it brought.
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            // No call is ever held open here, so no abandoned-call notice names one.
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
    }
}
