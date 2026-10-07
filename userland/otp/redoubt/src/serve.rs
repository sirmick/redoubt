//! Endpoints the VM serves (docs/userland/beamlet.md, "Natives": `serve/1`, `reply/2`): a thread per
//! served endpoint takes its calls and hands each to the VM as a request, on the serving library's
//! admission and parked calls.
//!
//! - **Admission and the deadline are the library's** (servers/serving.md R26, R28): each call is parked
//!   under its caller's bucket and share, at most [`REQUEST_WAIT_US`]; one past it is answered by the thread
//!   with the one status every protocol has, `malformed` (code 1), and its admission given back. So a request
//!   the Erlang side never answers is ended by the thread, never held by the VM, and a caller whose bucket or
//!   share is full is answered at once, `malformed` too.
//! - **To the VM through a wake-up**: the call's words, badge, account, labels and handles (by number: the
//!   thread is of the same process, and the handles are the VM's from then on) in a header, and its lent
//!   bytes after it, as the transfer of one `send` to the VM's endpoint through a badge minted for the thread
//!   ([`BADGE`] + its index). A `send` to the endpoint goes the same way, with nothing to answer.
//! - **`reply/2` goes back on the served endpoint itself**, as a `send` through a badge the VM minted from
//!   its receive right ([`SELF`]): only the VM holds it, so the thread takes a send with that badge as an
//!   answer and any other as a request. The answer names the call by the number the thread gave it; an answer
//!   to a call that has ended already (expired, or abandoned by its caller) is dropped.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use beamlet_vm::platform::{Event, Message, Object, Refused};
use redoubt_rt::abi::{FOREVER, Handle, Handles, MAX_LABELS, MAX_LEND_PAGES, MAX_MSG_HANDLES, PAGE_SIZE};
use redoubt_rt::handle::{Endpoint, time_now};
use redoubt_rt::ipc::{Buffer, Delivery, Request};
use redoubt_rt::server::parked::Parked;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::server::{Admission, AdmitKey, Limits, MALFORMED, close_delivery};

use crate::system::{Cap, kernel};

/// The most endpoints one VM serves.
pub const MAX_SERVED: usize = 2;

/// The badge of serve thread `i`'s wake-ups is `BADGE + i`: above every call thread's.
pub const BADGE: u64 = 0x200;

/// The badge the VM answers with, minted from the served endpoint's receive right: the largest a
/// mint gives, which no client's badge is.
pub const SELF: u64 = u32::MAX as u64;

/// The longest a request waits for its answer (µs): past it, the thread answers it.
pub const REQUEST_WAIT_US: u64 = 5_000_000;

/// The serve thread's stack, in pages.
const STACK_PAGES: usize = 4;

/// The bytes of a request's or an answer's header, before its lent bytes.
const HEADER: usize = 256;

/// The longest lent part a request or an answer carries: what fits the transfer after the header.
pub const MAX_BYTES: usize = MAX_LEND_PAGES * PAGE_SIZE - HEADER;

/// A wake-up's first word: a call, or a one-way send.
const CALL: u64 = 0xca11;
const SEND: u64 = 0x5e4d;

/// The admission of a served endpoint: a few callers, a few requests each.
const LIMITS: Limits = Limits { buckets: 8, in_flight: 4, files: 0, state: 0, requests: 0, pages: 0 };

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
}

/// The endpoints served.
#[derive(Default)]
pub(crate) struct Served {
    threads: Vec<Thread>,
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
        let i = self.threads.len() as u64;
        let woken = wake.mint(NonZeroU64::new(BADGE + i).ok_or(Refused("protocol"))?, None).map_err(name)?;
        let body = alloc::boxed::Box::new(move || serve(receive, woken));
        redoubt_rt::thread::spawn(body, STACK_PAGES).map_err(name)?;
        self.threads.push(Thread { asker, answer: Arc::new(answer), endpoint, _held: held });
        Ok(())
    }

    /// Takes a serve thread's wake-up as a request for its asker; anything else is handed back.
    pub(crate) fn deliver(&mut self, delivery: Delivery) -> Result<(u64, Event), Delivery> {
        let i = delivery.caller.badge.wrapping_sub(BADGE) as usize;
        let kind = delivery.words[0];
        let (Some(Thread { asker, answer, .. }), true) = (self.threads.get(i), kind == CALL || kind == SEND)
        else {
            return Err(delivery);
        };
        let Some(bytes) = delivery.transfer.as_deref() else { return Err(delivery) };
        let header = words_of(bytes);
        let labels = header[1..1 + (header[0] as usize).min(MAX_LABELS)].to_vec();
        let words = [header[17], header[18], header[19], header[20]];
        let handles = (0..(header[21] as usize).min(MAX_MSG_HANDLES))
            .filter_map(|h| Handle::new(header[22 + h] as u32))
            .map(Cap::received)
            .collect();
        let buffer = match header[26] {
            u64::MAX => None,
            n => Some(bytes[HEADER..HEADER + (n as usize).min(MAX_BYTES)].to_vec()),
        };
        let request = (kind == CALL)
            .then(|| Arc::new(Pending { answer: Arc::clone(answer), number: delivery.words[1] }) as Object);
        let (badge, account) = (delivery.words[2], delivery.words[3]);
        let message = Message { words, buffer, handles };
        Ok((*asker, Event::Request { request, badge, account, labels, message }))
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

/// A serve thread: takes the endpoint's calls and sends, hands each to the VM, and answers what the
/// VM answers.
fn serve(endpoint: Endpoint, wake: Endpoint) {
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
                let carried: Vec<Handle> = request.handles.as_slice().iter().flatten().copied().collect();
                let Some(pages) = forward(&mut request, &carried) else {
                    // Refused on drop: what it carried is closed.
                    drop(request);
                    continue;
                };
                let caller = request.caller.clone();
                if let Err(not) = parked.park(&mut admission, request, key, number, now) {
                    carried.iter().for_each(|h| {
                        let _ = redoubt_rt::handle::close(*h);
                    });
                    refuse(not.0);
                    continue;
                }
                next += 1;
                let woke =
                    wake.send(&[CALL, number, caller.badge, caller.account], &[], Some(pages), FOREVER);
                if woke.is_err() {
                    return;
                }
            }
            redoubt_rt::ipc::Event::Send(delivery) if delivery.caller.badge == SELF => {
                answer(&mut parked, &mut admission, &delivery);
            }
            redoubt_rt::ipc::Event::Send(delivery) => {
                let Some(pages) = forward_send(&delivery) else {
                    close_delivery(&delivery);
                    continue;
                };
                let (badge, account) = (delivery.caller.badge, delivery.caller.account);
                if wake.send(&[SEND, 0, badge, account], &[], Some(pages), FOREVER).is_err() {
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

/// The header and lent bytes of `request`, for the VM; its handles are the VM's from here on.
/// `None` if they do not fit, or there is no memory for them.
fn forward(request: &mut Request, carried: &[Handle]) -> Option<Buffer> {
    let caller = request.caller.clone();
    let words = request.words;
    let lent = request.lend();
    let bytes = (!lent.is_empty()).then(|| &lent[..(words[1] as usize).min(lent.len())]);
    let pages = pack(&caller.labels, &words, carried, bytes)?;
    request.handles = redoubt_rt::abi::ReceivedHandles::new();
    Some(pages)
}

fn forward_send(delivery: &Delivery) -> Option<Buffer> {
    let carried: Vec<Handle> = delivery.handles.as_slice().iter().flatten().copied().collect();
    let bytes = delivery.transfer.as_deref().map(|t| &t[..(delivery.words[1] as usize).min(t.len())]);
    pack(&delivery.caller.labels, &delivery.words, &carried, bytes)
}

/// A request's header and bytes in fresh pages.
fn pack(
    labels: &redoubt_rt::abi::Labels,
    words: &[u64; 4],
    handles: &[Handle],
    bytes: Option<&[u8]>,
) -> Option<Buffer> {
    let len = bytes.map_or(0, <[u8]>::len);
    if len > MAX_BYTES {
        return None;
    }
    let mut header = [0u64; 32];
    let labels = labels.as_slice();
    header[0] = labels.len() as u64;
    header[1..1 + labels.len()].copy_from_slice(labels);
    header[17..21].copy_from_slice(words);
    header[21] = handles.len() as u64;
    for (i, h) in handles.iter().enumerate() {
        header[22 + i] = u64::from(h.index());
    }
    header[26] = if bytes.is_some() { len as u64 } else { u64::MAX };
    let mut pages = Buffer::new((HEADER + len).div_ceil(PAGE_SIZE)).ok()?;
    put_words(&mut pages, &header);
    if let Some(bytes) = bytes {
        pages[HEADER..HEADER + len].copy_from_slice(bytes);
    }
    Some(pages)
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
