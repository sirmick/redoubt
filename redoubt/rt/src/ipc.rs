//! IPC (KERNEL-SPEC.md, Messages; CAPABILITIES.md, IPC): pages to lend and transfer, `call`,
//! `send`, `receive`, `reply` and `serve`.
//!
//! Words are `u64` here, as in `redoubt-wire`, so one layout serves both widths; a word that does
//! not fit the machine's word is `InvalidArgument` before anything is sent.

use core::num::NonZeroU64;
use core::ops::{Deref, DerefMut};

use redoubt_sys::{
    BODY_SLOTS, Body, BodyOf, Call, Error, ExitNotice, Handle, Handles, Labels, MemFlags, MessageKind,
    MintSource, PAGE_SIZE, Pages, RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles, Slot, WORDS,
};

use crate::handle::{Budget, Endpoint, handle, nothing};
use crate::sys::{Record, syscall};

/// A message's words, widened to `u64` (the same type as `redoubt_wire::typed::Words`).
pub type Words = [u64; WORDS];

/// Who sent a message, as the kernel attached it: unforgeable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caller {
    /// The badge of the handle it came through: which grant is in use.
    pub badge: u64,
    /// The sender budget's account (0 = none).
    pub account: u64,
    /// The sender budget's labels, sorted and deduplicated by the kernel.
    pub labels: Labels,
}

/// Pages this process owns, mapped read-write: what it lends in a `call`, transfers in a `send`,
/// or was transferred. Unmapped on drop.
#[derive(Debug)]
pub struct Buffer {
    pages: Pages,
}

impl Buffer {
    /// `npages` zeroed pages (`map_anon`).
    pub fn new(npages: usize) -> Result<Buffer, Error> {
        let npages = core::num::NonZeroUsize::new(npages).ok_or(Error::InvalidArgument)?;
        let len = npages.get().checked_mul(PAGE_SIZE).ok_or(Error::TooLarge)?;
        let addr = crate::handle::map_anon(len, MemFlags::READ | MemFlags::WRITE)?;
        Ok(Buffer { pages: Pages { addr, npages } })
    }

    pub fn npages(&self) -> usize { self.pages.npages.get() }

    /// Gives up ownership without unmapping (the pages are about to be transferred).
    fn into_pages(self) -> Pages {
        let pages = self.pages;
        core::mem::forget(self);
        pages
    }
}

/// The length of `pages` in bytes; 0 if it overflows, which a real mapping cannot.
fn byte_len(pages: Pages) -> usize { pages.npages.get().checked_mul(PAGE_SIZE).unwrap_or(0) }

impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: `pages` came from `map_anon` (read-write) or from a transfer the kernel mapped
        // into this process, and stays mapped until drop; a Buffer is the only owner, so the
        // shared borrow of `self` rules out a concurrent mutable view.
        unsafe { core::slice::from_raw_parts(self.pages.addr as *const u8, byte_len(self.pages)) }
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `deref`; the unique borrow of `self` makes this the only view.
        unsafe { core::slice::from_raw_parts_mut(self.pages.addr as *mut u8, byte_len(self.pages)) }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // A failure means the pages are already gone; there is nothing else to do.
        let _ = crate::handle::unmap(self.pages.addr, byte_len(self.pages));
    }
}

fn body(words: &Words, handles: &[Handle]) -> Result<Body, Error> {
    let mut body = Body { words: [0; WORDS], handles: Handles::from_slice(handles)? };
    for (word, value) in body.words.iter_mut().zip(words) {
        *word = usize::try_from(*value).map_err(|_| Error::InvalidArgument)?;
    }
    Ok(body)
}

fn words_of<H: Slot>(body: &BodyOf<H>) -> Words { body.words.map(|w| w as u64) }

/// The reply to a `call`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub words: Words,
    /// Handles the server sent, now in this process's table; `None` for one that could not be
    /// taken (QUESTIONS.md 116, pending) or was revoked on the way.
    pub handles: ReceivedHandles,
}

impl Endpoint {
    /// A new endpoint; the handle is its receive right (badge 0).
    pub fn create() -> Result<Endpoint, Error> {
        handle(syscall(&Call::EndpointCreate)).map(Endpoint::from_handle)
    }

    /// A handle to this endpoint with `badge` (this must be the receive right), stamped with
    /// this handle's stamp or, narrower, `budget`.
    pub fn mint(&self, badge: NonZeroU64, budget: Option<&Budget>) -> Result<Endpoint, Error> {
        mint(MintSource::Handle(self.handle()), badge, budget)
    }

    /// Calls the endpoint and waits for the reply, lending `lend` for the duration.
    pub fn call(
        &self,
        words: &Words,
        handles: &[Handle],
        lend: Option<&mut Buffer>,
        timeout: u64,
    ) -> Result<Reply, Error> {
        let mut rec = Record::<BODY_SLOTS>(body(words, handles)?.encode());
        // The unique borrow of the buffer lasts the whole call: the kernel unmaps it from us
        // until the reply, and nothing here can touch it meanwhile.
        let lend = lend.map(|buffer| buffer.pages);
        let call = Call::Call { endpoint: self.handle(), body_rec: rec.addr_mut(), lend, timeout };
        nothing(syscall(&call))?;
        let reply = ReceivedBody::decode(&rec.0)?;
        Ok(Reply { words: words_of(&reply), handles: reply.handles })
    }

    /// Sends one-way, transferring `transfer` for good if the receiver takes it. On failure
    /// the pages are still ours and come back with the error.
    pub fn send(
        &self,
        words: &Words,
        handles: &[Handle],
        transfer: Option<Buffer>,
        timeout: u64,
    ) -> Result<(), (Error, Option<Buffer>)> {
        let rec = match body(words, handles) {
            Ok(body) => Record::<BODY_SLOTS>(body.encode()),
            Err(e) => return Err((e, transfer)),
        };
        let pages = transfer.map(Buffer::into_pages);
        let call = Call::Send { endpoint: self.handle(), body_rec: rec.addr(), transfer: pages, timeout };
        // A failed send moved nothing (R4), so the pages are ours again.
        nothing(syscall(&call)).map_err(|e| (e, pages.map(|pages| Buffer { pages })))
    }

    /// Waits for a message on this endpoint (the receive right). Accepts a transfer of at most
    /// `max_transfer` pages (R4).
    pub fn receive(&self, timeout: u64, max_transfer: usize) -> Result<Event, Error> {
        receive_raw(Some(self.handle()), timeout, max_transfer)
    }
}

fn mint(source: MintSource, badge: NonZeroU64, budget: Option<&Budget>) -> Result<Endpoint, Error> {
    let call = Call::Mint { source, badge, budget: budget.map(Budget::handle) };
    handle(syscall(&call)).map(Endpoint::from_handle)
}

/// What `receive` returned.
#[derive(Debug)]
pub enum Event {
    /// A `call`: reply to it with [`Request::reply`]. It is now this thread's current call.
    Call(Request),
    /// A `send`: nothing to reply.
    Send(Delivery),
    /// The IRQ handle `receive` named fired.
    Interrupt,
    Exit(ExitNotice),
    /// The open call with this id, held by this thread, was abandoned: its caller is gone (R3).
    /// Reply to its [`Request`] to free it; the reply reaches nobody.
    Abandoned(NonZeroU64),
}

pub(crate) fn receive_raw(from: Option<Handle>, timeout: u64, max_transfer: usize) -> Result<Event, Error> {
    let mut rec = Record([0; RECEIVED_SLOTS]);
    nothing(syscall(&Call::Receive { from, timeout, max_transfer, received_rec: rec.addr_mut() }))?;
    Ok(match Received::decode(&rec.0)? {
        Received::Message(m) => {
            let caller = Caller { badge: m.badge, account: m.account, labels: m.labels };
            let (words, handles) = (words_of(&m.body), m.body.handles);
            match m.kind {
                MessageKind::Call { lend } => {
                    Event::Call(Request { id: m.msg_id, caller, words, handles, lend })
                }
                MessageKind::Send { transfer } => {
                    let transfer = transfer.map(|pages| Buffer { pages });
                    Event::Send(Delivery { caller, words, handles, transfer })
                }
            }
        }
        Received::Interrupt => Event::Interrupt,
        Received::Exit(notice) => Event::Exit(notice),
        Received::Abandoned(id) => Event::Abandoned(id),
    })
}

/// A call this process has taken and owes a reply (an open call: it holds one of the process's
/// `MAX_OPEN_CALLS` slots until [`Request::reply`]).
#[derive(Debug)]
#[must_use = "a call must be replied to, or its caller waits and its slot stays taken"]
pub struct Request {
    id: NonZeroU64,
    pub caller: Caller,
    pub words: Words,
    /// Handles the caller sent, now in this process's table; `None` for one revoked while the
    /// call was queued (R10), which keeps its slot. A protocol that needs it treats the request
    /// as `Malformed` (WIRE.md: a missing handle).
    pub handles: ReceivedHandles,
    lend: Option<Pages>,
}

impl Request {
    pub fn id(&self) -> NonZeroU64 { self.id }

    /// The caller's lent buffer (empty if none): the request's bytes, and where the reply's go.
    /// The caller cannot see it change until the reply; it is hostile input all the same.
    pub fn lend(&mut self) -> &mut [u8] {
        match self.lend {
            // SAFETY: the kernel mapped the lend writable into this process when it delivered
            // the call, and unmaps it only at `reply` (R3), which consumes `self`; so the pages
            // stay mapped for this borrow, and the unique borrow of `self` makes it the only view.
            Some(pages) => unsafe { core::slice::from_raw_parts_mut(pages.addr as *mut u8, byte_len(pages)) },
            None => &mut [],
        }
    }

    /// A handle to the endpoint this call came in on, with `badge`, stamped like the handle it
    /// came through or, narrower, `budget` (`mint` from a message).
    pub fn mint(&self, badge: NonZeroU64, budget: Option<&Budget>) -> Result<Endpoint, Error> {
        mint(MintSource::Message(self.id), badge, budget)
    }

    /// Makes this the thread's current call (`serve`): the one a fault of this thread blames,
    /// until it replies to it or takes another. `receive` sets it already; an event-driven server
    /// calls this when it resumes work on a call it took earlier.
    pub fn serve(&self) -> Result<(), Error> { nothing(syscall(&Call::Serve { msg_id: self.id })) }

    /// Replies, which returns the lend to the caller.
    ///
    /// The handles are **copied** into the caller (KERNEL-SPEC.md, Messages): this process keeps
    /// its own, and must close any it does not mean to keep (a handle minted for the caller,
    /// say), or its handle table grows by one per reply.
    ///
    /// On failure the request comes back with the error, so the server can still answer it:
    /// words or handles that cannot be encoded (more than `MAX_MSG_HANDLES` handles, or on rv32 a
    /// word wider than 32 bits) are refused before anything is sent. A request dropped unanswered
    /// keeps its caller waiting and its open-call slot taken.
    // The error carries the request back by value, which is its purpose; it is not boxed
    // because boxing allocates, and a server short of memory must still be able to answer.
    #[allow(clippy::result_large_err)]
    pub fn reply(self, words: &Words, handles: &[Handle]) -> Result<(), (Error, Request)> {
        let rec = match body(words, handles) {
            Ok(body) => Record::<BODY_SLOTS>(body.encode()),
            Err(e) => return Err((e, self)),
        };
        match nothing(syscall(&Call::Reply { msg_id: self.id, body_rec: rec.addr() })) {
            Ok(()) => Ok(()),
            Err(e) => Err((e, self)),
        }
    }
}

/// A message that came by `send`: no reply is owed; transferred pages are ours for good.
#[derive(Debug)]
pub struct Delivery {
    pub caller: Caller,
    pub words: Words,
    /// As in [`Request::handles`].
    pub handles: ReceivedHandles,
    pub transfer: Option<Buffer>,
}
