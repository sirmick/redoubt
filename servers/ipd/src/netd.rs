//! The link's calls to `netd` (servers/netd.md, "Serving `ipd`"): `info` for the MAC, and
//! `transmit`, one frame per call in a one-page lend `ipd` keeps and reuses.
//!
//! A transmit waits at most [`TRANSMIT_TIMEOUT_US`]: `busy`, a length `netd` refused, or no
//! answer in time drops the frame, as a full wire does. `failed`, a refusal of `ipd` itself, or
//! `netd` gone puts the link down ([`LinkFault::Down`]) until `info` answers again.

use redoubt_rt::abi::Error;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::wire::proto::netif::{ErrorCode, Info, InfoReply, Message, Reply, Transmit};

use crate::link::{LinkFault, Netif};

/// How long a transmit may take before its frame is dropped (µs).
pub const TRANSMIT_TIMEOUT_US: u64 = 100_000;
/// How long `info` may take (µs).
pub const INFO_TIMEOUT_US: u64 = 100_000;

/// `netd`, through the handle `init` gave `ipd` (its client badge).
pub struct NetdLink {
    netd: Endpoint,
    page: Option<Buffer>,
}

impl NetdLink {
    pub fn new(netd: Endpoint) -> NetdLink { NetdLink { netd, page: None } }

    /// Asks `netd` for the MAC. `Down` if it answers anything but a unicast MAC.
    pub fn info(&mut self) -> Result<[u8; 6], LinkFault> {
        let words = Message::Info(Info {}).encode(&mut []).map_err(|_| LinkFault::Down)?;
        let outcome = self.netd.call(&words, &[], None, INFO_TIMEOUT_US);
        let (reply, _) = outcome.into_result().map_err(|_| LinkFault::Down)?;
        match Reply::decode(1, &reply.words, &[], 0) {
            Ok(Ok(Reply::Info(InfoReply { mac, .. }))) => {
                let bytes = mac.to_le_bytes();
                let octets = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]];
                // `netd` refuses a MAC that is not unicast; one that says otherwise is not believed.
                let unicast = octets != [0; 6] && octets[0] & 1 == 0 && bytes[6] == 0 && bytes[7] == 0;
                if unicast { Ok(octets) } else { Err(LinkFault::Down) }
            }
            _ => Err(LinkFault::Down),
        }
    }
}

impl Netif for NetdLink {
    fn transmit(&mut self, frame: &[u8]) -> Result<(), LinkFault> {
        let mut page = match self.page.take() {
            Some(page) => page,
            None => Buffer::new(1).map_err(|_| LinkFault::Dropped)?,
        };
        let Ok(words) = Message::Transmit(Transmit { frame }).encode(&mut page) else {
            self.page = Some(page);
            return Err(LinkFault::Dropped);
        };
        let mut outcome = self.netd.call(&words, &[], Some(page), TRANSMIT_TIMEOUT_US);
        // The lend comes back on every completion but abandonment; it is kept for the next.
        self.page = outcome.buffer.take();
        match (outcome.status, outcome.reply.take()) {
            (Ok(()), Some(reply)) => match Reply::decode(2, &reply.words, &[], 0) {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(ErrorCode::Busy | ErrorCode::TooMany)) => Err(LinkFault::Dropped),
                _ => Err(LinkFault::Down),
            },
            (Err(Error::Timeout), _) => Err(LinkFault::Dropped),
            _ => Err(LinkFault::Down),
        }
    }
}
