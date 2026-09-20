//! The `blkd` server: the typed protocol of IO-ARCHITECTURE.md, over `redoubt-rt`'s shared
//! server library.
//!
//! **A typed protocol, not 9P.** `blkd` serves six fixed operations on ranges of sectors and has
//! no namespace to walk; WIRE.md lists the block protocol among the typed ones.
//!
//! **A range is a badge.** The root badge of partition *i* is *i* + 1 in GPT entry order, so
//! `init` mints each volume's range from the boot manifest without asking `blkd` anything, and a
//! restarted `blkd` gives the same badges the same meaning from the same disk while holding no
//! state across the restart (INIT.md, decision 5). Badges at or above [`FIRST_GRANTED_BADGE`] are
//! minted by `grant` and are never reused; each incarnation draws its first at random (answer
//! 126), so a handle granted before a restart names no range afterwards.
//!
//! **Every request** resolves the caller's badge to one range, applies [`check`] to it, and only
//! then touches the device. A badge that names no range and a caller whose labels fail the check
//! get the same `not_permitted`, which says no more than that.
//!
//! **Admission** counts grants, and only grants: `blkd` parks no call and keeps no other
//! per-client state, so a flood of reads makes it grow by nothing. What bounds that flood is the
//! kernel's fair waiting per (account, label set) (R2) and [`crate::virtio::MAX_SECTORS`], which
//! bounds one request's work by a number stated in the note rather than by the caller's lend.

use alloc::vec;
use alloc::vec::Vec;

use redoubt_rt::abi::{Error, Handle, Handles, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Request, Words};
pub use redoubt_rt::server::minted::FIRST_MINTED_BADGE as FIRST_GRANTED_BADGE;
use redoubt_rt::server::minted::{Entry, Kernel, MintError, Minted, Minter};
use redoubt_rt::server::typed::{Answer, Outcome, Protocol, TypedServer, answer, finish};
use redoubt_rt::server::{Access, Admission, AdmitKey, Cost, Limits, Resource, Unsized, check};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    ErrorCode, Flush, FlushReply, Grant, GrantReply, Info, InfoReply, Message, Read, ReadReply, Release,
    ReleaseReply, Reply, Write, WriteReply,
};

use crate::disk::Disk;
use crate::range::Range;
use crate::transport::Transport;
use crate::virtio::{DATA_LEN, DeviceError, MAX_SECTORS, SECTOR_SIZE};

/// The id `release` takes to mean "everything I granted". [`Minted`] never issues it, so it
/// cannot collide with a real one; it is what a holder asks for when its ids are gone, a server
/// `init` restarted on the same root badge among them (INIT.md).
pub const ALL_GRANTS: u64 = 0;

/// What a client may hold in `blkd` at once, per (account, label set) (CONTAINMENT.md).
///
/// - `buckets`: the (account, label set)s `blkd` serves at once. Its clients are `init` and the `fsd`
///   instances, all system class and so one bucket each by badge; sized above what milestone 1 starts, so the
///   cap does not bind in normal use (answer 118).
/// - `in_flight` is 0: no call is ever parked here; every request is answered as it is taken, which is also
///   what makes completions in order (IO-ARCHITECTURE.md) true by construction.
/// - `files` is 0: `blkd` has no files.
/// - `state`: ranges `grant` has made and `release` has not freed.
pub const LIMITS: Limits = Limits { buckets: 8, in_flight: 0, files: 0, state: 8 };

/// What one of those costs `blkd`, in bytes: a granted range is its record here and a badged
/// handle in the kernel, rounded up generously.
pub const COST: Cost = Cost { in_flight: 0, file: 0, state: 512 };

/// The bytes of `blkd`'s budget its clients may use between them; its manifest entry gives it the
/// budget, and [`BlockServer::new`] refuses limits that would not fit.
pub const BUDGET: u64 = 256 * 1024;

/// `blkd`: the disk, the partitions it found, the ranges it has granted, and its admission.
pub struct BlockServer<T: Transport> {
    disk: Disk<T>,
    /// The root ranges, in GPT entry order: index *i* is badge *i* + 1.
    roots: Vec<Range>,
    granted: Minted<Range>,
    admission: Admission,
    /// Where a read's bytes live while the reply borrows them. **`blkd`'s own memory, not the DMA
    /// region**: the device's bytes are copied here once, with a length `blkd` chose, so nothing
    /// the reply is built from can change underneath it (IO-ARCHITECTURE.md, DMA).
    scratch: Vec<u8>,
}

impl<T: Transport> BlockServer<T> {
    /// Serves `roots` on `disk` under `limits`. Refuses limits that cannot seat a fair share, or
    /// whose caps at their ceiling would not fit `budget` bytes (answer 85).
    ///
    /// `random` is one word of the kernel's CSPRNG, where the granted badges start (answer 126):
    /// a `blkd` that cannot get one does not start, because a predictable first badge is a hole
    /// across a restart.
    pub fn new(
        disk: Disk<T>,
        roots: Vec<Range>,
        limits: Limits,
        cost: &Cost,
        budget: u64,
        random: u64,
    ) -> Result<BlockServer<T>, Unsized> {
        if !limits.fits(cost, budget) {
            return Err(Unsized);
        }
        Ok(BlockServer {
            disk,
            roots,
            granted: Minted::new(random),
            admission: Admission::new(limits)?,
            scratch: vec![0; DATA_LEN],
        })
    }

    pub fn disk(&self) -> &Disk<T> { &self.disk }

    /// The partitions `blkd` found, in badge order.
    pub fn roots(&self) -> &[Range] { &self.roots }

    /// Ranges granted and not yet released.
    pub fn granted(&self) -> usize { self.granted.len() }

    pub fn admission(&self) -> &Admission { &self.admission }

    /// Answers one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        let (caller, words, handles) = (request.caller, request.words, request.handles);
        self.granted.answering();
        let mut kernel = Kernel(request.id());
        let outcome = answer_with(self, &caller, &words, &handles, request.lend(), &mut kernel);
        let sent = finish(request, &outcome);
        // A reply that never reached its caller leaves a granted range nobody can name: its id
        // went nowhere, and `release` answers only the holder of an id. Undo it, so a client
        // cannot fill its own bucket by dying mid-grant.
        if let Some(badge) = self.granted.minted_here() {
            if sent.is_err() {
                self.forget_badge(badge);
            }
        }
        sent
    }

    /// Frees the granted range with `badge`, and every range granted under it.
    fn forget_badge(&mut self, badge: u64) {
        let admission = &mut self.admission;
        self.granted.forget(badge, |gone| {
            let (client, share) = gone.charged_to();
            admission.release(client, share, Resource::State);
        });
    }

    /// The range the caller's badge names, or `not_permitted`: a badge that names nothing, one
    /// granted and since released, and one from before a restart are all the same answer.
    fn resolve(&self, caller: &Caller) -> Result<Range, ErrorCode> {
        let range = if caller.badge >= FIRST_GRANTED_BADGE {
            self.granted.get(caller.badge).copied()
        } else {
            // Badge 0 is the receive right and never arrives as a caller's badge; badge *i* + 1
            // is partition *i*.
            caller
                .badge
                .checked_sub(1)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| self.roots.get(i))
                .copied()
        };
        range.ok_or(ErrorCode::NotPermitted)
    }

    /// Answers one decoded request. `buf` is the caller's lend, which the reply is written over,
    /// so the reply may borrow from `self` but never from it.
    fn dispatch<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        buf_len: usize,
        kernel: &mut impl Minter,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let range = self.resolve(caller)?;
        // The label check on every request (CONTAINMENT.md). A range carries no labels in
        // milestone 1 (IO-ARCHITECTURE.md), so a read is allowed to anyone holding the badge and
        // a write only to an unlabelled caller; when volumes' labels reach `blkd` this is the one
        // line that changes. `flush` is a write: it is how a write becomes durable.
        let access = match request {
            Message::Info(_) | Message::Read(_) => Access::Read,
            Message::Write(_) | Message::Flush(_) | Message::Grant(_) | Message::Release(_) => Access::Write,
        };
        check(caller.labels.as_slice(), NO_LABELS, access).map_err(|_| ErrorCode::NotPermitted)?;
        match request {
            Message::Info(Info {}) => Ok(Answer::new(Reply::Info(InfoReply {
                sectors: range.sectors(),
                sector_size: SECTOR_SIZE,
                read_only: u32::from(self.disk.read_only()),
            }))),
            Message::Read(Read { sector, count }) => self.read(range, sector, count, buf_len),
            Message::Write(Write { sector, data }) => {
                self.write(range, sector, data).map(|()| Answer::new(Reply::Write(WriteReply {})))
            }
            Message::Flush(Flush {}) => {
                self.disk.flush().map_err(device_error)?;
                Ok(Answer::new(Reply::Flush(FlushReply {})))
            }
            Message::Grant(Grant { sector, count }) => self.grant(caller, range, sector, count, kernel),
            Message::Release(Release { id }) => {
                let admission = &mut self.admission;
                let mut freed = |gone: Entry<Range>| {
                    let (client, share) = gone.charged_to();
                    admission.release(client, share, Resource::State);
                };
                if id == ALL_GRANTS {
                    self.granted.disconnect_all(caller, freed);
                } else {
                    self.granted.disconnect(caller, id, &mut freed).map_err(|_| ErrorCode::NotPermitted)?;
                }
                Ok(Answer::new(Reply::Release(ReleaseReply {})))
            }
        }
    }

    /// `count` sectors from `sector` of `range`, into [`BlockServer::scratch`], which the reply
    /// then borrows.
    fn read<'s>(
        &'s mut self,
        range: Range,
        sector: u64,
        count: u32,
        buf_len: usize,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let bytes = sectors_to_bytes(count)?;
        // The reply is `u32` length + data (WIRE.md), written into the lend the request came in.
        // Checking it here means a caller that asked for more than it lent is refused before the
        // disk is touched, rather than after, with the answer thrown away.
        if bytes.checked_add(4).is_none_or(|needed| needed > buf_len) {
            return Err(ErrorCode::TooMany);
        }
        let lba = range.absolute(sector, u64::from(count)).ok_or(ErrorCode::OutOfRange)?;
        let out = self.scratch.get_mut(..bytes).ok_or(ErrorCode::Failed)?;
        self.disk.read(lba, out).map_err(device_error)?;
        // Borrowed from `self`, not from the lend: the reply is encoded over the lend.
        Ok(Answer::new(Reply::Read(ReadReply { data: &self.scratch[..bytes] })))
    }

    fn write(&mut self, range: Range, sector: u64, data: &[u8]) -> Result<(), ErrorCode> {
        // Whole sectors only (IO-ARCHITECTURE.md, `blkd`'s contract). A length that is not a
        // whole number of sectors is a request no sender could mean: `malformed`.
        if data.is_empty() || !data.len().is_multiple_of(SECTOR_SIZE as usize) {
            return Err(ErrorCode::Malformed);
        }
        let count = (data.len() / SECTOR_SIZE as usize) as u64;
        if count > u64::from(MAX_SECTORS) {
            return Err(ErrorCode::TooMany);
        }
        let lba = range.absolute(sector, count).ok_or(ErrorCode::OutOfRange)?;
        self.disk.write(lba, data).map_err(device_error)
    }

    /// `grant`: a fresh range inside the caller's own, stamped like the handle the request came
    /// through, with a random id only its requester may `release`.
    fn grant<'s>(
        &'s mut self,
        caller: &Caller,
        range: Range,
        sector: u64,
        count: u64,
        kernel: &mut impl Minter,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        // **Only a root badge may grant**, so grants never chain. Admission keys account 0 by
        // badge (CONTAINMENT.md, because the budget a system caller shares does not travel), and
        // a chain would open a fresh bucket per link until no client could grant at all
        // (QUESTIONS.md 141). Milestone 1 needs no chain: `init` hands out root badges from the
        // manifest, and a range granted from one is a leaf.
        if caller.badge >= FIRST_GRANTED_BADGE {
            return Err(ErrorCode::NotPermitted);
        }
        // Never wider than the caller's own, and never leaving it.
        let narrowed = range.narrow(sector, count).ok_or(ErrorCode::NotPermitted)?;
        // Admission first, so a client at its cap makes the server do no work for it.
        let (client, share) = (AdmitKey::of(caller), self.granted.share(caller));
        self.admission.admit(client, share, Resource::State).map_err(|_| ErrorCode::TooMany)?;
        let made = self
            .granted
            .reserve(caller, share, kernel)
            .and_then(|ticket| self.granted.commit(ticket, narrowed, kernel));
        let (handle, id, _badge) = made.map_err(|e| {
            self.admission.release(client, share, Resource::State);
            match e {
                MintError::TooMany => ErrorCode::TooMany,
                MintError::Failed => ErrorCode::Failed,
            }
        })?;
        let mut handles = Handles::new();
        // Cannot fail: one handle, and a reply may carry `MAX_MSG_HANDLES`.
        let _ = handles.push(handle);
        // The handle was minted for the caller: `blkd` closes its own copy once the reply has
        // copied it, or its table grows by one per grant.
        Ok(Answer { reply: Reply::Grant(GrantReply { id }), handles, close_after_reply: true })
    }
}

/// A range's labels in milestone 1: none (IO-ARCHITECTURE.md). Named, so the one place this
/// changes is obvious when volumes' labels reach `blkd`.
const NO_LABELS: &[u64] = &[];

/// `count` sectors as bytes, refusing more than one request may carry.
fn sectors_to_bytes(count: u32) -> Result<usize, ErrorCode> {
    if count == 0 {
        return Err(ErrorCode::Malformed);
    }
    if count > MAX_SECTORS {
        return Err(ErrorCode::TooMany);
    }
    // `count <= MAX_SECTORS`, so this fits `usize` on both widths.
    Ok(count as usize * SECTOR_SIZE as usize)
}

/// What a client is told about a device failure: `failed`, whatever went wrong, except the two
/// that are about the request rather than the device. None of them says which.
fn device_error(error: DeviceError) -> ErrorCode {
    match error {
        DeviceError::Range => ErrorCode::OutOfRange,
        DeviceError::ReadOnly => ErrorCode::NotPermitted,
        _ => ErrorCode::Failed,
    }
}

/// The protocol, for `redoubt-rt`'s typed dispatch.
pub struct Blkd;

impl Protocol for Blkd {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// The server, the kernel and the lend's length together, which is what the dispatch trait needs:
/// `handle` may borrow only from `self`, and minting needs the kernel.
struct Serving<'a, 'k, T: Transport, K: Minter> {
    server: &'a mut BlockServer<T>,
    kernel: &'k mut K,
    buf_len: usize,
}

impl<T: Transport, K: Minter> TypedServer<Blkd> for Serving<'_, '_, T, K> {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        // No message of this protocol carries a handle, and the codec refuses a request whose
        // handle count is not its layout's, so a request that brought one never reaches here: it
        // is malformed, and the dispatch closes what it brought, so a client cannot grow `blkd`'s
        // handle table (CONTAINMENT.md, the shared server library).
        debug_assert!(handles.is_empty(), "the codec refuses handles this protocol does not name");
        let _ = handles;
        self.server.dispatch(caller, request, self.buf_len, self.kernel)
    }
}

/// Answers one request without making any system call but the ones `kernel` makes: the entry host
/// tests drive, and what [`BlockServer::serve`] calls.
pub fn answer_with<T: Transport>(
    server: &mut BlockServer<T>,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
    kernel: &mut impl Minter,
) -> Outcome {
    let buf_len = buf.len();
    answer::<Blkd, _>(&mut Serving { server, kernel, buf_len }, caller, words, handles, buf)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
