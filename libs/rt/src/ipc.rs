//! IPC (kernel/ipc.md, "Messages"): pages to lend and transfer, `call`, `send`, `receive`,
//! `reply` and `serve`.
//!
//! Words are `u64` here, as in `redoubt-wire`, so one layout serves both widths; a word that does
//! not fit the machine's word is `InvalidArgument` before anything is sent.

use core::num::NonZeroU64;
use core::ops::{Deref, DerefMut};

use redoubt_sys::{
    BODY_SLOTS, Body, BodyOf, Call, Error, ExitNotice, Handle, Handles, Labels, LendDisposition, MemFlags,
    MessageKind, MintSource, PAGE_SIZE, Pages, RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles,
    ReplyOutcome, Return, Slot, WORDS,
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
    mapping: Mapping,
}

/// A unique view of writable RAM that the kernel has mapped into this process. Private:
/// only freshly mapped, received, or explicitly returned pages may be adopted. This view
/// does not unmap on drop: a received lend is released by `reply`, not by dropping a Request.
/// The reference never escapes with its storage lifetime; users get ordinary reborrows.
struct Mapping {
    pages: Pages,
    bytes: &'static mut [u8],
}

impl core::fmt::Debug for Mapping {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Do not inspect or print message contents as part of ownership diagnostics.
        self.pages.fmt(f)
    }
}

impl Mapping {
    fn adopt(pages: Pages) -> Self {
        let len = pages.npages.get().checked_mul(PAGE_SIZE).expect("mapping length overflow");
        assert!(pages.addr != 0 && pages.addr.is_multiple_of(PAGE_SIZE), "invalid mapping address");
        assert!(
            len <= isize::MAX as usize && pages.addr.checked_add(len).is_some(),
            "invalid mapping length"
        );
        // SAFETY: every caller adopts only RAM just mapped writable by map_anon, received
        // exclusively in a transfer/lend (R3/R4), or returned by call/send/reply completion.
        // The kernel guarantees initialized contiguous pages and exclusive access; the checks
        // above also bound the slice. Safe reborrows cannot outlive this private view, whose
        // reference is cleared before any syscall can unmap or transfer the pages.
        let bytes = unsafe { core::slice::from_raw_parts_mut(pages.addr as *mut u8, len) };
        Self { pages, bytes }
    }

    /// End the Rust view before entering a syscall that can remove the mapping.
    fn release(&mut self) -> Pages {
        self.bytes = &mut [];
        self.pages
    }
}

impl Buffer {
    /// `npages` zeroed pages (`map_anon`).
    pub fn new(npages: usize) -> Result<Buffer, Error> {
        let npages = core::num::NonZeroUsize::new(npages).ok_or(Error::InvalidArgument)?;
        let len = npages.get().checked_mul(PAGE_SIZE).ok_or(Error::TooLarge)?;
        let addr = crate::handle::map_anon(len, MemFlags::READ | MemFlags::WRITE)?;
        Ok(Buffer::adopt(Pages { addr, npages }))
    }

    fn adopt(pages: Pages) -> Self { Self { mapping: Mapping::adopt(pages) } }

    pub fn npages(&self) -> usize { self.mapping.pages.npages.get() }

    /// Gives up ownership without unmapping (the pages are about to be transferred).
    fn into_pages(mut self) -> Pages {
        let pages = self.mapping.release();
        core::mem::forget(self);
        pages
    }
}

impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] { self.mapping.bytes }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut [u8] { self.mapping.bytes }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // A failure means the pages are already gone; there is nothing else to do.
        let pages = self.mapping.release();
        let _ = crate::handle::unmap(pages.addr, pages.npages.get() * PAGE_SIZE);
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
#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub words: Words,
    /// Handles the server sent, now in this process's table; `None` for one that could not be
    /// taken (kernel/ipc.md R4) or was revoked on the way. The recipient owns these handles.
    pub handles: ReceivedHandles,
}

/// Ownership survives errors: inspect all three fields, not just `status`.
/// Dropping an unclaimed reply closes its handles, including a partial OutOfMemory reply.
#[derive(Debug)]
#[must_use = "inspect status, returned buffer and any committed reply"]
pub struct CallOutcome {
    pub status: Result<(), Error>,
    pub buffer: Option<Buffer>,
    pub reply: Option<Reply>,
}

impl CallOutcome {
    /// Convenience for clients that discard partial replies on error. All such handles are
    /// closed; a returned buffer is dropped on error. Successful ownership moves to the caller.
    pub fn into_result(mut self) -> Result<(Reply, Option<Buffer>), Error> {
        self.status?;
        let reply = self.reply.take().expect("successful call without committed reply");
        Ok((reply, self.buffer.take()))
    }
}

impl Drop for CallOutcome {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            for h in reply.handles.as_slice().iter().flatten() {
                let _ = crate::handle::close(*h);
            }
        }
    }
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

    /// Calls the endpoint, taking ownership of `lend`. Abandonment after receipt consumes it;
    /// every other completion returns it in the outcome, independently of status.
    pub fn call(&self, words: &Words, handles: &[Handle], lend: Option<Buffer>, timeout: u64) -> CallOutcome {
        let mut rec = match body(words, handles) {
            Ok(body) => Record::<BODY_SLOTS>(body.encode()),
            Err(e) => return CallOutcome { status: Err(e), buffer: lend, reply: None },
        };
        // Disarm before entering the kernel, so malformed results cannot run a stale destructor.
        let pages = lend.map(Buffer::into_pages);
        let call = Call::Call { endpoint: self.handle(), body_rec: rec.addr_mut(), lend: pages, timeout };
        let Ok(Return::Call(outcome)) = syscall(&call) else {
            panic!("invalid IPC call outcome: buffer ownership unknown");
        };
        assert_eq!(outcome.lend == LendDisposition::None, pages.is_none(), "invalid lend outcome");
        let buffer = match outcome.lend {
            LendDisposition::Returned => pages.map(Buffer::adopt),
            LendDisposition::None | LendDisposition::Consumed => None,
        };
        let reply = outcome.reply_present.then(|| {
            let reply = ReceivedBody::decode(&rec.0).expect("invalid committed IPC record");
            Reply { words: words_of(&reply), handles: reply.handles }
        });
        CallOutcome { status: outcome.status, buffer, reply }
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
        nothing(syscall(&call)).map_err(|e| (e, pages.map(Buffer::adopt)))
    }

    /// Waits for a message on this endpoint (the receive right). Accepts a transfer of at most
    /// `max_transfer` pages (R4).
    pub fn receive(&self, timeout: u64, max_transfer: usize) -> Result<Event, Error> {
        receive_raw(Some(self.handle()), timeout, max_transfer)
    }
}

/// `mint` from the open call `id` of this thread, with the default stamp (for a server that
/// holds the call's id but not its [`Request`], which the lend is borrowed from).
pub(crate) fn mint_from_message(id: NonZeroU64, badge: NonZeroU64) -> Result<Endpoint, Error> {
    mint(MintSource::Message(id), badge, None)
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
    /// An abandoned-call notice (kernel/ipc.md R3): the open call with this id, which this
    /// thread holds, lost its caller. It stays open, holding one of the process's
    /// `MAX_OPEN_CALLS`, until this thread replies to it; the reply reaches nobody.
    /// [`crate::server::parked::Parked::abandoned`] does that for a parked call.
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
                    let lend = lend.map(Mapping::adopt);
                    Event::Call(Request { id: m.msg_id, caller, words, handles, lend })
                }
                MessageKind::Send { transfer } => {
                    let transfer = transfer.map(Buffer::adopt);
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
    /// as `Malformed` (servers/wire.md: a missing handle).
    pub handles: ReceivedHandles,
    lend: Option<Mapping>,
}

impl Request {
    pub fn id(&self) -> NonZeroU64 { self.id }

    /// The caller's lent buffer (empty if none): the request's bytes, and where the reply's go.
    /// The caller cannot see it change until the reply; it is hostile input all the same.
    pub fn lend(&mut self) -> &mut [u8] {
        match self.lend.as_mut() {
            Some(mapping) => mapping.bytes,
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
    /// calls this when it resumes work on a call it took earlier (a parked call:
    /// [`crate::server::parked`] does it).
    pub fn serve(&self) -> Result<(), Error> { nothing(syscall(&Call::Serve { msg_id: self.id })) }

    /// Replies, which returns the lend to the caller.
    ///
    /// The handles are **copied** into the caller (kernel/ipc.md, "Messages"): this process keeps
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
    pub fn reply(mut self, words: &Words, handles: &[Handle]) -> Result<ReplyOutcome, (Error, Request)> {
        let rec = match body(words, handles) {
            Ok(body) => Record::<BODY_SLOTS>(body.encode()),
            Err(e) => return Err((e, self)),
        };
        let lend = self.lend.take().map(|mut mapping| mapping.release());
        match syscall(&Call::Reply { msg_id: self.id, body_rec: rec.addr() }) {
            Ok(Return::Reply(outcome)) => Ok(outcome.validate(handles.len()).expect("invalid reply mask")),
            Ok(_) => panic!("invalid IPC reply outcome"),
            Err(e) => {
                // Rejected replies leave the open call and its lend in the server.
                self.lend = lend.map(Mapping::adopt);
                Err((e, self))
            }
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
