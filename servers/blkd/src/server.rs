//! The `blkd` server: the typed protocol of servers/blkd.md, over `redoubt-rt`'s shared server
//! library.
//!
//! **A typed protocol, not 9P.** `blkd` serves four fixed operations on ranges of sectors and has
//! no namespace to walk (servers/blkd.md, "Messages").
//!
//! **A range is a badge.** The root badge of GPT entry *i* is *i* + 1, counting every entry of the
//! array, used or not, as `keyd`'s badge is the index of its key argument (servers/keyd.md). So
//! `init` mints each volume's range from the manifest's entry number without asking `blkd`
//! anything, a restarted `blkd` gives the same badges the same meaning from the same disk while
//! holding no state across the restart, and a badge naming an **unused** entry is refused exactly
//! as one past the end is. Counting positions among the used entries instead would renumber every
//! volume after a gap, and `gdisk` leaves gaps routinely.
//!
//! **`blkd` mints nothing and remembers nothing.** There is no `grant` and no `release`: every
//! range comes from the boot manifest through a badge `init` minted. So a client can make `blkd`
//! hold nothing that outlives its request — no parked call, no connection, no minted capability —
//! and there is no admission to keep. What bounds a flood of reads is the kernel's fair waiting
//! per (account, label set) (R2) and [`crate::virtio::MAX_SECTORS`], which bounds one request's
//! work by a number stated in the note rather than by the caller's lend.
//!
//! **Every request** resolves the caller's badge to one range, applies [`check`] to it, and only
//! then touches the device. A badge that names no range and a caller whose labels fail the check
//! get the same `not_permitted`, which says no more than that.

use alloc::vec;
use alloc::vec::Vec;

use redoubt_rt::abi::{Error, Handle, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Request, Words};
use redoubt_rt::server::typed::{Answer, Outcome, Protocol, TypedServer, answer, finish};
use redoubt_rt::server::{Access, check};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    ErrorCode, Flush, FlushReply, Info, InfoReply, Message, Read, ReadReply, Reply, Write, WriteReply,
};

use crate::disk::Disk;
use crate::range::Range;
use crate::transport::Transport;
use crate::virtio::{DATA_LEN, DeviceError, MAX_SECTORS, SECTOR_SIZE};

/// `blkd`: the disk, the ranges its badges name, and the buffer a reply is built from.
pub struct BlockServer<T: Transport> {
    disk: Disk<T>,
    /// The root ranges, **one slot per GPT entry**: slot *i* is badge *i* + 1, and `None` is an
    /// entry the table does not use.
    roots: Vec<Option<Range>>,
    /// Where a read's bytes live while the reply borrows them. **`blkd`'s own memory, not the DMA
    /// region**: the device's bytes are copied here once, with a length `blkd` chose, so nothing
    /// the reply is built from can change underneath it (servers/blkd.md, "The DMA region").
    scratch: Vec<u8>,
}

impl<T: Transport> BlockServer<T> {
    /// Serves `roots` on `disk`. `roots` is one slot per GPT entry, in entry order, as
    /// [`crate::read_partitions`] returns it.
    pub fn new(disk: Disk<T>, roots: Vec<Option<Range>>) -> BlockServer<T> {
        BlockServer { disk, roots, scratch: vec![0; DATA_LEN] }
    }

    pub fn disk(&self) -> &Disk<T> { &self.disk }

    /// One slot per GPT entry, in entry order; slot *i* is badge *i* + 1.
    pub fn roots(&self) -> &[Option<Range>] { &self.roots }

    /// Answers one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        let (caller, words, handles) = (request.caller, request.words, request.handles);
        let outcome = answer_with(self, &caller, &words, &handles, request.lend());
        finish(request, &outcome).map(|_| ())
    }

    /// The range the caller's badge names, or `not_permitted`: a badge past the end of the array
    /// and one naming an entry the table does not use are the same answer, which tells a caller
    /// only that this badge is not good here.
    fn resolve(&self, caller: &Caller) -> Result<Range, ErrorCode> {
        // Badge 0 is the receive right and never arrives as a caller's badge; badge *i* + 1 is
        // GPT entry *i*.
        caller
            .badge
            .checked_sub(1)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| self.roots.get(i))
            .copied()
            .flatten()
            .ok_or(ErrorCode::NotPermitted)
    }

    /// Answers one decoded request. `buf_len` is the caller's lend, which the reply is written
    /// over, so the reply may borrow from `self` but never from it.
    fn dispatch<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        buf_len: usize,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let range = self.resolve(caller)?;
        // The label check on every request (servers/serving.md R25). A range carries no labels
        // in milestone 1, so a read is allowed to anyone holding the badge and a write only to an
        // unlabelled caller; when volumes' labels reach `blkd` this is the one line that
        // changes. `flush` is a write: it is how a write becomes durable.
        let access = match request {
            Message::Info(_) | Message::Read(_) => Access::Read,
            Message::Write(_) | Message::Flush(_) => Access::Write,
        };
        check(caller.labels.as_slice(), NO_LABELS, access).map_err(|_| ErrorCode::NotPermitted)?;
        match request {
            Message::Info(Info {}) => Ok(Answer::new(Reply::Info(InfoReply {
                // The range's sectors, never the disk's: the number a client is told is the
                // number it may address.
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
        // The reply is `u32` length + data (servers/wire.md), written into the lend the request
        // came in. Checking it here means a caller that asked for more than it lent is refused
        // before the disk is touched, rather than after, with the answer thrown away.
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
        // Whole sectors only (servers/blkd.md, "Messages"). A length that is not a
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
}

/// A range's labels in milestone 1: none. Named, so the one place this changes is obvious when
/// volumes' labels reach `blkd`.
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

/// The server and the length of the caller's lend, which is what the dispatch trait needs: a
/// `read`'s reply must fit the lend, and `handle` may borrow only from `self`.
struct Serving<'a, T: Transport> {
    server: &'a mut BlockServer<T>,
    buf_len: usize,
}

impl<T: Transport> TypedServer<Blkd> for Serving<'_, T> {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        // No message of this protocol carries a handle, and the codec refuses a request whose
        // handle count is not its layout's, so a request that brought one never reaches here: it
        // is malformed, and the dispatch closes what it brought, so a client cannot grow `blkd`'s
        // handle table (servers/serving.md, "Authority").
        debug_assert!(handles.is_empty(), "the codec refuses handles this protocol does not name");
        let _ = handles;
        self.server.dispatch(caller, request, self.buf_len)
    }
}

/// Answers one request without making any system call: the entry host tests drive, and what
/// [`BlockServer::serve`] calls.
pub fn answer_with<T: Transport>(
    server: &mut BlockServer<T>,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
) -> Outcome {
    let buf_len = buf.len();
    answer::<Blkd, _>(&mut Serving { server, buf_len }, caller, words, handles, buf)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
