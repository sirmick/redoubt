//! The range a volume server holds: `blkd`'s protocol on the `volume` badge (servers/blkd.md,
//! "Messages"; the table is libs/wire/tables/blkd.md), or a `verityd`'s, which serves the same.
//! [`Range`] is what a format needs of it, whole sectors read, written and flushed, and
//! [`Blkd`] the one client of the protocol: what `blkd` answers is checked before it is used, and
//! a reply of the wrong shape or length is a [`Fault`], which a format sees as an I/O error.

use redoubt_rt::abi::{FOREVER, PAGE_SIZE};
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::MALFORMED;
pub use redoubt_rt::wire::blkd::{MAX_SECTORS, SECTOR};
#[cfg(feature = "one-volume-probe")]
use redoubt_rt::wire::proto::blkd::ErrorCode;
use redoubt_rt::wire::proto::blkd::{Flush, Info, Message, Read, Reply, Write};

#[cfg(feature = "test-support")]
mod memory;
#[cfg(feature = "test-support")]
pub use memory::{Disk, Memory};

/// A request to the range failed: it was refused, or the disk did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// What `info` says of a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub sectors: u64,
    /// The range refuses writes: a server then writes nothing at all.
    pub read_only: bool,
}

/// A range as a volume server uses it: its size, and whole sectors read, written and flushed.
/// A range that says nothing of writing refuses it, as `verityd`'s does and a test's in memory
/// may.
pub trait Range {
    /// The range's length in sectors and whether it may be written (`info`).
    fn info(&mut self) -> Result<Geometry, Fault>;
    /// Reads `out.len() / SECTOR` sectors from `sector`, whatever the count: a client splits it
    /// at what one call carries.
    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault>;
    /// Writes whole sectors from `sector`.
    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), Fault> {
        let _ = (sector, data);
        Err(Fault)
    }
    /// Returns once every write so far is durable: `blkd`'s `flush`.
    fn flush(&mut self) -> Result<(), Fault> { Err(Fault) }
    /// `out.len()` bytes from byte `at`, for a format whose structures are not sector-aligned
    /// (`erofsd`'s inodes and inline tails): whole sectors read, the piece copied out. The
    /// default reads the head and tail sectors through a sector of scratch and the whole sectors
    /// between straight into `out`; [`Blkd`] does it in one call per span.
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> Result<(), Fault> { read_at(self, at, out) }
}

/// [`Range::read_at`]'s default.
pub fn read_at<R: Range + ?Sized>(range: &mut R, at: u64, out: &mut [u8]) -> Result<(), Fault> {
    let sector = SECTOR as usize;
    let mut done = 0;
    while done < out.len() {
        let from = at.checked_add(done as u64).ok_or(Fault)?;
        let (s, skip) = (from / u64::from(SECTOR), (from % u64::from(SECTOR)) as usize);
        let left = out.len() - done;
        if skip == 0 && left >= sector {
            let whole = left / sector * sector;
            range.read(s, &mut out[done..done + whole])?;
            done += whole;
        } else {
            let mut one = [0u8; SECTOR as usize];
            range.read(s, &mut one)?;
            let n = left.min(sector - skip);
            out[done..done + n].copy_from_slice(&one[skip..skip + n]);
            done += n;
        }
    }
    Ok(())
}

/// The range at `blkd`, or at a `verityd`.
pub struct Blkd {
    endpoint: Endpoint,
    lend: Lend,
    /// The most sectors one call carries: [`MAX_SECTORS`], or what the lend holds beside the
    /// message if that is less.
    per_call: u32,
}

impl Blkd {
    /// A client lending `pages` to each call: the reply's data and the words around it. Two pages
    /// carry a block of eight sectors; nine carry [`MAX_SECTORS`].
    pub fn new(endpoint: Endpoint, pages: usize) -> Result<Blkd, Fault> {
        let fits = (pages * PAGE_SIZE).saturating_sub(SECTOR as usize) / SECTOR as usize;
        let per_call = u32::try_from(fits).unwrap_or(MAX_SECTORS).clamp(1, MAX_SECTORS);
        Ok(Blkd { endpoint, lend: Lend::new(pages).map_err(|_| Fault)?, per_call })
    }

    /// Calls the range with `message` (opcode `opcode`) and hands `read` its reply. An inline
    /// message (`flush`) travels without the lend, as the wire requires.
    fn call<T>(
        &mut self,
        opcode: u32,
        message: &Message<'_>,
        inline: bool,
        read: impl FnOnce(Reply<'_>) -> Option<T>,
    ) -> Result<T, Fault> {
        let outcome = if inline {
            self.endpoint.call(&message.encode(&mut []).map_err(|_| Fault)?, &[], None, FOREVER)
        } else {
            let words = message.encode(self.lend.pages().map_err(|_| Fault)?).map_err(|_| Fault)?;
            self.lend.call(&self.endpoint, &words, &[], FOREVER)
        };
        let (reply, _) = outcome.into_result().map_err(|_| Fault)?;
        // The range sends no handles; any that came are closed, and the reply is not believed.
        let mut handles = false;
        for handle in reply.handles.as_slice().iter().flatten() {
            let _ = redoubt_rt::handle::close(*handle);
            handles = true;
        }
        if handles || reply.words == MALFORMED {
            return Err(Fault);
        }
        let body = if inline { &[][..] } else { self.lend.bytes() };
        match Reply::decode(opcode, &reply.words, body, 0) {
            Ok(Ok(reply)) => read(reply).ok_or(Fault),
            _ => Err(Fault),
        }
    }

    /// One `read`: `count` sectors from `sector`, of which the bytes `skip..skip + piece.len()`
    /// are wanted.
    fn read_span(&mut self, sector: u64, count: u32, skip: usize, piece: &mut [u8]) -> Result<(), Fault> {
        self.call(2, &Message::Read(Read { sector, count }), false, |reply| match reply {
            Reply::Read(r) if r.data.len() == count as usize * SECTOR as usize => {
                piece.copy_from_slice(&r.data[skip..skip + piece.len()]);
                Some(())
            }
            _ => None,
        })
    }
}

#[cfg(feature = "one-volume-probe")]
impl Blkd {
    /// Test-only, for the bench's one-volume cases (feature `one-volume-probe`): the range's answer
    /// to a one-sector read at `sector`, its error code if it refuses.
    pub fn read_one(&mut self, sector: u64) -> Result<Result<(), ErrorCode>, Fault> {
        let words = Message::Read(Read { sector, count: 1 }).encode(self.lend.pages().map_err(|_| Fault)?);
        let words = words.map_err(|_| Fault)?;
        let (reply, _) =
            self.lend.call(&self.endpoint, &words, &[], FOREVER).into_result().map_err(|_| Fault)?;
        let reply = Reply::decode(2, &reply.words, self.lend.bytes(), reply.handles.as_slice().len());
        reply.map(|r| r.map(|_| ())).map_err(|_| Fault)
    }

    /// The range's badge, for the probe to try minting from.
    pub fn endpoint(&self) -> &Endpoint { &self.endpoint }
}

impl Range for Blkd {
    fn info(&mut self) -> Result<Geometry, Fault> {
        self.call(1, &Message::Info(Info {}), false, |reply| match reply {
            Reply::Info(info) if info.sector_size == SECTOR => {
                Some(Geometry { sectors: info.sectors, read_only: info.read_only != 0 })
            }
            _ => None,
        })
    }

    /// Whole sectors only, one call per [`Blkd::per_call`] of them: a part of a sector is a
    /// [`Fault`], never a short read.
    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        if !out.len().is_multiple_of(SECTOR as usize) {
            return Err(Fault);
        }
        self.read_at(sector.checked_mul(u64::from(SECTOR)).ok_or(Fault)?, out)
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), Fault> {
        self.call(3, &Message::Write(Write { sector, data }), false, |reply| {
            matches!(reply, Reply::Write(_)).then_some(())
        })
    }

    fn flush(&mut self) -> Result<(), Fault> {
        self.call(4, &Message::Flush(Flush {}), true, |reply| matches!(reply, Reply::Flush(_)).then_some(()))
    }

    /// One call per span of [`Blkd::per_call`] sectors the bytes touch: a file's blocks are one.
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> Result<(), Fault> {
        let mut done = 0;
        while done < out.len() {
            let from = at.checked_add(done as u64).ok_or(Fault)?;
            let (sector, skip) = (from / u64::from(SECTOR), (from % u64::from(SECTOR)) as usize);
            let n = (out.len() - done).min(self.per_call as usize * SECTOR as usize - skip);
            let count = (skip + n).div_ceil(SECTOR as usize) as u32;
            self.read_span(sector, count, skip, &mut out[done..done + n])?;
            done += n;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "range_tests.rs"]
mod tests;
