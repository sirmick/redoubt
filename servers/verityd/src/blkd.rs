//! The range as `blkd` serves it: `info` and `read` on the `volume` badge
//! (libs/wire/tables/blkd.md). A reply of the wrong shape or length is a [`Fault`], which the
//! volume sees as a block it could not read.

use redoubt_rt::abi::FOREVER;
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::wire::proto::blkd::{Info, Message, Read, Reply};

use crate::{Fault, Range, SECTOR, Size};

/// Pages lent to each call: a block of data and the message around it, as `fsd` lends.
const LEND_PAGES: usize = 2;

/// `verityd`'s range at `blkd`.
pub struct Blkd {
    endpoint: Endpoint,
    lend: Lend,
}

impl Blkd {
    pub fn new(endpoint: Endpoint) -> Result<Blkd, Fault> {
        Ok(Blkd { endpoint, lend: Lend::new(LEND_PAGES).map_err(|_| Fault)? })
    }

    /// Calls `blkd` with `message` (opcode `opcode`) and hands `read` its reply.
    fn call<T>(
        &mut self,
        opcode: u32,
        message: &Message<'_>,
        read: impl FnOnce(Reply<'_>) -> Option<T>,
    ) -> Result<T, Fault> {
        let words = message.encode(self.lend.pages().map_err(|_| Fault)?).map_err(|_| Fault)?;
        let (reply, _) =
            self.lend.call(&self.endpoint, &words, &[], FOREVER).into_result().map_err(|_| Fault)?;
        // `blkd` sends no handles; any that came are closed, and the reply is not believed.
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
    fn info(&mut self) -> Result<Size, Fault> {
        self.call(1, &Message::Info(Info {}), |reply| match reply {
            Reply::Info(info) if info.sector_size == SECTOR => Some(Size { sectors: info.sectors }),
            _ => None,
        })
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        let count = u32::try_from(out.len() / SECTOR as usize).map_err(|_| Fault)?;
        self.call(2, &Message::Read(Read { sector, count }), |reply| match reply {
            Reply::Read(r) if r.data.len() == out.len() => {
                out.copy_from_slice(r.data);
                Some(())
            }
            _ => None,
        })
    }
}
