//! The range as `blkd` serves it: `info`, `read`, `write` and `flush` on the `volume` badge
//! (libs/wire/tables/blkd.md). Whatever `blkd` answers is checked before it is used: a reply of
//! the wrong shape or length is a [`Fault`], which littlefs sees as an I/O error.

use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::MALFORMED;
#[cfg(feature = "one-volume-probe")]
use redoubt_rt::wire::proto::blkd::ErrorCode;
use redoubt_rt::wire::proto::blkd::{Flush, Info, Message, Read, Reply, Write};

use crate::volume::{Fault, Geometry, Range, SECTOR};

/// Pages lent to each call: a block of data and the message around it.
const LEND_PAGES: usize = 2;

/// `fsd`'s range at `blkd`.
pub struct Blkd {
    endpoint: Endpoint,
    lend: Lend,
}

impl Blkd {
    pub fn new(endpoint: Endpoint) -> Result<Blkd, Fault> {
        Ok(Blkd { endpoint, lend: Lend::new(LEND_PAGES).map_err(|_| Fault)? })
    }

    /// Calls `blkd` with `message` (opcode `opcode`) and hands `read` its reply. An inline
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
        // `blkd` sends no handles; any that came are closed, and the reply is not believed.
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
}

#[cfg(feature = "one-volume-probe")]
impl Blkd {
    /// Test-only, for the bench's `fsd-one-volume` (feature `one-volume-probe`): `blkd`'s answer
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

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        let count = u32::try_from(out.len() / SECTOR as usize).map_err(|_| Fault)?;
        self.call(2, &Message::Read(Read { sector, count }), false, |reply| match reply {
            Reply::Read(r) if r.data.len() == out.len() => {
                out.copy_from_slice(r.data);
                Some(())
            }
            _ => None,
        })
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), Fault> {
        self.call(3, &Message::Write(Write { sector, data }), false, |reply| {
            matches!(reply, Reply::Write(_)).then_some(())
        })
    }

    fn flush(&mut self) -> Result<(), Fault> {
        self.call(4, &Message::Flush(Flush {}), true, |reply| matches!(reply, Reply::Flush(_)).then_some(()))
    }
}
