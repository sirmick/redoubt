//! The range as `blkd` serves it, and a `verityd` the same way: `info` and `read` on the `volume`
//! badge (libs/wire/tables/blkd.md). Whatever comes back is checked before it is used: a reply of
//! the wrong shape or length is a [`Fault`].

use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::wire::proto::blkd::{Info, Message, Read, Reply};

use crate::server::{Fault, Range};

/// `blkd`'s sector.
pub const SECTOR: u64 = 512;
/// The most sectors one `read` asks for: `blkd`'s and `verityd`'s limit, 32 KiB.
pub const MAX_SECTORS: u64 = 64;
/// Pages lent to each call: the most sectors one read asks for, and the reply's length word.
const LEND_PAGES: usize = 9;

/// `erofsd`'s range.
pub struct Blkd {
    endpoint: Endpoint,
    lend: Lend,
}

impl Blkd {
    pub fn new(endpoint: Endpoint) -> Result<Blkd, Fault> {
        Ok(Blkd { endpoint, lend: Lend::new(LEND_PAGES).map_err(|_| Fault)? })
    }

    /// Calls the range with `message` (opcode `opcode`) and hands `read` its reply.
    fn call<T>(
        &mut self,
        opcode: u32,
        message: &Message<'_>,
        read: impl FnOnce(Reply<'_>) -> Option<T>,
    ) -> Result<T, Fault> {
        let words = message.encode(self.lend.pages().map_err(|_| Fault)?).map_err(|_| Fault)?;
        let (reply, _) =
            self.lend.call(&self.endpoint, &words, &[], FOREVER).into_result().map_err(|_| Fault)?;
        // The range sends no handles; any that came are closed, and the reply is not believed.
        let mut handles = false;
        for handle in reply.handles.as_slice().iter().flatten() {
            let _ = redoubt_rt::handle::close(*handle);
            handles = true;
        }
        if handles || reply.words == MALFORMED {
            return Err(Fault);
        }
        match Reply::decode(opcode, &reply.words, self.lend.bytes(), 0) {
            Ok(Ok(reply)) => read(reply).ok_or(Fault),
            _ => Err(Fault),
        }
    }
}

impl Range for Blkd {
    fn sectors(&mut self) -> Result<u64, Fault> {
        self.call(1, &Message::Info(Info {}), |reply| match reply {
            Reply::Info(info) if u64::from(info.sector_size) == SECTOR => Some(info.sectors),
            _ => None,
        })
    }

    /// One `read` per [`MAX_SECTORS`] the bytes touch: a 9P read of a file's blocks is one call.
    fn read(&mut self, at: u64, out: &mut [u8]) -> Result<(), Fault> {
        let mut done = 0;
        while done < out.len() {
            let from = at + done as u64;
            let (sector, skip) = (from / SECTOR, (from % SECTOR) as usize);
            let n = (out.len() - done).min((MAX_SECTORS * SECTOR) as usize - skip);
            let count = (skip + n).div_ceil(SECTOR as usize) as u32;
            let piece = &mut out[done..done + n];
            self.call(2, &Message::Read(Read { sector, count }), |reply| match reply {
                Reply::Read(r) if r.data.len() == count as usize * SECTOR as usize => {
                    piece.copy_from_slice(&r.data[skip..skip + n]);
                    Some(())
                }
                _ => None,
            })?;
            done += n;
        }
        Ok(())
    }
}
