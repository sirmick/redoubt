//! `keyd`, the program: read the keys out of the startup block, then answer calls on the
//! endpoint named `keyd` until that endpoint is destroyed.
//!
//! Everything it can do is in `redoubt-keyd`'s library, so host tests drive the same code
//! against the runtime's fake kernel (`tests/keyd.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use redoubt_keyd::keys::Keys;
use redoubt_keyd::server::{BUDGET, COST, KeyServer, LIMITS};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The startup block named no endpoint for `keyd` to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// The limits in this build do not fit the budget or the open-call headroom: a build-time
/// mistake, caught at the only moment it can be.
pub const BAD_LIMITS: u32 = 4;
/// A key argument was refused ([`redoubt_keyd::keys::KeyError`]): not `name,purpose,seed`, a
/// name outside the manifest's rule, an unknown purpose, a seed that is not 64 lower-case hex
/// digits, an all-zero seed (the one value `ed25519-compact` panics on), a repeated name, two
/// keys with the same public key, or more than [`redoubt_keyd::keys::MAX_KEYS`]. `keyd` does
/// not start on any of them: a key it cannot read is a key it cannot sign with, and serving
/// without it would look like the key simply not existing (TENETS.md 2, fail closed and
/// loudly).
pub const BAD_KEYS: u32 = 5;
/// The kernel would not give a random word. `keyd`'s first granted badge is drawn from one
/// (answer 126), and a predictable one is a hole across a restart, so it does not start
/// without it.
pub const NO_RANDOM: u32 = 6;

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("keyd") else { return NO_ENDPOINT };
    let Ok(keys) = Keys::from_args(startup.args()) else { return BAD_KEYS };
    // A predictable first granted badge would be a hole across a restart (answer 126), so a
    // `keyd` that cannot draw one does not start.
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = KeyServer::new(keys, LIMITS, &COST, BUDGET, random) else { return BAD_LIMITS };
    let endpoint = Endpoint::from_handle(handle);
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // Shared finish completes a rejected reply with a handle-free refusal (or
                // exits under R4b); serve rolls back provisional state before returning an error.
                let _ = server.serve(request);
            }
            // Every message of this protocol is a `call` (WIRE.md, answer 98). A `send` is
            // dropped, and what it brought is closed, so it cannot grow the handle table.
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            // No call is ever held open here: every request is answered as it is taken, so no
            // abandoned-call notice can name one.
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
    }
}
