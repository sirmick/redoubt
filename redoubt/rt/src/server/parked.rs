//! Parked calls (CONTAINMENT.md, the shared server library; answers 81 and 82): calls a server
//! has taken and holds open while it waits for something else (a console read waiting for input,
//! a connect waiting for the network), answered later from the same thread.
//!
//! - **Admission.** Parking takes one [`Resource::InFlight`] of the caller's bucket and share; answering,
//!   abandoning or expiring the call gives it back. The caps leave headroom under `MAX_OPEN_CALLS`
//!   ([`Admission::new`]), so parked calls never stop the server taking new ones.
//! - **A server-side deadline.** Every parked call has one, at most [`Parked::new`]'s longest wait;
//!   [`Parked::expired`] hands back the calls past it, for the server to answer (with its protocol's timeout
//!   error), so a client that parks calls and waits cannot pin them for good.
//! - **`serve` before resuming** (answer 82). Every call handed back is made the thread's current call first
//!   ([`Request::serve`]), so a crash while working on it blames its caller, not whoever sent the call taken
//!   most recently.
//! - **Abandoned calls** (answer 81). [`Parked::abandoned`] replies to the call at once, which frees it (the
//!   reply reaches nobody), and hands back the server's state for it.
//!
//! - **Ahead of admission.** A request the server answers at once takes no admission, so one that must always
//!   get through (the steward ending a lease for its sponsor, answer 90) is answered straight from the
//!   receive loop, never parked; the caps' headroom under `MAX_OPEN_CALLS`
//!   ([`super::admit::OPEN_CALL_HEADROOM`]) leaves room to take it however full the buckets are
//!   (`tests/parked.rs` shows the loop).
//!
//! The table belongs to one thread: a call is replied to by the thread that took it
//! (KERNEL-SPEC.md, `reply`), and its abandoned-call notice arrives at that thread's `receive`.

use alloc::vec::Vec;

use redoubt_sys::Error;

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

/// The calls one thread has parked, each with the server's state for it (`T`).
pub struct Parked<T> {
    admission: Admission,
    calls: Vec<Call<T>>,
    longest: u64,
}

impl<T> Parked<T> {
    /// Parks calls under `admission` (its `in_flight` caps), each for at most `longest`
    /// microseconds.
    pub fn new(admission: Admission, longest: u64) -> Parked<T> {
        Parked { admission, calls: Vec::new(), longest }
    }

    /// Parks `request` until `deadline` (µs since boot; clamped to `now` + the longest wait),
    /// charged to its caller's bucket and `share`. Refused when the caller's bucket or share is
    /// full, or there is no memory; the request comes back to be answered now.
    #[allow(clippy::result_large_err)] // the request comes back by value, as `Request::reply`'s does
    pub fn park(
        &mut self,
        request: Request,
        share: u64,
        state: T,
        now: u64,
        deadline: u64,
    ) -> Result<(), NotParked> {
        let key = AdmitKey::of(&request.caller);
        if self.calls.try_reserve(1).is_err() || self.admission.admit(key, share, Resource::InFlight).is_err()
        {
            return Err(NotParked(request));
        }
        let deadline = deadline.min(now.saturating_add(self.longest));
        self.calls.push(Call { request, key, share, deadline, state });
        Ok(())
    }

    /// How many calls are parked.
    pub fn len(&self) -> usize { self.calls.len() }

    pub fn is_empty(&self) -> bool { self.calls.is_empty() }

    /// The earliest deadline, to bound the thread's next `receive`.
    pub fn next_deadline(&self) -> Option<u64> { self.calls.iter().map(|c| c.deadline).min() }

    /// Removes the call at `index`, releasing its admission.
    fn take(&mut self, index: usize) -> (Request, T) {
        let call = self.calls.swap_remove(index);
        self.admission.release(call.key, call.share, Resource::InFlight);
        (call.request, call.state)
    }

    /// Hands back the parked call `id` to be worked on and answered, after making it the
    /// thread's current call (`serve`). `None` if no such call is parked. If `serve` fails the
    /// call is not an open call of this thread (a server bug: parked from another thread), so
    /// nothing can answer it here; it is dropped and the error comes back.
    pub fn resume(&mut self, id: core::num::NonZeroU64) -> Option<Result<(Request, T), Error>> {
        let index = self.calls.iter().position(|c| c.request.id() == id)?;
        let served = self.calls[index].request.serve();
        let call = self.take(index);
        Some(served.map(|()| call))
    }

    /// The first parked call whose state `pick` accepts, resumed as [`Parked::resume`] does:
    /// for servers that resume by what the call waits for (input on a channel) rather than by
    /// id.
    pub fn resume_first(&mut self, mut pick: impl FnMut(&T) -> bool) -> Option<Result<(Request, T), Error>> {
        let id = self.calls.iter().find(|c| pick(&c.state))?.request.id();
        self.resume(id)
    }

    /// One call past its deadline at `now`, resumed as [`Parked::resume`] does, for the server
    /// to answer with its protocol's timeout error. Call until `None`.
    pub fn expired(&mut self, now: u64) -> Option<Result<(Request, T), Error>> {
        let id = self.calls.iter().find(|c| c.deadline <= now)?.request.id();
        self.resume(id)
    }

    /// An abandoned-call notice for `id` ([`crate::ipc::Event::Abandoned`]): replies to the call
    /// at once with `words` (the reply reaches nobody; replying frees the call and its lend) and
    /// hands back the server's state for it. `None` if no such call is parked.
    pub fn abandoned(&mut self, id: core::num::NonZeroU64, words: &Words) -> Option<T> {
        let index = self.calls.iter().position(|c| c.request.id() == id)?;
        let (request, state) = self.take(index);
        // The call is open until this reply; the reply's words cannot fail to encode.
        let _ = request.reply(words, &[]);
        Some(state)
    }

    /// Admission for parked calls.
    pub fn admission(&self) -> &Admission { &self.admission }
}
