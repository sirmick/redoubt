//! `verityd`'s server: `blkd`'s own protocol (libs/wire/tables/blkd.md, unchanged), on one
//! volume, to the badge `init` minted for the volume's `littlefsd` (docs/servers/verityd.md,
//! "Messages").
//!
//! - `info` gives the volume's data blocks in sectors, read-only, whether or not the start check passed (a
//!   signed volume refused before its N is known gives the range's blocks before its root block): an
//!   `littlefsd` that cannot size its range exits and would be restarted, while one whose mount fails serves
//!   the volume as corrupt and stays up (servers/littlefsd.md R49).
//! - `read` works in whole blocks: each block the request touches is checked through the tree
//!   ([`crate::volume`]) before any of its sectors is copied out. A block that does not check, or any read of
//!   a volume refused at start, is `failed`, as `blkd` answers a device error.
//! - `write` is `not_permitted`, as `blkd` answers one on a read-only disk; `flush` answers at once, since
//!   nothing was written.
//! - The label check (servers/serving.md R25) runs first on every request, against the volume's set, as
//!   `blkd`'s does: `info` and `read` are reads, `write` and `flush` writes.

use alloc::vec::Vec;
use core::fmt;

use redoubt_rt::abi::{Error, Handle, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Request, Words};
use redoubt_rt::server::typed::{Answer, Outcome, Protocol, TypedServer, answer, finish};
use redoubt_rt::server::{Access, check};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    ErrorCode, FlushReply, Info, InfoReply, Message, Read, ReadReply, Reply,
};
use redoubt_verity::SECTORS_PER_BLOCK;

use crate::volume::{Bad, Refusal, Volume};
use crate::{Mode, Range, SECTOR};

/// The most sectors one `read` may ask for, as at `blkd` (servers/blkd.md, "Messages").
pub const MAX_SECTORS: u32 = 64;
/// The one badge `verityd` serves: the volume's, which `init` mints for its `littlefsd`.
pub const BADGE: u64 = 1;

/// A line for `verityd`'s console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// The start check failed.
    Refused(Refusal),
    /// A read did not check.
    Bad(Bad),
    /// Test-only (`boot-stats`): the reads served so far and the volume's counts, at each power
    /// of two of reads from 2^7.
    #[cfg(feature = "boot-stats")]
    Stats(u64, crate::volume::Counts),
}

impl fmt::Display for Said {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Said::Refused(why) => write!(f, "verityd: the volume is refused: {why}"),
            Said::Bad(bad) => write!(f, "verityd: {bad}"),
            #[cfg(feature = "boot-stats")]
            Said::Stats(served, c) => write!(
                f,
                "verityd: boot-stats: reads {served}, data blocks asked {}, held {}, checked {}, level-1 hits {}, \
                 blkd reads {}",
                c.requests, c.held, c.checked, c.hits, c.reads
            ),
        }
    }
}

/// `verityd`: the volume, or why it was refused, and what a reply is built from.
pub struct Verityd<R> {
    volume: Result<Volume<R>, Refusal>,
    /// The volume's data blocks, in sectors: what `info` says either way.
    sectors: u64,
    labels: Vec<u64>,
    /// Where a read's sectors are gathered while the reply borrows them: `verityd`'s own memory,
    /// never the lend the reply is written over.
    scratch: Vec<u8>,
    /// The line to say after this request, and the last bad block said, so a client reading one
    /// bad block again and again gets one line.
    said: Option<Said>,
    last_bad: Option<Bad>,
    /// Reads served (`boot-stats`).
    #[cfg(feature = "boot-stats")]
    served: u64,
}

impl<R: Range> Verityd<R> {
    /// Checks `range` against `mode` ([`Volume::open`]), and serves it under `labels`.
    pub fn new(range: R, mode: &Mode, labels: Vec<u64>) -> Verityd<R> {
        let mut scratch = Vec::new();
        let bytes = MAX_SECTORS as usize * SECTOR as usize;
        let (volume, sectors) = match (scratch.try_reserve_exact(bytes), mode) {
            (Ok(()), _) => Volume::open(range, mode),
            (Err(_), Mode::Pinned { geometry, .. }) => {
                (Err(Refusal::NoMemory), geometry.data_blocks() * SECTORS_PER_BLOCK)
            }
            (Err(_), Mode::Signed { .. }) => (Err(Refusal::NoMemory), 0),
        };
        scratch.resize(scratch.capacity().min(bytes), 0);
        let said = volume.as_ref().err().map(|why| Said::Refused(*why));
        Verityd {
            volume,
            sectors,
            labels,
            scratch,
            said,
            last_bad: None,
            #[cfg(feature = "boot-stats")]
            served: 0,
        }
    }

    /// Why the start check failed, if it did.
    pub fn refused(&self) -> Option<Refusal> { self.volume.as_ref().err().copied() }

    /// The volume's counts, if it passed the start check.
    #[cfg(test)]
    pub fn counts(&self) -> Option<crate::volume::Counts> { self.volume.as_ref().ok().map(Volume::counts) }

    /// The line to say on the console, once.
    pub fn take_line(&mut self) -> Option<Said> { self.said.take() }

    /// Answers one call and replies to it, handing `say` the line the answer leaves first: a
    /// block's refusal is on the console before the client hears of it.
    pub fn serve(&mut self, mut request: Request, say: impl FnOnce(Said)) -> Result<(), Error> {
        let (caller, words, handles) = (request.caller, request.words, request.handles);
        let outcome = answer_with(self, &caller, &words, &handles, request.lend());
        if let Some(line) = self.said.take() {
            say(line);
        }
        finish(request, &outcome).map(|_| ())
    }

    fn dispatch<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        buf_len: usize,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        if caller.badge != BADGE {
            return Err(ErrorCode::NotPermitted);
        }
        let access = match request {
            Message::Info(_) | Message::Read(_) => Access::Read,
            Message::Write(_) | Message::Flush(_) => Access::Write,
        };
        check(caller.labels.as_slice(), &self.labels, access).map_err(|_| ErrorCode::NotPermitted)?;
        match request {
            Message::Info(Info {}) => Ok(Answer::new(Reply::Info(InfoReply {
                sectors: self.sectors,
                sector_size: SECTOR,
                read_only: 1,
            }))),
            Message::Read(Read { sector, count }) => self.read(sector, count, buf_len),
            Message::Write(_) => Err(ErrorCode::NotPermitted),
            Message::Flush(_) => Ok(Answer::new(Reply::Flush(FlushReply {}))),
        }
    }

    /// `count` sectors from `sector`, each block they touch checked, gathered in
    /// [`Verityd::scratch`], which the reply then borrows.
    fn read<'s>(
        &'s mut self,
        sector: u64,
        count: u32,
        buf_len: usize,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        if count == 0 {
            return Err(ErrorCode::Malformed);
        }
        if count > MAX_SECTORS {
            return Err(ErrorCode::TooMany);
        }
        let bytes = count as usize * SECTOR as usize;
        // The reply is a `u32` length and the data, written into the lend: refused before any
        // block is read if it would not fit.
        if bytes + 4 > buf_len || bytes > self.scratch.len() {
            return Err(ErrorCode::TooMany);
        }
        let end = sector.checked_add(u64::from(count)).filter(|end| *end <= self.sectors);
        let end = end.ok_or(ErrorCode::OutOfRange)?;
        let volume = self.volume.as_mut().map_err(|_| ErrorCode::Failed)?;
        let (mut at, mut s) = (0, sector);
        while s < end {
            let b = s / SECTORS_PER_BLOCK;
            let off = (s % SECTORS_PER_BLOCK) as usize * SECTOR as usize;
            let n = (end.min((b + 1) * SECTORS_PER_BLOCK) - s) as usize * SECTOR as usize;
            match volume.block(b) {
                Ok(block) => self.scratch[at..at + n].copy_from_slice(&block[off..off + n]),
                Err(bad) => {
                    if self.last_bad != Some(bad) {
                        self.said = Some(Said::Bad(bad));
                        self.last_bad = Some(bad);
                    }
                    return Err(ErrorCode::Failed);
                }
            }
            at += n;
            s += (n / SECTOR as usize) as u64;
        }
        #[cfg(feature = "boot-stats")]
        {
            self.served += 1;
            if self.served >= 1 << 7 && self.served.is_power_of_two() {
                self.said = Some(Said::Stats(self.served, volume.counts()));
            }
        }
        Ok(Answer::new(Reply::Read(ReadReply { data: &self.scratch[..bytes] })))
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

/// The server and the length of the caller's lend: a `read`'s reply must fit the lend, and
/// `handle` may borrow only from `self`.
struct Serving<'a, R> {
    server: &'a mut Verityd<R>,
    buf_len: usize,
}

impl<R: Range> TypedServer<Blkd> for Serving<'_, R> {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        _handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        // No message of the protocol carries a handle: the codec refuses one that brought any.
        self.server.dispatch(caller, request, self.buf_len)
    }
}

/// Answers one request without making any system call: the entry host tests drive, and what
/// [`Verityd::serve`] calls.
pub fn answer_with<R: Range>(
    server: &mut Verityd<R>,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
) -> Outcome {
    let buf_len = buf.len();
    answer::<Blkd, _>(&mut Serving { server, buf_len }, caller, words, handles, buf)
}
