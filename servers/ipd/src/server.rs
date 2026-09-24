//! `ipd`'s dispatch: what each event on its endpoint does. The program ([`crate`]'s `bin/ipd.rs`)
//! and the host tests drive the same [`Ipd`].
//!
//! **Labelled callers get nothing** (NAMESPACES.md; CONTAINMENT.md: `ipd` is a sink). A caller
//! whose budget carries any label is refused before its request is decoded: before 9P,
//! `ninep_common`, `grant` or admission, so it opens no bucket and cannot even make a socket by
//! reading `clone`, which the skeleton's label check alone would let it read.
//!
//! **Parked calls** wait on the network: a `ctl` read for at most [`CTL_WAIT_US`], a `data` read
//! or write for at most [`DATA_WAIT_US`], then `timeout`. They are served again once the stack
//! says they would not wait, and an abandoned one is answered at once.
//!
//! **Crash blame never lands on a parked caller** (KERNEL-SPEC.md, the current call). The stack
//! is polled only right after a `receive` that returned something other than a call, so no call
//! is current while smoltcp runs; [`Ipd::current`] tracks it and [`Ipd::poll`] checks it.

use core::num::NonZeroU64;

use redoubt_rt::abi::{Error, Handle, Handles};
use redoubt_rt::ipc::{Caller, Delivery, Request, Words};
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::minted::{Kernel, Minter};
use redoubt_rt::server::ninep::{NineError, NineServer, refuse};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::wire::proto::ipd::{ErrorCode, GrantReply, Message, Reply};
use redoubt_rt::wire::typed::error_reply;

use crate::fs::{At, NetFs, Node};
use crate::link::Netif;
use crate::scope::Scope;
use crate::stack::{Entropy, Owner, WaitFor};

/// How long a `ctl` read waits for a connect or an accept (µs).
pub const CTL_WAIT_US: u64 = 60_000_000;
/// How long a `data` read or write waits (µs).
pub const DATA_WAIT_US: u64 = 30_000_000;
/// `grant`'s opcode, and the ingress `frame`'s (NAMESPACES.md, `ipd`'s table).
pub const GRANT: u64 = 16;
pub const FRAME: u64 = 17;

/// What a parked call waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub owner: Owner,
    pub n: u32,
    pub what: WaitFor,
}

/// `ipd`: `/net`, its parked calls, and the badge `netd`'s frames arrive on.
pub struct Ipd<N: Netif, E: Entropy> {
    pub nine: NineServer<NetFs<N, E>>,
    ctl_parked: Parked<Waiting>,
    data_parked: Parked<Waiting>,
    ingress: u64,
    /// The call the kernel holds as this thread's current one, as far as `ipd` can tell.
    current: Option<NonZeroU64>,
    /// Polls made while a call was current: always 0 (checked by tests).
    pub polls_with_a_current_call: u64,
    /// What the last call parked waits for, so a round of resumes does not take it up again.
    parked_last: Option<Waiting>,
}

fn handles_of(request: &Request) -> Handles {
    let mut list = Handles::new();
    for handle in request.handles.as_slice().iter().flatten() {
        let _ = list.push(*handle);
    }
    list
}

fn close_all(handles: &Handles) {
    for handle in handles.as_slice() {
        let _ = redoubt_rt::handle::close(*handle);
    }
}

impl<N: Netif, E: Entropy> Ipd<N, E> {
    pub fn new(nine: NineServer<NetFs<N, E>>, ingress: u64) -> Ipd<N, E> {
        Ipd {
            nine,
            ctl_parked: Parked::new(CTL_WAIT_US),
            data_parked: Parked::new(DATA_WAIT_US),
            ingress,
            current: None,
            polls_with_a_current_call: 0,
            parked_last: None,
        }
    }

    pub fn fs(&mut self) -> &mut NetFs<N, E> { &mut self.nine.fs }

    /// Calls parked now.
    pub fn parked(&self) -> usize { self.ctl_parked.len() + self.data_parked.len() }

    pub fn current(&self) -> Option<NonZeroU64> { self.current }

    /// When the next parked call's deadline falls, if any.
    pub fn next_deadline(&self) -> Option<u64> {
        match (self.ctl_parked.next_deadline(), self.data_parked.next_deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// `receive` returned something other than a call: no call is current now.
    pub fn received_no_call(&mut self) { self.current = None; }

    /// A call arrived (it is now current). Labelled callers are refused first; everything else is
    /// 9P, `ninep_common` or `grant`, answered or parked.
    pub fn on_call(&mut self, request: Request, now: u64) {
        self.current = Some(request.id());
        self.nine.fs.now = now;
        if !request.caller.labels.as_slice().is_empty() {
            refuse_labelled(request);
            self.current = None;
            return;
        }
        self.serve(request, now);
    }

    /// Serves `request` (current), parking it if the file server asked it to wait.
    fn serve(&mut self, request: Request, now: u64) {
        let _ = self.nine.fs.take_wait();
        match self.nine.serve_parking(request, serve_own) {
            Ok(None) | Err(_) => self.current = None,
            Ok(Some(request)) => self.park(request, now),
        }
    }

    /// Parks a call the file server asked to hold. It stays current (nothing has replied to it)
    /// until the next `receive`.
    fn park(&mut self, request: Request, now: u64) {
        let Some((owner, n, what)) = self.nine.fs.take_wait() else {
            // A wait with nothing to wait for is a bug here; the caller is told, not left hanging.
            let _ = refuse(request, NineError::NOT_SUPPORTED);
            self.current = None;
            return;
        };
        let share = self.nine.share_of(&request.caller);
        let table = if what == WaitFor::Ctl { &mut self.ctl_parked } else { &mut self.data_parked };
        let waiting = Waiting { owner, n, what };
        self.parked_last = Some(waiting);
        if let Err(NotParked(request)) = table.park(self.nine.admission_mut(), request, share, waiting, now) {
            // Its bucket's parked calls are at their cap: answered now, with the socket's
            // `too_many`, rather than held.
            let _ = refuse(request, NineError("too_many"));
            self.current = None;
        }
    }

    /// A `send` arrived: a frame from `netd` on the ingress badge goes to the stack; anything else
    /// is dropped, its handles closed and its pages unmapped. Returns whether a socket may have
    /// changed.
    pub fn on_send(&mut self, delivery: Delivery, now: u64) -> bool {
        self.current = None;
        for handle in delivery.handles.as_slice().iter().flatten() {
            let _ = redoubt_rt::handle::close(*handle);
        }
        let from_netd = delivery.caller.badge == self.ingress && delivery.caller.labels.as_slice().is_empty();
        let (Some(page), true) = (delivery.transfer.as_ref(), from_netd) else { return false };
        match Message::decode(&delivery.words, page, 0) {
            Ok(Message::Frame(frame)) => self.nine.fs.stack.ingress(frame.frame, now),
            _ => false,
        }
    }

    /// An abandoned-call notice: if the call is parked here, it is answered at once (the reply
    /// reaches nobody) and its admission given back.
    pub fn on_abandoned(&mut self, id: NonZeroU64) {
        self.current = None;
        let admission = self.nine.admission_mut();
        if self.ctl_parked.abandoned(admission, id, &MALFORMED).is_none() {
            let _ = self.data_parked.abandoned(self.nine.admission_mut(), id, &MALFORMED);
        }
    }

    /// Answers every parked call past its deadline with `timeout`.
    pub fn expire(&mut self, now: u64) {
        for table in [&mut self.ctl_parked, &mut self.data_parked] {
            while let Some(call) = table.expired(self.nine.admission_mut(), now) {
                if let Ok((request, _)) = call {
                    let _ = refuse(request, NineError("timeout"));
                }
            }
        }
        self.current = None;
    }

    /// Polls the stack (egress, timers, lingering sockets), then serves again every parked call
    /// that would no longer wait. Only ever right after a `receive` that returned no call.
    pub fn poll(&mut self, now: u64) {
        if self.current.is_some() {
            self.polls_with_a_current_call += 1;
            debug_assert!(false, "the stack polled while a call was current");
        }
        self.nine.fs.now = now;
        self.nine.fs.stack.poll(now);
        self.resume(now);
    }

    /// Serves again the parked calls whose sockets have moved. A call that parks again (another
    /// call on the same socket took what it was waiting for) waits for the next round: it is
    /// added to what still waits, so one round serves each call at most once.
    pub fn resume(&mut self, now: u64) {
        let mut waiting = self.still_waiting();
        for ctl in [true, false] {
            loop {
                let table = if ctl { &mut self.ctl_parked } else { &mut self.data_parked };
                let Some(call) = table.resume_first(self.nine.admission_mut(), |w| !waiting.contains(w))
                else {
                    break;
                };
                let Ok((request, _)) = call else { continue };
                // `resume` made it current (`serve`).
                self.current = Some(request.id());
                self.nine.fs.now = now;
                self.parked_last = None;
                self.serve(request, now);
                if let Some(again) = self.parked_last.take() {
                    waiting.push(again);
                }
            }
        }
    }

    /// Every (socket, wait) that would still wait now: a parked call on anything else is served
    /// again.
    fn still_waiting(&self) -> alloc::vec::Vec<Waiting> {
        let stack = &self.nine.fs.stack;
        stack
            .waits()
            .filter(|(owner, n, what)| stack.would_wait(*owner, *n, *what))
            .map(|(owner, n, what)| Waiting { owner, n, what })
            .collect()
    }

    /// The next `receive`'s timeout (µs from `now`): the nearest parked deadline or stack timer.
    pub fn timeout(&mut self, now: u64) -> Option<u64> {
        let parked = self.next_deadline().map(|d| d.saturating_sub(now));
        let stack = self.nine.fs.stack.poll_delay(now);
        match (parked, stack) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Refuses a labelled caller without decoding anything: `Rerror` for 9P, `not_permitted` (2)
/// for any typed opcode. Handles it brought are closed.
fn refuse_labelled(request: Request) {
    let handles = handles_of(&request);
    if request.words[0] == 0 {
        close_all(&handles);
        let _ = refuse(request, NineError::PERMISSION);
    } else {
        let outcome = Outcome {
            words: error_reply(ErrorCode::NotPermitted.code()),
            send: Handles::new(),
            close: handles,
        };
        let _ = finish(request, &outcome);
    }
}

/// `ipd`'s own opcodes on its 9P endpoint: `grant`. A `frame` is a `send`, so as a call it is
/// malformed, like any other opcode.
fn serve_own<N: Netif, E: Entropy>(
    nine: &mut NineServer<NetFs<N, E>>,
    mut request: Request,
) -> Result<(), Error> {
    let caller = request.caller;
    let words = request.words;
    let handles = handles_of(&request);
    let missing = handles.as_slice().len() != request.handles.as_slice().len();
    let mut kernel = Kernel(request.id());
    let (outcome, minted) = if missing {
        (Outcome { words: MALFORMED, send: Handles::new(), close: handles }, None)
    } else {
        answer_grant(nine, &caller, &words, &handles, request.lend(), &mut kernel)
    };
    let sent = finish(request, &outcome);
    // The connection is undone unless the reply was delivered with its handle (answer 168).
    if let Some(badge) = minted {
        if !sent.as_ref().is_ok_and(|o| o.accepted(1)) {
            nine.unmint(badge);
            nine.fs.collect_scopes();
        }
    }
    sent.map(|_| ())
}

/// `grant(scope)`: a connection whose scope is `scope`, which must be canonical and no wider than
/// the caller's own. Makes no system call but through `kernel`. Returns the outcome and, if a
/// connection was minted, its badge (to undo if the reply is not delivered).
pub fn answer_grant<N: Netif, E: Entropy>(
    nine: &mut NineServer<NetFs<N, E>>,
    caller: &Caller,
    words: &Words,
    handles: &Handles,
    lend: &mut [u8],
    kernel: &mut impl Minter,
) -> (Outcome, Option<u64>) {
    let none = Handles::new();
    let fail = |code: ErrorCode| (Outcome { words: code.encode(), send: none, close: *handles }, None);
    let requested = match Message::decode(words, lend, handles.as_slice().len()) {
        Ok(Message::Grant(grant)) => match Scope::decode(grant.scope) {
            Ok(scope) => scope,
            Err(_) => return fail(ErrorCode::NotPermitted),
        },
        _ => return (Outcome { words: MALFORMED, send: none, close: *handles }, None),
    };
    let Some((_, held)) = nine.fs.scope_of(caller) else { return fail(ErrorCode::NotPermitted) };
    if !held.narrows(&requested) {
        return fail(ErrorCode::NotPermitted);
    }
    let scope = nine.fs.add_scope(requested);
    let root = Node { scope, at: At::Root };
    nine.fs.set_granting(true);
    let made = nine.mint_rooted(caller, (root, root.qid()), kernel);
    nine.fs.set_granting(false);
    let (handle, id, badge): (Handle, u64, u64) = match made {
        Ok(made) => made,
        Err(e) => {
            nine.fs.collect_scopes();
            let code = if e == NineError::TOO_MANY { ErrorCode::TooMany } else { ErrorCode::NotPermitted };
            return fail(code);
        }
    };
    match Reply::Grant(GrantReply { id }).encode(lend) {
        Ok(words) => {
            let send = Handles::from_slice(&[handle]).unwrap_or(none);
            (Outcome { words, send, close: send }, Some(badge))
        }
        Err(_) => {
            nine.unmint(badge);
            nine.fs.collect_scopes();
            let close = Handles::from_slice(&[handle]).unwrap_or(none);
            (Outcome { words: MALFORMED, send: none, close }, None)
        }
    }
}
