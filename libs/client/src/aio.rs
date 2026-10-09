//! Many 9P requests outstanding at once, on as few threads as one (userland/native.md, "Many
//! requests at once"): the client half of a multiplexed connection (servers/serving.md,
//! "Multiplexed connections").
//!
//! - **A [`Hub`] owns, it does not run.** One value holds every connection's tags, queue and completion
//!   buffer, and every buffer a request was submitted with: a buffer goes in by value with its request and
//!   comes back by value with its completion ([`Done`]), so the page has one owner at a time. Its methods
//!   take `&mut self`: whichever thread holds it runs it. It is `Send`, never `Sync`: a process that shares
//!   it between threads puts it behind a lock of its own, and that lock is "the hub's lock".
//! - **Submitting is inline.** [`Hub::submit`] sends the request from the calling thread, waiting at most
//!   [`SUBMIT_TIMEOUT_US`] for the server to take it, so a busy server never stalls the caller. A send not
//!   taken stays queued, in order, and is sent again at the hub's next entry: a submit, a completion handed
//!   in, a [`Hub::wait`] or a [`Hub::poll`]. Nothing else sends it: a caller that queues work and then idles
//!   re-enters the hub within [`RETRY_US`] while anything is queued (a poll, or a wait bounded by
//!   `RETRY_US`); a receive that outlives that is the caller's bug, not the hub's. Requests queued together
//!   go end to end in transfers of one page, so many small ones cost one page and a send never needs more
//!   than the page a server's share may allow; a request longer than a page goes alone. [`Hub::batch`] queues
//!   a run of submits to send them so.
//! - **Completions.** Each connection has one completion call, which names how long the server may hold it;
//!   its own kernel timeout is that hold and [`COLLECT_MARGIN_US`], so a timeout means the server broke its
//!   promise and the session is lost. A server ends a session its session bound (at most [`COLLECT_WAIT`])
//!   after its last completion call returned with none parked, so a connection with requests outstanding
//!   keeps one parked.
//!   - With one connection the caller may make it itself when it would idle ([`Hub::wait`], holding it at
//!     most until its next deadline), and no other thread exists. Only a caller that idles at least every
//!     `COLLECT_WAIT / 2` may: any other gives the connection a waiter.
//!   - With more, each has a waiter thread ([`Hub::spawn_waiter`]) blocked in it, calling again at once; the
//!     waiter hands its filled buffer to the caller as the transfer of a one-word wake-up `send` to the
//!     caller's own endpoint, where the caller idles in `receive` with a `max_transfer` of
//!     [`COMPLETION_PAGES`], and the caller hands it to the hub ([`Hub::deliver`]). A waiter talks to no
//!     other server. A caller busy elsewhere does not cost its sessions: a hand-over not taken within
//!     [`HAND_OVER_US`] is held, in its answers' own pages, while the waiter keeps calling, up to
//!     [`MAX_HELD`] of them. A waiter returns once it has handed over its connection's end; the connection is
//!     then the caller's to let go ([`Hub::release`]), so a caller that bounds its waiters gets the place
//!     back when a server ends.
//! - **An `Rerror` keeps its name** ([`Outcome::Rerror`]), read by the one table's decoder as the blocking
//!   client reads it; `busy`, over the connection's share, is [`Outcome::Busy`].
//! - **The server is not trusted.** An answer must frame, decode and carry a tag outstanding on its
//!   connection; anything else ends the connection, as the server's own end does: every request still
//!   outstanding comes back [`Outcome::Ended`], with its buffer, its fate unknown.
//! - **Flush.** A request flushed before it was sent comes back [`Outcome::Flushed`] at once. One already
//!   sent comes back with its answer, if the server gave one before its `Rflush`, or `Flushed` with the
//!   `Rflush`: once either way, with its buffer.
//! - **A tag names one request until its completion is taken**, not only until its answer is read: a caller
//!   may match completions to its requests by tag.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_rt::abi::{Error as SysError, FOREVER, MAX_LEND_PAGES, PAGE_SIZE};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Delivery, Words};
use redoubt_rt::server::ninep::{COLLECT_WAIT, IN_WORDS, MAX_TAGS, OPENED, collect_words};
use redoubt_rt::wire::MSIZE;
use redoubt_rt::wire::ninep::{Body, IOHDRSZ, Message, message_size};

use crate::error::{Error, Name, Refusal};

/// How long a submit waits for its server to take the request (µs) before it is queued instead.
pub const SUBMIT_TIMEOUT_US: u64 = 1_000;
/// How much longer than its hold a completion call waits for its answer (µs) before the server
/// is taken to have broken its promise.
pub const COLLECT_MARGIN_US: u64 = 1_000_000;
/// How long a completion call is held at most while a request waits in the queue (µs), so the
/// queue is tried again soon.
pub const RETRY_US: u64 = 10_000;
/// The most data one [`Hub::write`] carries: its `Twrite` and the header fit one page, the most a
/// small server's share gives a badge, so a longer write is split by its caller.
pub const MAX_WRITE: usize = PAGE_SIZE - IOHDRSZ;
/// A completion call's buffer: room for one answer of the whole msize.
pub const COMPLETION_PAGES: usize = MAX_LEND_PAGES;
/// Word 0 of a waiter's wake-up, which carries its connection's index, the completion call's
/// reply words 0 and 1, and its buffer.
pub const WAKE: u64 = 0xa10;
/// Word 2 of a wake-up whose completion call failed rather than being answered: no status a
/// reply carries, and a word on both widths.
const FAILED: u64 = u32::MAX as u64;
/// How long a waiter's hand-over waits for its caller to take it (µs) before the waiter calls
/// again, so a caller away from its endpoint for longer than a session bound keeps its session.
pub const HAND_OVER_US: u64 = COLLECT_WAIT / 4;
/// The most hand-overs a waiter holds for a caller that has not taken them, each in its answers'
/// own pages: with this many it reads no more and waits for the caller without bound, and the
/// server may end the session at its bound.
pub const MAX_HELD: usize = 4;
/// A waiter thread's stack, in pages.
const WAITER_STACK_PAGES: usize = 8;
const WORD: usize = core::mem::size_of::<usize>();

/// One of the hub's connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conn(usize);

/// How a request ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `Rread`: this many bytes, at the front of the request's buffer.
    Read(usize),
    /// `Rwrite`: this many bytes taken.
    Wrote(u32),
    /// Flushed before any answer, or the `Rflush` of a flush.
    Flushed,
    /// Over the connection's share at the server: not served.
    Busy,
    /// Any other `Rerror`, by its text's name in the one table (servers/wire.md, "Error names"); the
    /// text is not kept.
    Rerror(Name),
    /// Any other R-message, whole.
    Reply(Vec<u8>),
    /// The connection ended with the request outstanding: whether it happened is unknown.
    Ended,
}

/// A request's completion, and the buffer it was submitted with.
#[derive(Debug)]
pub struct Done {
    pub conn: Conn,
    pub tag: u16,
    pub outcome: Outcome,
    pub buffer: Option<Buffer>,
}

/// A request not yet taken by its server, as its T-message.
struct Queued {
    tag: u16,
    message: Vec<u8>,
}

/// An outstanding tag: the buffer it holds, and for a flush the tag it flushes.
struct Slot {
    buffer: Option<Buffer>,
    flushes: Option<u16>,
}

struct Connection {
    endpoint: Endpoint,
    slots: Vec<Option<Slot>>,
    queue: VecDeque<Queued>,
    /// The completion call's buffer, while no call is out with it.
    lend: Option<Buffer>,
    /// The badge of its waiter's wake-ups, while it has a waiter: from its start until its last
    /// wake-up is taken, after which the waiter returns.
    waiter: Option<u64>,
    ended: bool,
}

/// Every connection's tags, queue and buffers.
#[derive(Default)]
pub struct Hub {
    conns: Vec<Connection>,
    done: VecDeque<Done>,
    /// Inside [`Hub::batch`]: submits queue, and send at its end.
    batching: bool,
}

impl Hub {
    pub fn new() -> Hub { Hub::default() }

    /// Opens a multiplexed session on `endpoint` (a connection the caller has attached, its fids
    /// its own to name): one completion call, answered at once.
    pub fn connect(&mut self, endpoint: Endpoint) -> Result<Conn, Error> {
        let lend = Buffer::new(COMPLETION_PAGES)?;
        let (reply, lend) = endpoint.call(&collect_words(0), &[], Some(lend), FOREVER).into_result()?;
        if reply.words[0] != 0 {
            return Err(Error::Server(reply.words[0] as u32));
        }
        if reply.words != [0, 0, OPENED, 0] || !reply.handles.as_slice().is_empty() {
            return Err(Error::Unexpected);
        }
        let slots = (0..MAX_TAGS).map(|_| None).collect();
        let conn = Connection { endpoint, slots, queue: VecDeque::new(), lend, waiter: None, ended: false };
        self.conns.push(conn);
        Ok(Conn(self.conns.len() - 1))
    }

    /// Sends `body` on `conn` with a fresh tag, holding `buffer` for its completion: a `Tread`'s
    /// data is copied into it. Returns the tag.
    pub fn submit(&mut self, conn: Conn, body: Body<'_>, buffer: Option<Buffer>) -> Result<u16, Error> {
        let tag = self.free_tag(conn)?;
        let flushes = match body {
            Body::Tflush { oldtag } => Some(oldtag),
            _ => None,
        };
        let message = encode(Message { tag, body })?;
        self.enqueue(conn, Queued { tag, message }, Slot { buffer, flushes });
        Ok(tag)
    }

    /// Runs `submits`, sending what they queue together at the end: requests sent together go
    /// end to end in one transfer.
    pub fn batch<R>(&mut self, submits: impl FnOnce(&mut Hub) -> R) -> R {
        self.batching = true;
        let r = submits(self);
        self.batching = false;
        self.poll();
        r
    }

    /// A `Tread` of `buffer`'s length (at most what one answer carries) at `offset`.
    pub fn read(&mut self, conn: Conn, fid: u32, offset: u64, buffer: Buffer) -> Result<u16, Error> {
        let count = buffer.len().min(COMPLETION_PAGES * PAGE_SIZE - IOHDRSZ) as u32;
        self.submit(conn, Body::Tread { fid, offset, count }, Some(buffer))
    }

    /// A `Twrite` of the first `len` bytes of `buffer` at `offset`, at most [`MAX_WRITE`] (else
    /// `TooLarge`, for the caller to split). The data is sent in a page of its own, so the buffer
    /// comes back with the completion like any other.
    pub fn write(
        &mut self,
        conn: Conn,
        fid: u32,
        offset: u64,
        buffer: Buffer,
        len: usize,
    ) -> Result<u16, Error> {
        let tag = self.free_tag(conn)?;
        let data = buffer.get(..len).filter(|_| len <= MAX_WRITE);
        let data = data.ok_or(Error::Wire(redoubt_rt::wire::Error::TooLarge))?;
        let message = encode(Message { tag, body: Body::Twrite { fid, offset, data } })?;
        self.enqueue(conn, Queued { tag, message }, Slot { buffer: Some(buffer), flushes: None });
        Ok(tag)
    }

    /// A tag free on `conn`: not outstanding (a queued request holds its slot too), not named by a
    /// flush still outstanding, whose `Rflush` would take a new request's slot for its target's
    /// when the target's own answer came first, and not a completion's the caller has yet to take.
    /// A completion's slot is free once its answer is read, and one completion buffer may carry
    /// several answers: a request the caller sends on taking the first must not share a tag with
    /// one still to come, or the caller, matching completions to requests by tag, gives one
    /// request's answer to the other.
    fn free_tag(&self, conn: Conn) -> Result<u16, Error> {
        let c = &self.conns[conn.0];
        if c.ended {
            return Err(Error::Disconnected);
        }
        let mut taken = [false; MAX_TAGS];
        // A completion's tag is one of its connection's slots, so below `MAX_TAGS`.
        self.done.iter().filter(|d| d.conn == conn).for_each(|d| taken[usize::from(d.tag)] = true);
        for (tag, slot) in c.slots.iter().enumerate() {
            let Some(slot) = slot else { continue };
            taken[tag] = true;
            if let Some(old) = slot.flushes.and_then(|old| taken.get_mut(usize::from(old))) {
                *old = true;
            }
        }
        Ok((0..MAX_TAGS as u16).find(|t| !taken[usize::from(*t)]).ok_or(Refusal::NoTag)?)
    }

    /// Queues a request and sends what the server will take. A flush of a request still queued
    /// sends neither: both are done at once.
    fn enqueue(&mut self, conn: Conn, queued: Queued, slot: Slot) {
        let c = &mut self.conns[conn.0];
        if let Some(i) = slot.flushes.and_then(|old| c.queue.iter().position(|q| q.tag == old)) {
            let old = c.queue.remove(i).map_or(0, |q| q.tag);
            let held = c.slots[usize::from(old)].take().and_then(|s| s.buffer);
            self.done.push_back(Done { conn, tag: old, outcome: Outcome::Flushed, buffer: held });
            self.done.push_back(Done {
                conn,
                tag: queued.tag,
                outcome: Outcome::Flushed,
                buffer: slot.buffer,
            });
            return;
        }
        c.slots[usize::from(queued.tag)] = Some(slot);
        c.queue.push_back(queued);
        if !self.batching {
            self.send_queued(conn.0);
        }
    }

    /// Sends what is queued, on every connection.
    pub fn poll(&mut self) {
        for c in 0..self.conns.len() {
            self.send_queued(c);
        }
    }

    /// How many requests are not yet taken by their servers.
    pub fn queued(&self) -> usize { self.conns.iter().map(|c| c.queue.len()).sum() }

    /// How many waiter threads the hub has started that have not yet handed over their last
    /// wake-up.
    pub fn waiters(&self) -> usize { self.conns.iter().filter(|c| c.waiter.is_some()).count() }

    /// Lets go of `conn` if it is over and no thread of the hub's is left in it: it ended, and its
    /// waiter, if it had one, has handed over its last wake-up and returned. What its record held
    /// (its tags, its queue, its completion buffer) is freed, and `true` comes back; `conn` itself
    /// stays its own, never another connection's, and is refused `Disconnected`. A connection
    /// still running, or whose waiter has yet to hand over its end, is kept: `false`.
    pub fn release(&mut self, conn: Conn) -> bool {
        let c = &mut self.conns[conn.0];
        if !c.ended || c.waiter.is_some() {
            return false;
        }
        c.slots = Vec::new();
        c.queue = VecDeque::new();
        c.lend = None;
        true
    }

    /// The next completion, if any.
    pub fn completed(&mut self) -> Option<Done> { self.done.pop_front() }

    /// Waits in `conn`'s completion call itself, which its server holds at most `hold_us` µs (at
    /// most [`RETRY_US`] while anything is queued), and takes what it brings: for a caller with
    /// one connection and nothing else to wake it. Not for a connection with a waiter.
    pub fn wait(&mut self, conn: Conn, hold_us: u64) -> Result<(), Error> {
        self.poll();
        let c = &mut self.conns[conn.0];
        if c.ended {
            return Err(Error::Disconnected);
        }
        if c.waiter.is_some() {
            return Err(Error::Unexpected);
        }
        let lend = match c.lend.take() {
            Some(lend) => lend,
            None => Buffer::new(COMPLETION_PAGES)?,
        };
        let hold = if c.queue.is_empty() { hold_us } else { hold_us.min(RETRY_US) }.min(COLLECT_WAIT);
        let mut outcome = c.endpoint.call(&collect_words(hold), &[], Some(lend), hold + COLLECT_MARGIN_US);
        let reply = match (&outcome.status, &outcome.reply) {
            (Ok(()), Some(reply)) => Some([reply.words[0], reply.words[1], reply.words[2]]),
            _ => None,
        };
        self.collected(conn.0, reply, outcome.buffer.take());
        self.poll();
        Ok(())
    }

    /// Starts `conn`'s waiter: a thread blocked in its completion call, which hands each filled
    /// buffer to the caller as the transfer of a wake-up `send` to `receive` (the caller's own
    /// endpoint) through a handle minted with `badge`. The caller passes each such delivery to
    /// [`Hub::deliver`].
    pub fn spawn_waiter(&mut self, conn: Conn, receive: &Endpoint, badge: NonZeroU64) -> Result<(), Error> {
        let c = &mut self.conns[conn.0];
        let wake = receive.mint(badge, None)?;
        let lend = match c.lend.take() {
            Some(lend) => lend,
            None => Buffer::new(COMPLETION_PAGES)?,
        };
        let endpoint = Endpoint::from_handle(c.endpoint.handle());
        let index = conn.0 as u64;
        let body = alloc::boxed::Box::new(move || waiter(endpoint, wake, index, lend));
        redoubt_rt::thread::spawn(body, WAITER_STACK_PAGES)?;
        c.waiter = Some(badge.get());
        Ok(())
    }

    /// A delivery on the caller's endpoint: a waiter's wake-up is taken here, its buffer and all;
    /// anything else is handed back.
    pub fn deliver(&mut self, mut delivery: Delivery) -> Option<Delivery> {
        let conn = usize::try_from(delivery.words[1]).ok().filter(|c| *c < self.conns.len());
        let ours = conn.filter(|c| self.conns[*c].waiter == Some(delivery.caller.badge));
        let (true, Some(c)) = (delivery.words[0] == WAKE, ours) else { return Some(delivery) };
        let reply = (delivery.words[2] != FAILED).then_some([delivery.words[2], delivery.words[3], 0]);
        // Any first word but 0 is the waiter's last hand-over: the connection is over, and the
        // waiter returns once this is sent.
        if delivery.words[2] != 0 {
            self.conns[c].waiter = None;
        }
        self.collected(c, reply, delivery.transfer.take());
        self.poll();
        None
    }

    /// Takes a completion call's reply words 0 to 2 (`None`: the call failed) and its buffer.
    fn collected(&mut self, c: usize, reply: Option<[u64; 3]>, buffer: Option<Buffer>) {
        // Failed, ended, refused, or a session the server opened afresh: what was outstanding is gone.
        let (Some([0, bytes, 0]), Some(lend)) = (reply, buffer) else { return self.end(c) };
        let n = usize::try_from(bytes).unwrap_or(usize::MAX);
        if n > lend.len() || self.answers(c, &lend[..n]).is_err() {
            self.end(c);
        }
        self.conns[c].lend = Some(lend);
    }

    /// Hands out every answer in `bytes`; an error ends the connection.
    fn answers(&mut self, c: usize, mut bytes: &[u8]) -> Result<(), ()> {
        let conn = Conn(c);
        while !bytes.is_empty() {
            let size = message_size(bytes).map_err(|_| ())?;
            let message = Message::decode(&bytes[..size]).map_err(|_| ())?;
            let slot =
                self.conns[c].slots.get_mut(usize::from(message.tag)).and_then(Option::take).ok_or(())?;
            let mut buffer = slot.buffer;
            let outcome = match message.body {
                Body::Rread { data } => match buffer.as_mut() {
                    Some(b) if data.len() <= b.len() => {
                        b[..data.len()].copy_from_slice(data);
                        Outcome::Read(data.len())
                    }
                    // More than was asked for.
                    Some(_) => return Err(()),
                    None => Outcome::Reply(bytes[..size].to_vec()),
                },
                Body::Rwrite { count } => Outcome::Wrote(count),
                Body::Rerror { ename } if Name::of(ename) == Name::Busy => Outcome::Busy,
                Body::Rerror { ename } => Outcome::Rerror(Name::of(ename)),
                Body::Rflush => {
                    // The flushed request, if its answer did not come first: it ends here, once.
                    let old =
                        slot.flushes.and_then(|old| self.conns[c].slots.get_mut(usize::from(old))?.take());
                    if let (Some(old), Some(tag)) = (old, slot.flushes) {
                        self.done.push_back(Done {
                            conn,
                            tag,
                            outcome: Outcome::Flushed,
                            buffer: old.buffer,
                        });
                    }
                    Outcome::Flushed
                }
                _ => Outcome::Reply(bytes[..size].to_vec()),
            };
            self.done.push_back(Done { conn, tag: message.tag, outcome, buffer });
            bytes = &bytes[size..];
        }
        Ok(())
    }

    /// Sends `c`'s queue in order until a send is not taken: one request alone in the words if it
    /// fits them, else as many as fit one page, end to end, or one longer than a page alone.
    fn send_queued(&mut self, c: usize) {
        let conn = &mut self.conns[c];
        while let Some(first) = conn.queue.front() {
            let (count, sent) = if conn.queue.len() == 1 && first.message.len() <= IN_WORDS {
                (1, conn.endpoint.send(&in_words(&first.message), &[], None, SUBMIT_TIMEOUT_US))
            } else {
                let mut len = 0;
                // One page: what a share of a server's `Pages` may be (a bucket of 2 is one page a
                // badge), and 178 reads' worth.
                let room = PAGE_SIZE.max(first.message.len());
                let mut count = 0;
                for q in &conn.queue {
                    if len + q.message.len() > room {
                        break;
                    }
                    len += q.message.len();
                    count += 1;
                }
                let Ok(mut pages) = Buffer::new(len.div_ceil(PAGE_SIZE)) else { return };
                let mut at = 0;
                for q in conn.queue.iter().take(count) {
                    pages[at..at + q.message.len()].copy_from_slice(&q.message);
                    at += q.message.len();
                }
                (count, conn.endpoint.send(&[0, len as u64, 0, 0], &[], Some(pages), SUBMIT_TIMEOUT_US))
            };
            match sent {
                Ok(()) => drop(conn.queue.drain(..count)),
                // Not taken: the server is busy. They go again at the next entry, first.
                Err((SysError::Timeout | SysError::Busy, _)) => return,
                Err(_) => return self.end(c),
            }
        }
    }

    /// The connection is over: everything outstanding comes back `Ended`.
    fn end(&mut self, c: usize) {
        let conn = &mut self.conns[c];
        conn.ended = true;
        conn.queue.clear();
        for (tag, slot) in conn.slots.iter_mut().enumerate() {
            if let Some(slot) = slot.take() {
                let done =
                    Done { conn: Conn(c), tag: tag as u16, outcome: Outcome::Ended, buffer: slot.buffer };
                self.done.push_back(done);
            }
        }
    }
}

/// A request's T-message, in a vector of its own length: a queued request holds its bytes, not
/// the whole msize it was encoded in.
fn encode(message: Message<'_>) -> Result<Vec<u8>, Error> {
    let mut scratch = alloc::vec![0; MSIZE];
    let n = message.encode(&mut scratch)?;
    Ok(scratch[..n].to_vec())
}

/// The words of a request whose T-message fits them: packed into words 1 to 3.
fn in_words(message: &[u8]) -> Words {
    let mut small = [0u8; 3 * 8];
    small[..message.len()].copy_from_slice(message);
    let mut words = [0; 4];
    for (word, chunk) in words[1..].iter_mut().zip(small.as_chunks::<WORD>().0) {
        *word = usize::from_le_bytes(*chunk) as u64;
    }
    words
}

/// A waiter's whole life: the completion call, again and again, each filled buffer handed to the
/// caller with a wake-up, until the connection or the caller is gone. A hand-over the caller does
/// not take within [`HAND_OVER_US`] is held, in its answers' own pages, while the waiter calls
/// again with a hold of 0, which keeps the session and takes what is ready; at [`MAX_HELD`] it
/// waits for the caller without bound. Its wake-up handle, minted for it alone, is closed when it
/// returns.
fn waiter(endpoint: Endpoint, wake: Endpoint, index: u64, lend: Buffer) {
    hand_over(&endpoint, &wake, index, lend);
    let _ = redoubt_rt::handle::close(wake.handle());
}

fn hand_over(endpoint: &Endpoint, wake: &Endpoint, index: u64, lend: Buffer) {
    let mut lend = Some(lend);
    // Hand-overs not yet taken, oldest first: the reply's words 0 and 1, and the answers.
    let mut held: VecDeque<([u64; 2], Option<Buffer>)> = VecDeque::new();
    loop {
        // Once the connection is over nothing more is read, so what is left waits without bound.
        let ended = held.back().is_some_and(|(words, _)| words[0] != 0);
        while let Some((words, buffer)) = held.pop_front() {
            let wait = if ended || held.len() + 1 >= MAX_HELD { FOREVER } else { HAND_OVER_US };
            match wake.send(&[WAKE, index, words[0], words[1]], &[], buffer, wait) {
                Ok(()) => {}
                Err((SysError::Timeout, buffer)) => {
                    let (buffer, spare) = compact(words, buffer);
                    lend = lend.or(spare);
                    held.push_front((words, buffer));
                    break;
                }
                Err(_) => return,
            }
        }
        if ended {
            return;
        }
        let Some(buffer) = lend.take().or_else(|| Buffer::new(COMPLETION_PAGES).ok()) else {
            held.push_back(([FAILED, 0], None));
            continue;
        };
        let hold = if held.is_empty() { COLLECT_WAIT } else { 0 };
        let mut outcome = endpoint.call(&collect_words(hold), &[], Some(buffer), hold + COLLECT_MARGIN_US);
        // A session the server opened afresh (word 2) is as good as an end: what was outstanding
        // is gone.
        let words = match (&outcome.status, &outcome.reply) {
            (Ok(()), Some(reply)) if reply.words[2] == 0 => [reply.words[0], reply.words[1]],
            _ => [FAILED, 0],
        };
        // Answered empty at the end of its hold: nothing to hand over; call again.
        if words == [0, 0] && outcome.buffer.is_some() {
            lend = outcome.buffer.take();
            continue;
        }
        held.push_back((words, outcome.buffer.take()));
    }
}

/// A held hand-over's answers moved into pages of their own length, and the completion buffer
/// they leave free; as it was if it carries no answers or no smaller pages are to be had.
fn compact(words: [u64; 2], buffer: Option<Buffer>) -> (Option<Buffer>, Option<Buffer>) {
    let Some(full) = buffer else { return (None, None) };
    let n = usize::try_from(words[1]).unwrap_or(usize::MAX);
    if words[0] != 0 || n > full.len() || n.div_ceil(PAGE_SIZE) >= full.len() / PAGE_SIZE {
        return (Some(full), None);
    }
    let Ok(mut own) = Buffer::new(n.div_ceil(PAGE_SIZE).max(1)) else { return (Some(full), None) };
    own[..n].copy_from_slice(&full[..n]);
    (Some(own), Some(full))
}
