//! Endpoints the VM serves (docs/userland/beamlet.md, "Natives": `serve/1`, `reply/2`): a thread per
//! served endpoint takes its calls and hands each to the VM as a request, on the serving library's
//! admission and parked calls.
//!
//! - **Admission and the deadline are the library's** (servers/serving.md R26, R28): each call is parked
//!   under its caller's bucket and share, at most [`REQUEST_WAIT_US`]; one past it is answered by the thread
//!   with the one status every protocol has, `malformed` (code 1), and its admission given back. So a request
//!   the Erlang side never answers is ended by the thread, never held by the VM, and a caller whose bucket or
//!   share is full is answered at once, `malformed` too.
//! - **To the VM through a queue, never a wait on it**: the call's words, badge, account, labels and handles
//!   (by number: the thread is of the same process, and the handles are the VM's from then on) and a copy of
//!   its lent bytes go into the forwards queue the VM takes at its poll ([`Forwards`], bounded by
//!   [`MAX_FORWARDS`]), and the thread goes straight back to its receive. A VM in its idle receive is woken
//!   by one send through a badge minted for the thread ([`BADGE`] + its index) carrying nothing; a busy one
//!   finds the queue at its next poll. Once the thread forwarded each request by a `send` to the VM's
//!   endpoint that waited for the VM to take it: a VM busy in Erlang took one per scheduler turn, and a burst
//!   of the steward's `lease_ended` sends was taken one turn each, the last of N about N turns late and one
//!   lost at the steward's bound (docs/userland/beamlet.md, "The forwards queue"). A `send` to the endpoint
//!   goes the same way, with nothing to answer.
//! - **`reply/2` goes back on the served endpoint itself**, as a `send` through a badge the VM minted from
//!   its receive right ([`SELF`]): only the VM holds it, so the thread takes a send with that badge as an
//!   answer and any other as a request. The answer names the call by the number the thread gave it; an answer
//!   to a call that has ended already (expired, or abandoned by its caller) is dropped.
//! - **A launch in a budget the VM carved is given a served endpoint as a send right with a badge of its
//!   own** ([`Served::admit`]), minted from the receive right and stamped with that budget, never the receive
//!   right itself: the handles a request with that badge brings die with the budget, as the program's mints
//!   do, and go under its [`Life`], so a collection closes nothing at their indices once the kernel has freed
//!   them (docs/userland/beamlet.md, "Handles are resource terms").

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use beamlet_vm::platform::{Event, Message, Object, Refused};
use beamlet_vm::sync::Lock;
use redoubt_rt::abi::{FOREVER, Handle, Handles, MAX_LEND_PAGES, PAGE_SIZE};
use redoubt_rt::handle::{Budget, Endpoint, time_now};
use redoubt_rt::ipc::{Buffer, Delivery, Request};
use redoubt_rt::server::parked::Parked;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::server::{Admission, AdmitKey, Limits, MALFORMED, close_delivery};

use crate::system::{Cap, Life, kernel};

/// The most endpoints one VM serves.
pub const MAX_SERVED: usize = 2;

/// The badge of serve thread `i`'s wake-ups is `BADGE + i`: above every call thread's.
pub const BADGE: u64 = 0x200;

/// The badge the VM answers with, minted from the served endpoint's receive right: the largest a
/// mint gives, which no client's badge is.
pub const SELF: u64 = u32::MAX as u64;

/// The first badge minted for a launch given a served endpoint; each launch on that endpoint takes
/// the next, below [`SELF`]. A request with a badge from here on that no live launch holds is one
/// a destroyed launch's program sent before its budget went: what it brought is closed when its
/// terms go, dead or live (I17).
pub const LAUNCH_BADGE: u64 = 0x8000_0000;

/// The longest a request waits for its answer (µs): past it, the thread answers it.
pub const REQUEST_WAIT_US: u64 = 5_000_000;

/// The serve thread's stack, in pages.
const STACK_PAGES: usize = 4;

/// The bytes of an answer's header, before its lent bytes.
const HEADER: usize = 256;

/// The longest lent part a request or an answer carries: what fits an answer's transfer after the
/// header, and the same for a request, whose bytes are copied into the forwards queue.
pub const MAX_BYTES: usize = MAX_LEND_PAGES * PAGE_SIZE - HEADER;

/// A serve thread's wake-up's first word: the VM's forwards queue has something for it.
const WAKE: u64 = 0x3a4e;

/// The admission of a served endpoint: a few callers, a few requests each.
const LIMITS: Limits = Limits { buckets: 8, in_flight: 4, files: 0, state: 0, requests: 0, pages: 0 };

/// The calls one served endpoint's admission parks at once, each forwarded once: its buckets
/// times the requests in flight per bucket.
const PARKED: usize = (LIMITS.buckets * LIMITS.in_flight) as usize;

/// The one-way sends one served endpoint holds for the VM beyond its parked calls: what the
/// steward can send a session before its VM polls (docs/servers/steward.md, "Leases": one
/// `lease_ended` per live lease of the sponsor's, sixteen under the latency manifest, one
/// `server_restarted` per slot of the session's binding table, and `approval_waiting`), with
/// room over it.
pub const SENDS: usize = 32;

/// The most requests and sends the serve threads hold for the VM's take: per served endpoint, its
/// parked calls and [`SENDS`] one-way sends. A thread finding the queue full waits for the VM's
/// next poll, as every forward once did (docs/userland/beamlet.md, "The forwards queue").
pub const MAX_FORWARDS: usize = MAX_SERVED * (PARKED + SENDS);

/// How long a wake of an idling VM waits to be taken (µs) before the thread looks again whether
/// the VM still says it idles: the wake is taken at once by a VM in its idle receive, and this is
/// the step while the VM is between saying so and entering it.
const WAKE_US: u64 = 1_000;

/// A request or a one-way send a serve thread took, for the VM: which thread's, the call's number
/// (none for a send), the caller's badge, account and labels, its words, its handles, the VM's
/// from here on by number, and a copy of its lent bytes.
struct Forwarded {
    thread: usize,
    number: Option<u64>,
    badge: u64,
    account: u64,
    labels: Vec<u64>,
    words: [u64; 4],
    handles: Vec<Handle>,
    buffer: Option<Vec<u8>>,
}

/// What the serve threads hand the VM, taken at its poll ([`Served::take`]), and whether the VM
/// is in its idle receive or about to enter it, told under the same lock as the queue is pushed
/// and looked at, so a thread that queues sees the VM idle, or the VM's last look before its
/// receive sees the queue: never both miss.
pub(crate) struct Forwards {
    queue: Lock<Queue>,
}

struct Queue {
    items: VecDeque<Forwarded>,
    idle: bool,
}

/// A request the VM has, behind its resource: where to answer it.
pub(crate) struct Pending {
    answer: Arc<Endpoint>,
    number: u64,
}

/// One endpoint served: its asker, its answering handle, the receive right, and the resource that
/// keeps that right open for as long as its thread serves it, which is the VM's life.
struct Thread {
    asker: u64,
    answer: Arc<Endpoint>,
    endpoint: Handle,
    _held: Object,
    /// The badge minted for each launch given the endpoint, and the budget it runs in; one whose
    /// budget has gone leaves at the next launch.
    launches: Vec<(u64, Arc<Life>)>,
    /// The next launch's badge.
    next: u64,
}

impl Thread {
    /// The budget a request's handles die with, by the request's badge
    /// ([`LAUNCH_BADGE`]): a live launch's, one already gone for a badge of a launch's that is
    /// forgotten, none for any other client.
    fn life(&self, badge: u64) -> Option<Arc<Life>> {
        match self.launches.iter().find(|(b, _)| *b == badge) {
            Some((_, life)) => Some(Arc::clone(life)),
            None if (LAUNCH_BADGE..SELF).contains(&badge) => Some(Arc::new(Life::ended())),
            None => None,
        }
    }
}

/// The endpoints served, and the queue their threads forward through.
pub(crate) struct Served {
    threads: Vec<Thread>,
    forwards: Arc<Forwards>,
}

impl Default for Served {
    fn default() -> Served {
        let queue = Lock::new(Queue { items: VecDeque::new(), idle: false });
        Served { threads: Vec::new(), forwards: Arc::new(Forwards { queue }) }
    }
}

impl Served {
    /// Whether an endpoint is served: its requests may come at any time.
    pub(crate) fn busy(&self) -> bool { !self.threads.is_empty() }

    /// Serves the receive right `endpoint`, which `held` keeps open, for `asker`; an endpoint
    /// already served is refused, since its answers could not be told apart.
    pub(crate) fn serve(
        &mut self,
        wake: &Endpoint,
        asker: u64,
        endpoint: Handle,
        held: Object,
    ) -> Result<(), Refused> {
        if self.threads.iter().any(|t| t.endpoint == endpoint) {
            return Err(Refused("already_served"));
        }
        if self.threads.len() >= MAX_SERVED {
            return Err(Refused("too_many"));
        }
        let name = |e| Refused(kernel(e));
        let receive = Endpoint::from_handle(endpoint);
        // Only a receive right mints: anything else is refused here, by the kernel.
        let answer = receive.mint(NonZeroU64::new(SELF).ok_or(Refused("protocol"))?, None).map_err(name)?;
        let i = self.threads.len();
        let badge = NonZeroU64::new(BADGE + i as u64).ok_or(Refused("protocol"))?;
        let woken = wake.mint(badge, None).map_err(name)?;
        let forwards = Arc::clone(&self.forwards);
        let body = alloc::boxed::Box::new(move || serve(receive, woken, &forwards, i));
        redoubt_rt::thread::spawn(body, STACK_PAGES).map_err(name)?;
        self.threads.push(Thread {
            asker,
            answer: Arc::new(answer),
            endpoint,
            _held: held,
            launches: Vec::new(),
            next: LAUNCH_BADGE,
        });
        Ok(())
    }

    /// For a launch in `budget`, which dies with `life`, given the receive right `endpoint`: the send
    /// right it gets instead, with a badge of its own, stamped with that budget; `None` if the VM
    /// does not serve `endpoint`. The VM's copy is the caller's to close once the child holds its own.
    /// The launches whose budgets have gone leave the endpoint's `launches` here, at the next launch.
    pub(crate) fn admit(
        &mut self,
        endpoint: Handle,
        budget: Handle,
        life: &Arc<Life>,
    ) -> Option<Result<Handle, Refused>> {
        let thread = self.threads.iter_mut().find(|t| t.endpoint == endpoint)?;
        thread.launches.retain(|(_, life)| !life.gone());
        if thread.next >= SELF {
            return Some(Err(Refused("too_many")));
        }
        let Some(badge) = NonZeroU64::new(thread.next) else { return Some(Err(Refused("protocol"))) };
        let budget = Budget::from_handle(budget);
        let minted = match Endpoint::from_handle(endpoint).mint(badge, Some(&budget)) {
            Ok(minted) => minted.handle(),
            Err(e) => return Some(Err(Refused(kernel(e)))),
        };
        thread.next += 1;
        thread.launches.push((badge.get(), Arc::clone(life)));
        Some(Ok(minted))
    }

    /// Whether a wake-up is a serve thread's: it carries nothing, the forwards are in the queue.
    pub(crate) fn woken(&self, delivery: &Delivery) -> bool {
        let i = delivery.caller.badge.wrapping_sub(BADGE) as usize;
        i < self.threads.len() && delivery.words[0] == WAKE
    }

    /// The VM is about to idle (`true`), or has woken (`false`): what a serve thread that queues
    /// meanwhile goes by ([`Forwards`]). Said before the VM's last look at the queue, which is
    /// [`Served::take`] too.
    pub(crate) fn idle(&self, idle: bool) { self.forwards.queue.lock().idle = idle; }

    /// Takes what the serve threads queued, each as a request for its thread's asker, onto
    /// `events`. The handles arrive as resources that die with the request's badge's launch, as
    /// [`Thread::life`] says.
    pub(crate) fn take(&mut self, events: &mut VecDeque<(u64, Event)>) {
        let taken = core::mem::take(&mut self.forwards.queue.lock().items);
        for f in taken {
            let Some(thread) = self.threads.get(f.thread) else { continue };
            let life = thread.life(f.badge);
            let handles = f.handles.into_iter().map(|h| Cap::returned(h, life.clone())).collect();
            let request = f
                .number
                .map(|number| Arc::new(Pending { answer: Arc::clone(&thread.answer), number }) as Object);
            let message = Message { words: f.words, buffer: f.buffer, handles };
            let (badge, account, labels) = (f.badge, f.account, f.labels);
            events.push_back((thread.asker, Event::Request { request, badge, account, labels, message }));
        }
    }
}

/// Hands `forwarded` to the VM: queued, and the VM woken once if it idles. A full queue holds the
/// thread until the VM's poll takes a wake and makes room, as every forward once held it; `false`
/// if the VM is gone.
fn hand(forwards: &Forwards, wake: &Endpoint, forwarded: Forwarded) -> bool {
    let mut forwarded = Some(forwarded);
    let idle = loop {
        let mut q = forwards.queue.lock();
        if q.items.len() < MAX_FORWARDS {
            q.items.extend(forwarded.take());
            break q.idle;
        }
        drop(q);
        if wake.send(&[WAKE, 0, 0, 0], &[], None, FOREVER).is_err() {
            return false;
        }
    };
    if !idle {
        // A running VM finds the queue at its next poll.
        return true;
    }
    // The VM is in its idle receive, or about to enter it: the wake is taken at once, or waits
    // for the receive in steps of WAKE_US while the VM still says it idles. A VM that woke on its
    // own meanwhile says it does not, and takes the queue at its poll.
    loop {
        match wake.send(&[WAKE, 0, 0, 0], &[], None, WAKE_US) {
            Ok(()) => return true,
            Err((redoubt_rt::abi::Error::Timeout, _)) if forwards.queue.lock().idle => {}
            Err((redoubt_rt::abi::Error::Timeout, _)) => return true,
            Err(_) => return false,
        }
    }
}

/// Answers the request `request` with `reply`, whose handles the platform has checked: `carried`.
pub(crate) fn reply(request: &Object, reply: &Message, carried: &[Handle]) -> Result<(), Refused> {
    let pending = request.downcast_ref::<Pending>().ok_or(Refused("wrong_object"))?;
    let mut header = [0u64; 32];
    header[0] = pending.number;
    header[1..5].copy_from_slice(&reply.words);
    let bytes = reply.buffer.as_deref().unwrap_or(&[]);
    if bytes.len() > MAX_BYTES {
        return Err(Refused("too_large"));
    }
    header[10] = bytes.len() as u64;
    let mut pages =
        Buffer::new((HEADER + bytes.len()).div_ceil(PAGE_SIZE)).map_err(|e| Refused(kernel(e)))?;
    put_words(&mut pages, &header);
    pages[HEADER..HEADER + bytes.len()].copy_from_slice(bytes);
    // The reply's handles travel with the answer, copied, so the thread holds its own until it has
    // answered, whatever the caller does with its resources meanwhile.
    pending.answer.send(&[0; 4], carried, Some(pages), FOREVER).map_err(|(e, _)| Refused(kernel(e)))
}

/// The little-endian words at the front of `bytes`.
fn words_of(bytes: &[u8]) -> [u64; 32] {
    core::array::from_fn(|i| {
        bytes.get(i * 8..i * 8 + 8).map_or(0, |w| u64::from_le_bytes(w.try_into().unwrap_or([0; 8])))
    })
}

fn put_words(bytes: &mut [u8], words: &[u64; 32]) {
    for (i, w) in words.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
}

/// Serve thread `i`: takes the endpoint's calls and sends, hands each to the VM through the
/// forwards queue, and answers what the VM answers.
fn serve(endpoint: Endpoint, wake: Endpoint, forwards: &Forwards, i: usize) {
    let Ok(mut admission) = Admission::new(LIMITS) else { return };
    let mut parked: Parked<u64> = Parked::new(REQUEST_WAIT_US);
    let mut next: u64 = 1;
    loop {
        let now = time_now().unwrap_or(0);
        while let Some(Ok((request, _))) = parked.expired(&mut admission, now) {
            refuse(request);
        }
        let timeout = parked.next_deadline().map_or(FOREVER, |d| d.saturating_sub(now).max(1));
        let event = match endpoint.receive(timeout, MAX_LEND_PAGES) {
            Ok(event) => event,
            Err(redoubt_rt::abi::Error::Timeout) => continue,
            Err(_) => return,
        };
        match event {
            redoubt_rt::ipc::Event::Call(mut request) => {
                let key = (AdmitKey::of(&request.caller), request.caller.badge);
                let number = next;
                let Some(forwarded) = forward(&mut request, i, number) else {
                    // Refused on drop: what it carried is closed.
                    drop(request);
                    continue;
                };
                if let Err(not) = parked.park(&mut admission, request, key, number, now) {
                    forwarded.handles.iter().for_each(|h| {
                        let _ = redoubt_rt::handle::close(*h);
                    });
                    refuse(not.0);
                    continue;
                }
                next += 1;
                if !hand(forwards, &wake, forwarded) {
                    return;
                }
            }
            redoubt_rt::ipc::Event::Send(delivery) if delivery.caller.badge == SELF => {
                answer(&mut parked, &mut admission, &delivery);
            }
            redoubt_rt::ipc::Event::Send(delivery) => {
                let Some(forwarded) = forward_send(&delivery, i) else {
                    close_delivery(&delivery);
                    continue;
                };
                if !hand(forwards, &wake, forwarded) {
                    return;
                }
            }
            redoubt_rt::ipc::Event::Abandoned(id) => {
                let _ = parked.abandoned(&mut admission, id, &MALFORMED);
            }
            _ => {}
        }
    }
}

/// Answers `request` `malformed`, closing nothing: its handles are the VM's by now.
fn refuse(mut request: Request) {
    request.handles = redoubt_rt::abi::ReceivedHandles::new();
    let _ = finish(request, &Outcome { words: MALFORMED, send: Handles::new(), close: Handles::new() });
}

/// Call `number` on thread `i`, as the VM takes it; its handles are the VM's from here on. `None`
/// if its bytes do not fit.
fn forward(request: &mut Request, i: usize, number: u64) -> Option<Forwarded> {
    let caller = request.caller.clone();
    let words = request.words;
    let handles: Vec<Handle> = request.handles.as_slice().iter().flatten().copied().collect();
    let lent = request.lend();
    let bytes = (!lent.is_empty()).then(|| &lent[..(words[1] as usize).min(lent.len())]);
    let forwarded = pack(&caller, i, Some(number), &words, handles, bytes)?;
    request.handles = redoubt_rt::abi::ReceivedHandles::new();
    Some(forwarded)
}

/// A one-way send on thread `i`, as the VM takes it.
fn forward_send(delivery: &Delivery, i: usize) -> Option<Forwarded> {
    let handles: Vec<Handle> = delivery.handles.as_slice().iter().flatten().copied().collect();
    let bytes = delivery.transfer.as_deref().map(|t| &t[..(delivery.words[1] as usize).min(t.len())]);
    pack(&delivery.caller, i, None, &delivery.words, handles, bytes)
}

/// A request's parts and a copy of its bytes, for the queue.
fn pack(
    caller: &redoubt_rt::ipc::Caller,
    thread: usize,
    number: Option<u64>,
    words: &[u64; 4],
    handles: Vec<Handle>,
    bytes: Option<&[u8]>,
) -> Option<Forwarded> {
    if bytes.is_some_and(|b| b.len() > MAX_BYTES) {
        return None;
    }
    Some(Forwarded {
        thread,
        number,
        badge: caller.badge,
        account: caller.account,
        labels: caller.labels.as_slice().to_vec(),
        words: *words,
        handles,
        // A copy the heap refuses is a refused forward, as the lend's own buffer was: the
        // allocator returns null past the cap and the VM has no handler for a failed allocation.
        buffer: match bytes {
            Some(b) => {
                let mut copy = Vec::new();
                copy.try_reserve_exact(b.len()).ok()?;
                copy.extend_from_slice(b);
                Some(copy)
            }
            None => None,
        },
    })
}

/// The VM's answer in `delivery`: the parked call it names, answered with its words, the handles
/// the answer carried (closed here once sent) and its bytes; dropped, with what it carried, if
/// that call has ended.
fn answer(parked: &mut Parked<u64>, admission: &mut Admission, delivery: &Delivery) {
    let mut send = Handles::new();
    for handle in delivery.handles.as_slice().iter().flatten() {
        let _ = send.push(*handle);
    }
    let resumed = delivery.transfer.as_deref().and_then(|bytes| {
        let number = words_of(bytes)[0];
        parked.resume_first(admission, |n| *n == number)?.ok()
    });
    let (Some(bytes), Some((mut request, _))) = (delivery.transfer.as_deref(), resumed) else {
        close_delivery(delivery);
        return;
    };
    let header = words_of(bytes);
    let words = [header[1], header[2], header[3], header[4]];
    let n = (header[10] as usize).min(MAX_BYTES).min(bytes.len().saturating_sub(HEADER));
    let lend = request.lend();
    if n > lend.len() {
        close_delivery(delivery);
        refuse(request);
        return;
    }
    lend[..n].copy_from_slice(&bytes[HEADER..HEADER + n]);
    let _ = finish(request, &Outcome { words, send, close: send });
}
