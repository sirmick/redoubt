//! `piped`, the program: serve one session's pipes over 9P until its endpoint is gone.
//!
//! Everything it can do is in `redoubt-piped`'s library and here, so host tests drive the same code
//! against the runtime's fake kernel (`tests/piped.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_piped::server::{BUDGET, COST, Pipes, limits};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Request;
use redoubt_rt::server::ninep::{Around, NineError, NineServer, WORDS_9P, refuse, refuse_malformed};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The name its launcher gives the endpoint it serves (docs/userland/beamlet.md, "Natives": `launch`
/// with `serve`).
pub const ENDPOINT: &str = "serve";

/// The startup block named no endpoint to serve.
pub const NO_ENDPOINT: u32 = 2;
/// No `buckets=N` in the arguments, or one whose buckets at their caps do not fit the budget or
/// the open-call headroom: its launcher sized it wrongly, and it does not guess.
pub const BAD_LIMITS: u32 = 4;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27).
pub const NO_RANDOM: u32 = 7;

/// Answers `request`, or parks it if its pipe cannot answer yet.
fn serve_or_park(
    server: &mut NineServer<Pipes>,
    parked: &mut Parked<()>,
    request: Request,
    now: u64,
) -> Result<(), Error> {
    // `piped` serves no typed protocol of its own: only 9P and `ninep_common`.
    let held = server.serve_parking(request, |_, request| refuse_malformed(request).map(|()| None))?;
    let Some(request) = held else { return Ok(()) };
    let charge = server.charge_of(&request.caller);
    match parked.park(server.admission_mut(), request, charge, (), now) {
        Ok(()) => Ok(()),
        // Its share is full of waiting calls, or there is no memory for one more: it is told so,
        // rather than left waiting on a call the server cannot hold.
        Err(NotParked(request)) => refuse(request, NineError::TOO_MANY),
    }
}

/// The parked calls, beside the skeleton's loop.
pub struct Waiting(pub Parked<()>);

impl Around<Pipes> for Waiting {
    fn call(&mut self, server: &mut NineServer<Pipes>, request: Request, now: u64) {
        // A failed reply means the caller is gone; there is nobody to tell.
        let _ = serve_or_park(server, &mut self.0, request, now);
    }

    /// When a pipe has moved since the last turn (bytes went in or out, an end was let go, a pipe
    /// was removed), every parked call is served again, longest wait first, and so is every
    /// multiplexed request that waits: a call still unanswerable is parked again. Serving one
    /// may move a pipe again, so the turn repeats until nothing moves.
    fn turn(&mut self, server: &mut NineServer<Pipes>, now: u64) {
        while server.fs.take_moved() {
            for _ in 0..self.0.len() {
                let Some(call) = self.0.resume_first(server.admission_mut(), |_| true) else { break };
                // A call this thread cannot serve is a server bug; it is gone either way.
                let Ok((request, ())) = call else { continue };
                let _ = serve_or_park(server, &mut self.0, request, now);
            }
            server.wake(now);
        }
    }

    /// A caller gave up on a parked call: replying frees the call and its lend, and the reply
    /// reaches nobody (R3).
    fn abandoned(&mut self, server: &mut NineServer<Pipes>, id: NonZeroU64) {
        self.0.abandoned(server.admission_mut(), id, &WORDS_9P);
    }
}

/// Serves until the endpoint is gone.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle(ENDPOINT) else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(handle);
    let args: Vec<&str> = startup.args().collect();
    let Ok(buckets) = redoubt_rt::server::buckets(&args) else { return BAD_LIMITS };
    let limits = limits(buckets);
    if !limits.fits(&COST, BUDGET) {
        return BAD_LIMITS;
    }
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(Pipes::new(), limits, random) else { return BAD_LIMITS };
    // A call on a pipe waits on another stage, for as long as that takes: what reclaims it is its
    // caller giving up or dying, which arrives as an abandoned-call notice.
    server.requests_wait(FOREVER);
    server.run_around(&endpoint, Waiting(Parked::new(FOREVER)))
}
