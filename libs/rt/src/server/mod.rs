//! The shared server library (servers/serving.md): what every system server serving more than one
//! account links, so that admission and the label check are written once.
//!
//! - [`admit`]: per-(account, label set) limits on what a client holds in the server, with a fair share per
//!   badge.
//! - [`check`]: no read up, no write down.
//! - [`ninep`]: a 9P2000 server skeleton that applies both, and keeps `..` inside a fid's root.
//! - [`minted`]: the capabilities a server mints for its clients, and their release.
//! - [`typed`]: typed-message dispatch over the generated codecs.
//! - [`parked`]: calls held open for later, each with a deadline, resumed under `serve`.
//! - [`serve`]: the one receive loop of a server that takes only calls; [`close_delivery`] for the loops that
//!   keep their own shape.

use redoubt_sys::{Error, FOREVER};

use crate::exit;
use crate::handle::Endpoint;
use crate::ipc::{Delivery, Event, Held, Request};

pub mod admit;
pub mod consol;
pub mod label;
pub mod minted;
pub mod ninep;
pub mod parked;
pub mod typed;

pub use admit::{
    Admission, AdmitKey, Cost, LAUNCHED_STACK_PAGES, Limits, MAX_BUCKETS, Override, Refused, Resource,
    Unsized, buckets, budget, budget_of, own_args,
};

/// The reply words of a malformed request, in 9P calls and every typed protocol alike: status 1,
/// `Malformed` (servers/wire.md), which the wire generator reserves in every error table.
pub const MALFORMED: crate::ipc::Words = redoubt_wire::typed::error_reply(redoubt_wire::typed::MALFORMED);
pub use label::{Access, Denied, check};

/// Receives on `endpoint` until it dies, handing each call to `f`, for a server that takes only
/// calls; returns the exit code.
///
/// `f` owns the call and answers it; what it returns says only whether it holds the reply
/// ([`Served`]). So `f` is a handler that replies to every call itself and returns how the reply
/// went, as every handler on the shared finish path ([`typed::finish`]) does: a rejected reply is
/// completed with a handle-free refusal, and the handler undoes what the request made before it
/// returns (servers/serving.md, "Replies and rollback"), so an error leaves no open call and
/// nothing to act on. Or it holds a reply that carries no handles ([`typed::finish_held`]), and
/// the loop makes it with its next receive, as one call (a server's turn), with nothing run in
/// between. A call `f` drops unanswered is answered `Malformed` by [`Request`]'s drop. A handler
/// whose error still needs a reply or an undo does not use `serve`.
///
/// A `send` is dropped and the handles it brought closed, so it cannot grow the handle table.
/// Abandoned-call and exit notices are ignored: such a server holds no call open and starts no
/// child. An endpoint's receive never returns an interrupt. The endpoint's death ends the server
/// with [`exit::OK`], any other failure with [`exit::RECEIVE_FAILED`].
pub fn serve<R: Served>(endpoint: &Endpoint, mut f: impl FnMut(Request) -> R) -> u32 {
    let mut held = None;
    loop {
        match endpoint.receive_after(held.take(), FOREVER, 0) {
            Ok(Event::Call(request)) => held = f(request).held(),
            Ok(Event::Send(delivery)) => close_delivery(&delivery),
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => return exit::OK,
            Err(_) => return exit::RECEIVE_FAILED,
        }
    }
}

/// What a [`serve`] handler returns: how its reply went, or the reply it holds for the loop's
/// next receive.
pub trait Served {
    fn held(self) -> Option<Held>;
}

/// How a reply went, or nothing: nothing is held.
impl<T, E> Served for Result<T, E> {
    fn held(self) -> Option<Held> { None }
}

impl Served for () {
    fn held(self) -> Option<Held> { None }
}

/// A reply held, or made ([`typed::finish_held`]).
impl Served for typed::Finished {
    fn held(self) -> Option<Held> {
        match self {
            typed::Finished::Held(held) => Some(held),
            typed::Finished::Made(_) => None,
        }
    }
}

/// Closes the handles `delivery` brought, so a `send` cannot grow the handle table: for a server
/// that drops a send, or keeps only its words and pages.
pub fn close_delivery(delivery: &Delivery) {
    for handle in delivery.handles.as_slice().iter().flatten() {
        let _ = crate::handle::close(*handle);
    }
}
