//! Parked calls (servers/serving.md, "Parked calls"; R28): calls a server has taken and holds
//! open while it waits for something else (a console read waiting for input, a connect waiting
//! for the network), answered later from the same thread.
//!
//! - **Admission.** Parking takes one [`Resource::InFlight`] of the caller's bucket and share; answering,
//!   abandoning or expiring the call gives it back. The caps leave headroom under `MAX_OPEN_CALLS`
//!   ([`Admission::new`]), so parked calls never stop the server taking new ones. The [`Admission`] is the
//!   server's, passed in on every call here rather than owned, so that a 9P server's fids and its parked
//!   calls are charged in the same buckets and shares ([`crate::server::ninep::NineServer::admission_mut`]).
//! - **A server-side deadline.** Every parked call has one, at most [`Parked::new`]'s longest wait;
//!   [`Parked::expired`] hands back the calls past it, for the server to answer (with its protocol's timeout
//!   error), so a client that parks calls and waits cannot pin them for good. A server whose calls wait on a
//!   person rather than on the machine (`consoled`, for a key press) passes [`FOREVER`] as the longest wait:
//!   such a call has no deadline and never expires, and what reclaims it is its caller giving up, which
//!   arrives as an abandoned-call notice.
//! - **`serve` before resuming** (kernel/processes.md R21). Every call handed back is made the
//!   thread's current call first ([`Request::serve`]), so a crash while working on it blames its
//!   caller, not whoever sent the call taken most recently.
//! - **Abandoned calls** (kernel/ipc.md R3). [`Parked::abandoned`] replies to the call at once,
//!   which frees it (the reply reaches nobody), and hands back the server's state for it.
//!
//! - **Ahead of admission.** A request the server answers at once takes no admission, so one that
//!   must always get through (the steward ending a lease for its sponsor, servers/serving.md R26)
//!   is answered straight from the receive loop, never parked; the caps' headroom under
//!   `MAX_OPEN_CALLS` ([`super::admit::OPEN_CALL_HEADROOM`]) leaves room to take it however full
//!   the buckets are (`tests/parked.rs` shows the loop).
//!
//! The table belongs to one thread: a call is replied to by the thread that took it
//! (kernel/ipc.md, "The calls"), and its abandoned-call notice arrives at that thread's
//! `receive`.
//! The loop around it has one shape (`tests/parked.rs` runs it):
//!
//! ```text
//! loop {
//!     let now = time_now()?;
//!     while let Some(call) = parked.expired(&mut admission, now) { ...answer it with a timeout... }
//!     let timeout = parked.next_deadline().map_or(FOREVER, |d| d.saturating_sub(now).max(1));
//!     match endpoint.receive(timeout, 0)? {
//!         Event::Call(request) => ...park it, answer it, or answer it ahead of admission...,
//!         Event::Abandoned(id) => { parked.abandoned(&mut admission, id, &words); }
//!         ...
//!     }
//! }
//! ```
//!
//! **Joined to the 9P skeleton.** [`crate::server::ninep::NineServer::serve_parking`]
//! hands back a request the file server asked to hold, with its T-message untouched in its lend;
//! the server parks it here and serves it again when it can be answered. `consoled` is the
//! worked example.

use alloc::vec::Vec;

use redoubt_sys::{Error, FOREVER};

use super::admit::{Admission, AdmitKey, Resource};
use crate::ipc::{Request, Words};

/// Why a call could not be parked. The request comes back so the server can answer it now.
#[derive(Debug)]
pub struct NotParked(pub Request);

struct Call<T> {
    request: Request,
    key: AdmitKey,
    share: u64,
    deadline: u64,
    state: T,
}

/// The calls one thread has parked, each with the server's state for it (`T`). The server's
/// [`Admission`] is passed to every method rather than owned, so that a 9P server charges its
/// fids and its parked calls in one set of buckets and shares.
pub struct Parked<T> {
    calls: Vec<Call<T>>,
    longest: u64,
}

impl<T> Parked<T> {
    /// Parks calls for at most `longest` microseconds each; [`FOREVER`] for calls that wait on a
    /// person and have no deadline.
    pub fn new(longest: u64) -> Parked<T> { Parked { calls: Vec::new(), longest } }

    /// Parks `request` at `now` (µs since boot) for at most the longest wait, charged to its
    /// caller's bucket and `share` in `admission` (its `in_flight` caps). Refused when the
    /// caller's bucket or share is full, or there is no memory; the request comes back to be
    /// answered now.
    #[allow(clippy::result_large_err)] // the request comes back by value, as `Request::reply`'s does
    pub fn park(
        &mut self,
        admission: &mut Admission,
        request: Request,
        share: u64,
        state: T,
        now: u64,
    ) -> Result<(), NotParked> {
        let key = AdmitKey::of(&request.caller);
        if self.calls.try_reserve(1).is_err() || admission.admit(key, share, Resource::InFlight).is_err() {
            return Err(NotParked(request));
        }
        // `FOREVER` is "no deadline" (kernel/abi.md), and saturating past it is too.
        let deadline = now.saturating_add(self.longest);
        self.calls.push(Call { request, key, share, deadline, state });
        Ok(())
    }

    /// How many calls are parked.
    pub fn len(&self) -> usize { self.calls.len() }

    pub fn is_empty(&self) -> bool { self.calls.is_empty() }

    /// The earliest deadline, to bound the thread's next `receive`; `None` when nothing is
    /// parked or every parked call waits without one ([`FOREVER`]).
    pub fn next_deadline(&self) -> Option<u64> {
        self.calls.iter().map(|c| c.deadline).filter(|d| *d != FOREVER).min()
    }

    /// Removes the call at `index`, releasing its admission.
    fn take(&mut self, admission: &mut Admission, index: usize) -> (Request, T) {
        // In order, not `swap_remove`: `resume_first` serves the call that has waited longest.
        let call = self.calls.remove(index);
        admission.release(call.key, call.share, Resource::InFlight);
        (call.request, call.state)
    }

    /// Hands back the parked call `id` to be worked on and answered, after making it the
    /// thread's current call (`serve`). `None` if no such call is parked. If `serve` fails the
    /// call is not an open call of this thread (a server bug: parked from another thread), so
    /// nothing can answer it here; it is dropped and the error comes back.
    pub fn resume(
        &mut self,
        admission: &mut Admission,
        id: core::num::NonZeroU64,
    ) -> Option<Result<(Request, T), Error>> {
        let index = self.calls.iter().position(|c| c.request.id() == id)?;
        let served = self.calls[index].request.serve();
        let call = self.take(admission, index);
        Some(served.map(|()| call))
    }

    /// The first parked call whose state `pick` accepts, resumed as [`Parked::resume`] does:
    /// for servers that resume by what the call waits for (input on a channel) rather than by
    /// id. The calls are kept in the order they were parked, so the longest wait is served
    /// first.
    pub fn resume_first(
        &mut self,
        admission: &mut Admission,
        mut pick: impl FnMut(&T) -> bool,
    ) -> Option<Result<(Request, T), Error>> {
        let id = self.calls.iter().find(|c| pick(&c.state))?.request.id();
        self.resume(admission, id)
    }

    /// One call past its deadline at `now`, resumed as [`Parked::resume`] does, for the server
    /// to answer with its protocol's timeout error. Call until `None`. A call parked without a
    /// deadline ([`FOREVER`]) never comes back this way.
    pub fn expired(&mut self, admission: &mut Admission, now: u64) -> Option<Result<(Request, T), Error>> {
        let id = self.calls.iter().find(|c| c.deadline != FOREVER && c.deadline <= now)?.request.id();
        self.resume(admission, id)
    }

    /// An abandoned-call notice for `id` ([`crate::ipc::Event::Abandoned`]): replies to the call
    /// at once with `words` (the reply reaches nobody; replying frees the call and its lend) and
    /// hands back the server's state for it. `None` if no such call is parked.
    pub fn abandoned(
        &mut self,
        admission: &mut Admission,
        id: core::num::NonZeroU64,
        words: &Words,
    ) -> Option<T> {
        let index = self.calls.iter().position(|c| c.request.id() == id)?;
        let (request, state) = self.take(admission, index);
        // The call is open until this reply; the reply's words cannot fail to encode.
        let _ = request.reply(words, &[]);
        Some(state)
    }
}
