//! Multiplexed connections (servers/serving.md, "Multiplexed connections"; R77): many 9P
//! requests outstanding on one connection, answered through one long-poll call.
//!
//! - **A request** is a `send` on the connection's badge, word 0 = 0: one T-message packed in words 1 to 3
//!   (little-endian, a machine word's bytes each: [`IN_WORDS`]), or one or more end to end at the start of a
//!   transfer, word 1 their length, so many small requests cost one page. Tags are 0 to [`MAX_TAGS`] - 1; a
//!   tag in use or out of range, a message that does not frame, or a `Tversion` ends the session. Anything
//!   sent before the session opened is dropped.
//! - **The completion call** is a 9P `call` with words [`collect_words`] and a lend: word 2 is how long the
//!   server may hold it (µs). The first opens the connection's **session** and is answered at once, empty,
//!   with [`OPENED`] in word 2 (or [`REFUSED`]): until then the server holds nothing for the connection.
//!   Later ones are parked, one at a time (a second is malformed), and answered with R-messages end to end at
//!   the front of the lend, their length in word 1. A parked one with nothing to answer is answered empty
//!   when its hold runs out, or at the **session bound** if that is shorter: the server's longest wait
//!   ([`NineServer::requests_wait`]) or [`COLLECT_WAIT`], whichever is shorter. A hold of 0 answers at once.
//! - **Served only into a parked completion call's lend.** A request is answered when a completion call is
//!   parked, straight into its lend: nothing is copied out or held in the server but the request, which holds
//!   its admission until then. A read is served only where its whole count fits what is left of the lend (the
//!   first one always is), so packing never shortens a read; the rest wait for the next call, in order. A
//!   request the file server asks to hold ([`super::Read::Wait`]) stays pending and is served again at the
//!   next call, wake-up ([`NineServer::wake`]) or request; at its deadline ([`NineServer::requests_wait`]) it
//!   is answered [`NineError::TIMEOUT`].
//! - **Admission** (servers/serving.md R26, R28, R77). The session holds one `InFlight` of the connection's
//!   bucket and share: its completion call's, the one open call. Each request holds one `Requests`, and the
//!   pages a send brought one `Pages` each, counted once for the send and given back with the last of its
//!   requests too long for the words (one that fits them is copied out of the page as it is taken, so a
//!   request that waits pins no page), until the answer is delivered; each resource has its own share. Over
//!   either share a request is not served: its tag goes into the session's refused set, a bitmap, answered
//!   `Rerror` [`NineError::BUSY`] first in the next completion call; pages the share cannot pay for refuse
//!   every request they came with, and go at once.
//! - **Flush.** `Tflush(oldtag)` drops oldtag's request (or its pending `busy`) when it arrives, and is
//!   answered `Rflush` in turn: an answer delivered before it is the only one oldtag gets (intro(5), flush).
//! - **The end.** The completion call's abandonment (kernel/ipc.md R3: its caller died or gave up), a reply
//!   to it that was discarded, a protocol error, the connection's `disconnect`, or no completion call parked
//!   for the session bound since the last one returned, ends the session: every request goes, its admission
//!   with it, and a parked call is answered [`ENDED`]. So a client that dies between two completion calls is
//!   found at the session bound.
//! - **Crash blame** (kernel/processes.md R21): requests are served with the completion call as the thread's
//!   current call, so a crash while serving them blames their client.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_sys::{Error, FOREVER, Handles, MAX_LEND_PAGES};
use redoubt_wire::ninep::{Body, IOHDRSZ, Message, NOTAG, message_size};
use redoubt_wire::typed::error_reply;

use super::super::admit::{AdmitKey, Resource};
use super::super::typed::{Outcome, finish};
use super::{
    Answer, ConnKey, FileServer, Held, NineError, NineServer, refuse_malformed, tag_of, write_reply,
};
use crate::exit;
use crate::handle::Endpoint;
use crate::ipc::{Buffer, Caller, Delivery, Event, Request, Words};

/// Requests outstanding on one connection at most: tags 0 to 255.
pub const MAX_TAGS: usize = 256;
/// Word 1 of a completion call.
pub const COLLECT: u64 = 1;

/// The words of a completion call the server may hold for `hold_us` µs (0: answer at once).
pub const fn collect_words(hold_us: u64) -> Words { [0, COLLECT, hold_us, 0] }
/// Word 2 of the reply that opens a session: nothing sent on the connection before it is held.
pub const OPENED: u64 = 1;
/// Status of a completion call that could not open a session: its share or the server is full.
pub const REFUSED: u32 = 3;
/// Status of a parked completion call whose session has ended: its requests are gone.
pub const ENDED: u32 = 4;
/// The longest session bound (µs): a parked completion call is held at most this long, and a
/// session lasts at most this long after its last completion call returned with none parked.
pub const COLLECT_WAIT: u64 = 10_000_000;
/// The bytes of the server's record of one multiplexed request (its place in the queue: about
/// 216 on rv64), rounded up: what one `Requests` costs a server.
pub const REQUEST_STATE: u64 = 256;
const _: () = assert!(core::mem::size_of::<Pending>() as u64 <= REQUEST_STATE);
/// The bytes of a T-message the words of a request carry: three machine words.
pub const IN_WORDS: usize = 3 * WORD;

const WORD: usize = core::mem::size_of::<usize>();
/// What a request holds while outstanding.
const REQUEST: Resource = Resource::Requests;
/// The room an answer other than a read's is given before it is packed: an `Rstat`, an `Rwalk`
/// of 16 qids and every `Rerror` fit.
const SMALL_REPLY: usize = 1024;

/// What a server keeps beside its files, around [`NineServer::run_around`]'s receive.
pub trait Around<S: FileServer> {
    /// A call.
    fn call(&mut self, server: &mut NineServer<S>, request: Request, now: u64);
    /// Before each receive, after what the deadlines made due: whatever moved since.
    fn turn(&mut self, _server: &mut NineServer<S>, _now: u64) {}
    /// An abandoned-call notice for a call that is no completion call: one parked here.
    fn abandoned(&mut self, _server: &mut NineServer<S>, _id: NonZeroU64) {}
    /// A send that is no multiplexed request: a protocol's one-way message, which the server
    /// decodes itself (`consol`'s `ended`), or nothing it knows, dropped.
    fn send(&mut self, _server: &mut NineServer<S>, delivery: Delivery, _now: u64) {
        super::super::close_delivery(&delivery);
    }
}

/// [`NineServer::run`]'s: calls served, the server's own typed opcodes by the closure.
struct Own<G>(G);

impl<S: FileServer, G: FnMut(&mut NineServer<S>, Request) -> Result<(), Error>> Around<S> for Own<G> {
    fn call(&mut self, server: &mut NineServer<S>, request: Request, _: u64) {
        // A server served through `run` has no parked calls, so its own requests never wait.
        let own = &mut self.0;
        let _ = server.serve_with(request, |server, request| own(server, request).map(|()| None));
    }
}

type Tags = [u64; MAX_TAGS / 64];

fn has(tags: &Tags, tag: u16) -> bool { tags[usize::from(tag) / 64] & 1 << (tag % 64) != 0 }

fn set(tags: &mut Tags, tag: u16, on: bool) {
    let bit = 1 << (tag % 64);
    let word = &mut tags[usize::from(tag) / 64];
    *word = if on { *word | bit } else { *word & !bit };
}

/// Where a request's T-message is: in the record itself, when it fits what the words carry on
/// the widest machine (from the words, or copied out of its page), or at an offset in
/// transferred pages now ours, shared by the requests of theirs too long for that and freed with
/// the last of them.
enum Stored {
    Words([u8; WORDS_BYTES]),
    Pages(Arc<Buffer>, usize),
}

/// The most a T-message in the words can be: three 64-bit words. A request's record keeps a
/// message up to this long, whichever width it came on and however it came.
const WORDS_BYTES: usize = 3 * 8;

/// A request not yet answered, or answered and not yet delivered: they are the same thing here.
struct Pending {
    key: ConnKey,
    tag: u16,
    /// When it is answered `TIMEOUT` if it is still waiting; `FOREVER` for none.
    deadline: u64,
    message: Stored,
    len: usize,
}

impl Pending {
    /// The T-message.
    fn bytes(&self) -> &[u8] {
        match &self.message {
            Stored::Words(bytes) => &bytes[..self.len],
            Stored::Pages(pages, at) => &pages[*at..*at + self.len],
        }
    }

    /// The room its answer is given: a read's whole count, so packing never shortens it.
    fn need(&self) -> usize {
        match Message::decode(self.bytes()) {
            Ok(Message { body: Body::Tread { count, .. }, .. }) => IOHDRSZ.saturating_add(count as usize),
            _ => SMALL_REPLY,
        }
    }
}

/// One connection's session.
pub(super) struct Session {
    key: ConnKey,
    caller: Caller,
    /// The bucket and share everything of the session is charged to.
    charge: (AdmitKey, u64),
    /// The parked completion call.
    call: Option<Request>,
    /// With a call parked, when it is answered empty; without, when the session ends.
    deadline: u64,
    /// Tags outstanding: pending, or refused and not yet answered.
    tags: Tags,
    refused: Tags,
}

/// Every session of the server, and their requests in the order they came.
pub(super) struct Mux {
    sessions: Vec<Session>,
    pending: Vec<Pending>,
    /// The longest a request waits ([`NineServer::requests_wait`]).
    wait: u64,
}

impl Mux {
    pub(super) fn new() -> Mux { Mux { sessions: Vec::new(), pending: Vec::new(), wait: COLLECT_WAIT } }

    fn session(&self, key: &ConnKey) -> Option<usize> { self.sessions.iter().position(|s| s.key == *key) }

    /// The session bound: how long a completion call is held at most, and how long a session lasts
    /// with none parked. The server's longest wait or [`COLLECT_WAIT`], whichever is shorter.
    fn bound(&self) -> u64 { self.wait.min(COLLECT_WAIT) }

    /// Ends every session on `badge` (a minted connection disconnected).
    pub(super) fn end_badge(&mut self, admission: &mut super::Admission, badge: u64) {
        while let Some(s) = self.sessions.iter().position(|s| s.key.badge == badge) {
            let session = self.sessions.swap_remove(s);
            end(&mut self.pending, admission, session);
        }
    }
}

/// Drops `session` and its requests, releasing what they held, and answers its parked call.
fn end(pending: &mut Vec<Pending>, admission: &mut super::Admission, session: Session) {
    let mut i = 0;
    while i < pending.len() {
        if pending[i].key == session.key {
            release(admission, session.charge, pending.remove(i));
        } else {
            i += 1;
        }
    }
    let (key, share) = session.charge;
    admission.release(key, share, Resource::InFlight);
    if let Some(call) = session.call {
        let _ = finish(call, &plain(error_reply(ENDED)));
    }
}

/// Gives back what `request` held: its `Requests`, and, if it is the last request holding the
/// pages its send brought, those `Pages`.
fn release(admission: &mut super::Admission, charge: (AdmitKey, u64), request: Pending) {
    admission.release(charge.0, charge.1, REQUEST);
    if let Stored::Pages(pages, _) = request.message {
        if Arc::strong_count(&pages) == 1 {
            release_pages(admission, charge, pages.npages());
        }
    }
}

/// Takes `n` [`Resource::Pages`] for `share` in `bucket`, all or none, as `ipd` takes sockets.
fn admit_pages(admission: &mut super::Admission, (bucket, share): (AdmitKey, u64), n: usize) -> bool {
    for taken in 0..n {
        if admission.admit(bucket, share, Resource::Pages).is_err() {
            release_pages(admission, (bucket, share), taken);
            return false;
        }
    }
    true
}

fn release_pages(admission: &mut super::Admission, (bucket, share): (AdmitKey, u64), n: usize) {
    for _ in 0..n {
        admission.release(bucket, share, Resource::Pages);
    }
}

fn plain(words: Words) -> Outcome { Outcome { words, send: Handles::new(), close: Handles::new() } }

/// A send's T-messages: one in its words 1 to 3, or one or more end to end at the start of its
/// transfer, word 1 their length. `None` if any does not frame.
fn stored(delivery: Delivery) -> Option<Vec<(Stored, usize)>> {
    let mut messages = Vec::new();
    match delivery.transfer {
        Some(pages) => {
            let end = usize::try_from(delivery.words[1]).ok().filter(|n| *n <= pages.len())?;
            let pages = Arc::new(pages);
            let mut at = 0;
            while at < end {
                let len = message_size(&pages[at..end]).ok()?;
                messages.try_reserve(1).ok()?;
                // One that fits the words is kept as if it had come in them, so the page is held
                // only by requests too long for them: a read that waits pins no page on either
                // width (on rv32 the words carry 12 bytes, and every read comes in a page).
                let message = if len <= WORDS_BYTES {
                    let mut bytes = [0; WORDS_BYTES];
                    bytes[..len].copy_from_slice(&pages[at..at + len]);
                    Stored::Words(bytes)
                } else {
                    Stored::Pages(pages.clone(), at)
                };
                messages.push((message, len));
                at += len;
            }
        }
        None => {
            let mut bytes = [0; WORDS_BYTES];
            for (chunk, word) in bytes.chunks_mut(WORD).zip(&delivery.words[1..]) {
                chunk.copy_from_slice(&usize::try_from(*word).ok()?.to_le_bytes());
            }
            let len = message_size(&bytes[..IN_WORDS]).ok()?;
            messages.try_reserve(1).ok()?;
            messages.push((Stored::Words(bytes), len));
        }
    }
    Some(messages)
}

/// The length of the message just written at the front of `out`.
fn written(out: &[u8]) -> usize {
    out.get(..4).map_or(0, |n| u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)
}

impl<S: FileServer> NineServer<S> {
    /// Multiplexed requests wait at most `longest` µs for the file server ([`FOREVER`]: no deadline,
    /// for a server whose reads wait on a person). [`COLLECT_WAIT`] unless set.
    pub fn requests_wait(&mut self, longest: u64) { self.mux.wait = longest; }

    /// Sessions open now.
    pub fn sessions(&self) -> usize { self.mux.sessions.len() }

    /// A completion call ([`collect_words`]): opens the session, or is parked and served.
    pub(super) fn collect(&mut self, mut request: Request) -> Result<(), Error> {
        let now = crate::handle::time_now().unwrap_or(0);
        if !request.handles.as_slice().is_empty() || request.lend().is_empty() {
            return refuse_malformed(request);
        }
        let key = self.conn_key(&request.caller);
        let Some(s) = self.mux.session(&key) else {
            if !self.open_session(&request.caller, now) {
                return finish(request, &plain(error_reply(REFUSED))).map(|_| ());
            }
            let sent = finish(request, &plain([0, 0, OPENED, 0]));
            if !sent.as_ref().is_ok_and(|o| o.delivered) {
                self.end_session(self.mux.sessions.len() - 1);
            }
            return sent.map(|_| ());
        };
        if self.mux.sessions[s].call.is_some() {
            return refuse_malformed(request);
        }
        let hold = request.words[2];
        self.mux.sessions[s].call = Some(request);
        self.mux.sessions[s].deadline = now.saturating_add(hold.min(self.mux.bound()));
        self.pump(s, now, hold == 0);
        Ok(())
    }

    /// Opens `caller`'s session, as its first completion call does, without a system call: one
    /// `InFlight` of its share. `false` if it is refused, or one is open already.
    pub fn open_session(&mut self, caller: &Caller, now: u64) -> bool {
        let key = self.conn_key(caller);
        let (bucket, share) = self.charge_of(caller);
        if self.mux.session(&key).is_some()
            || self.mux.sessions.try_reserve(1).is_err()
            || self.admission.admit(bucket, share, Resource::InFlight).is_err()
        {
            return false;
        }
        let deadline = now.saturating_add(self.mux.bound());
        let charge = (bucket, share);
        let (caller, (tags, refused)) = (*caller, ([0; 4], [0; 4]));
        self.mux.sessions.push(Session { key, caller, charge, call: None, deadline, tags, refused });
        true
    }

    /// A message that came by `send`: a multiplexed request if its word 0 is 0, taken here, and
    /// what it brought closed; anything else is handed back for the server's own use.
    pub fn deliver(&mut self, delivery: Delivery, now: u64) -> Option<Delivery> {
        if delivery.words[0] != 0 {
            return Some(delivery);
        }
        super::super::close_delivery(&delivery);
        let key = self.conn_key(&delivery.caller);
        // Nothing is held for a connection without a session: the request is dropped.
        let s = self.mux.session(&key)?;
        self.take_all(s, stored(delivery), now);
        None
    }

    /// [`NineServer::deliver`] for a request in `words` alone, without a system call while no
    /// completion call is parked: what the fuzz target drives.
    pub fn take_request(&mut self, caller: &Caller, words: &Words, now: u64) {
        let key = self.conn_key(caller);
        let Some(s) = self.mux.session(&key) else { return };
        let delivery = Delivery {
            caller: *caller,
            words: *words,
            handles: redoubt_sys::ReceivedHandles::new(),
            transfer: None,
        };
        self.take_all(s, stored(delivery), now);
    }

    /// What a parked completion call of `caller`'s would be given now, packed into `lend`, without
    /// a system call: how many bytes. 0 with no session, or one whose call is parked.
    pub fn collect_into(&mut self, caller: &Caller, lend: &mut [u8], now: u64) -> usize {
        let key = self.conn_key(caller);
        match self.mux.session(&key) {
            Some(s) if self.mux.sessions[s].call.is_none() => self.fill(s, lend, now),
            _ => 0,
        }
    }

    /// Session `s`'s requests from one send, then what can be answered; `None` (one did not
    /// frame) ends the session.
    fn take_all(&mut self, s: usize, messages: Option<Vec<(Stored, usize)>>, now: u64) {
        let Some(messages) = messages else { return self.end_session(s) };
        let charge = self.mux.sessions[s].charge;
        // The pages a send brought count once, before its requests, if any request still lives
        // in them: refused, every request in it is answered busy, and the pages go now.
        let batch = messages.iter().find_map(|(message, _)| match message {
            Stored::Pages(pages, _) => Some(pages.clone()),
            Stored::Words(_) => None,
        });
        let paid =
            batch.as_ref().is_none_or(|pages| admit_pages(&mut self.admission, charge, pages.npages()));
        let mut alive = true;
        for (message, len) in messages {
            if alive {
                alive = self.take(s, message, len, paid, now);
            }
        }
        // No request of the send holds its pages any more (all refused, or the session ended):
        // they go back here.
        if let Some(pages) = batch.filter(|pages| paid && Arc::strong_count(pages) == 1) {
            release_pages(&mut self.admission, charge, pages.npages());
        }
        if alive {
            self.pump(s, now, false);
        }
    }

    /// Session `s`'s request `message`: its tag checked, admitted or refused (always, if the
    /// pages it came in were not `paid` for), a flush acted on. `false` if it ended the session.
    fn take(&mut self, s: usize, message: Stored, len: usize, paid: bool, now: u64) -> bool {
        let key = self.mux.sessions[s].key;
        let entry = Pending { key, tag: NOTAG, deadline: now.saturating_add(self.mux.wait), message, len };
        // The frame holds a header (`message_size`), so the tag is there; a body that does not
        // decode is answered `Rerror` in its turn, as a call's would be.
        let tag = tag_of(entry.bytes()).unwrap_or(NOTAG);
        let (version, flushes) = match Message::decode(entry.bytes()) {
            Ok(Message { body: Body::Tversion { .. }, .. }) => (true, None),
            Ok(Message { body: Body::Tflush { oldtag }, .. }) => (false, Some(oldtag)),
            _ => (false, None),
        };
        let session = &mut self.mux.sessions[s];
        if usize::from(tag) >= MAX_TAGS || has(&session.tags, tag) || version {
            self.end_session(s);
            return false;
        }
        set(&mut session.tags, tag, true);
        let (bucket, share) = session.charge;
        if !paid
            || self.mux.pending.try_reserve(1).is_err()
            || self.admission.admit(bucket, share, REQUEST).is_err()
        {
            set(&mut self.mux.sessions[s].refused, tag, true);
        } else {
            if let Some(old) = flushes.filter(|old| *old != tag) {
                self.flush(s, old);
            }
            self.mux.pending.push(Pending { tag, ..entry });
        }
        true
    }

    /// `Tflush(old)` arrived: `old`'s request, or its pending `busy`, is dropped unanswered.
    fn flush(&mut self, s: usize, old: u16) {
        let session = &mut self.mux.sessions[s];
        if usize::from(old) >= MAX_TAGS || !has(&session.tags, old) {
            return;
        }
        set(&mut session.tags, old, false);
        if has(&session.refused, old) {
            set(&mut session.refused, old, false);
            return;
        }
        let (key, charge) = (session.key, session.charge);
        if let Some(i) = self.mux.pending.iter().position(|p| p.key == key && p.tag == old) {
            release(&mut self.admission, charge, self.mux.pending.remove(i));
        }
    }

    /// Receives on `endpoint` until it dies, serving calls (`own` takes the server's own typed
    /// opcodes, as in [`NineServer::serve_with`]), multiplexed requests and their deadlines: the
    /// loop of a 9P server that keeps nothing beside its files. Transfers of up to
    /// `MAX_LEND_PAGES` are taken, for requests too long for their words. The endpoint's death
    /// ends it with [`exit::OK`], any other failure with [`exit::RECEIVE_FAILED`].
    pub fn run(
        &mut self,
        endpoint: &Endpoint,
        own: impl FnMut(&mut Self, Request) -> Result<(), Error>,
    ) -> u32 {
        self.run_around(endpoint, Own(own))
    }

    /// [`NineServer::run`] for a server that keeps state beside its files: `around` is given its
    /// calls, the abandoned-call notices that are no completion call's, and a turn before each
    /// receive.
    pub fn run_around(&mut self, endpoint: &Endpoint, mut around: impl Around<S>) -> u32 {
        loop {
            let now = crate::handle::time_now().unwrap_or(0);
            self.expire(now);
            around.turn(self, now);
            let timeout = self.next_deadline().map_or(FOREVER, |d| d.saturating_sub(now).max(1));
            let received = endpoint.receive(timeout, MAX_LEND_PAGES);
            // The receive may have waited out a whole hold: what came is served at the time it
            // came, so a deadline it sets runs from then.
            let now = crate::handle::time_now().unwrap_or(0);
            match received {
                Ok(Event::Call(request)) => around.call(self, request, now),
                Ok(Event::Send(delivery)) => {
                    if let Some(other) = self.deliver(delivery, now) {
                        around.send(self, other, now);
                    }
                }
                Ok(Event::Abandoned(id)) => {
                    if !self.abandoned(id) {
                        around.abandoned(self, id);
                    }
                }
                Ok(Event::Interrupt | Event::Exit(_)) | Err(Error::Timeout) => {}
                Err(Error::Dead) => return exit::OK,
                Err(_) => return exit::RECEIVE_FAILED,
            }
        }
    }

    /// Answers an abandoned-call notice for a session's completion call: the session ends.
    /// `false` if `id` is no completion call of this server.
    pub fn abandoned(&mut self, id: NonZeroU64) -> bool {
        let found = self.mux.sessions.iter().position(|s| s.call.as_ref().is_some_and(|c| c.id() == id));
        found.inspect(|s| self.end_session(*s)).is_some()
    }

    /// What `now` makes due: requests past their deadline answered, parked completion calls past
    /// theirs answered empty, sessions without one for as long ended.
    pub fn expire(&mut self, now: u64) {
        let mut s = 0;
        while s < self.mux.sessions.len() {
            let session = &self.mux.sessions[s];
            let due = session.deadline <= now;
            let alive = if session.call.is_none() {
                // Nothing has collected for the session bound: the client is gone.
                if due {
                    self.end_session(s);
                }
                !due
            } else {
                let late = self.mux.pending.iter().any(|p| p.key == session.key && p.deadline <= now);
                !(late || due) || self.pump(s, now, due)
            };
            s += usize::from(alive);
        }
    }

    /// The next time [`NineServer::expire`] has something to do.
    pub fn next_deadline(&self) -> Option<u64> {
        let sessions = self.mux.sessions.iter().map(|s| s.deadline);
        // A request's deadline matters only while a completion call can take its timeout.
        let parked =
            |p: &&Pending| self.mux.session(&p.key).is_some_and(|s| self.mux.sessions[s].call.is_some());
        let requests = self.mux.pending.iter().filter(parked).map(|p| p.deadline);
        sessions.chain(requests).filter(|d| *d != FOREVER).min()
    }

    /// Serves again every request that waits, where a completion call is parked to take its
    /// answer: for a server whose file server's state moved (input came, a socket filled).
    pub fn wake(&mut self, now: u64) {
        let mut s = 0;
        while s < self.mux.sessions.len() {
            s += usize::from(self.pump(s, now, false));
        }
    }

    /// Serves session `s`'s refused tags and requests into its parked completion call, and
    /// replies if anything was written, or if `empty` (its deadline). Returns whether the session
    /// is still at `s`.
    fn pump(&mut self, s: usize, now: u64, empty: bool) -> bool {
        let Some(mut call) = self.mux.sessions[s].call.take() else { return true };
        // A crash while serving these blames their client.
        let _ = call.serve();
        let pos = self.fill(s, call.lend(), now);
        if pos == 0 && !empty {
            self.mux.sessions[s].call = Some(call);
            return true;
        }
        let sent = finish(call, &plain([0, pos as u64, 0, 0]));
        if !sent.is_ok_and(|o| o.delivered) {
            // Its answers reached nobody: the client is gone, or gave up on the call.
            self.end_session(s);
            return false;
        }
        self.mux.sessions[s].deadline = now.saturating_add(self.mux.bound());
        true
    }

    /// Packs session `s`'s `busy` answers, then its requests in order, into `lend`; returns how
    /// many bytes. Each request answered is gone, its admission released.
    fn fill(&mut self, s: usize, lend: &mut [u8], now: u64) -> usize {
        let session = &self.mux.sessions[s];
        let (key, caller, (bucket, share)) = (session.key, session.caller, session.charge);
        let mut pos = 0;
        for tag in 0..MAX_TAGS as u16 {
            if !has(&self.mux.sessions[s].refused, tag) {
                continue;
            }
            let busy = Message { tag, body: Body::Rerror { ename: NineError::BUSY.0 } };
            let Ok(n) = busy.encode(&mut lend[pos..]) else { break };
            pos += n;
            set(&mut self.mux.sessions[s].refused, tag, false);
            set(&mut self.mux.sessions[s].tags, tag, false);
        }
        let mut i = 0;
        while i < self.mux.pending.len() {
            if self.mux.pending[i].key != key {
                i += 1;
                continue;
            }
            if lend.len() - pos < self.mux.pending[i].need().min(lend.len()) {
                break;
            }
            let entry = self.mux.pending.remove(i);
            let out = &mut lend[pos..];
            let answer = if entry.deadline <= now {
                write_reply(entry.tag, Err(Held::Error(NineError::TIMEOUT)), out)
            } else {
                self.fs.serving(&caller, (bucket, share), &mut self.admission);
                let answer = self.answer_into(&caller, entry.bytes(), out);
                self.fs.served(&mut self.admission);
                answer
            };
            if answer != Answer::Replied {
                // Waiting (or, in a lend too small even for an `Rerror`, left for a larger one):
                // back in its place, which the removal left room for.
                self.mux.pending.insert(i, entry);
                i += 1;
                continue;
            }
            pos += written(out);
            set(&mut self.mux.sessions[s].tags, entry.tag, false);
            release(&mut self.admission, (bucket, share), entry);
        }
        pos
    }

    fn end_session(&mut self, s: usize) {
        let session = self.mux.sessions.swap_remove(s);
        end(&mut self.mux.pending, &mut self.admission, session);
    }
}
