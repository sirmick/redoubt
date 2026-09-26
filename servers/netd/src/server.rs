//! The `netif` protocol (servers/netd.md, "Serving `ipd`"): `info` and `transmit`, for one client.
//!
//! **One client.** `netd`'s one argument names the badge `ipd`'s handle carries; any other badge,
//! and any labelled caller, is `not_permitted`. `netd` mints nothing and parks nothing, so a call
//! holds nothing after its reply.
//!
//! **A device fault never stops `netd`.** A lie or a timeout marks the device broken: it is reset
//! (so it stops touching its rings) and every later request is answered `failed`. `netd` stays up
//! and `init` has nothing to restart, so a lying device cannot make the box reboot-loop.

use redoubt_rt::abi::{Error, Handle, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Request, Words};
use redoubt_rt::server::typed::{Answer, Outcome, Protocol, TypedServer, answer, finish};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::netif::{ErrorCode, Info, InfoReply, Message, Reply, Transmit, TransmitReply};

use crate::transport::Transport;
use crate::txq::{Sent, TxQueue};
use crate::virtio::{self, DeviceError, MTU};

/// The serving thread's half of `netd`: the transmit queue, in its own region, and the device's
/// state as far as this thread knows it.
pub struct NetServer<T: Transport> {
    t: T,
    tx: TxQueue,
    mac: u64,
    client: u64,
    broken: bool,
    /// The badge the receive thread reports a broken device on ([`NetServer::expect_reports_on`]).
    reports: Option<u64>,
}

impl<T: Transport> NetServer<T> {
    /// Serves `client` (a badge below 2^63, [`crate::parse_client`]) on the transmit queue `tx`
    /// in `t`'s region, for a device whose MAC is `mac`.
    pub fn new(t: T, tx: TxQueue, mac: u64, client: u64) -> NetServer<T> {
        NetServer { t, tx, mac, client, broken: false, reports: None }
    }

    /// The receive thread reports a broken device with a `send` on `badge`, drawn at random at or
    /// above [`crate::FIRST_MINTED_BADGE`], so never the client's. A badge below that is refused
    /// (`false`), as it could be one the manifest gave out.
    pub fn expect_reports_on(&mut self, badge: u64) -> bool {
        let minted = badge >= crate::FIRST_MINTED_BADGE;
        if minted {
            self.reports = Some(badge);
        }
        minted
    }

    /// A `send` arrived on `badge` with `words` (its handles are the caller's to close). The one
    /// `send` that means anything is the receive thread's [`crate::BROKEN`] on the reporting
    /// badge: the device is reset and `failed` from then on. Any other is ignored; the client
    /// only calls, and nobody else can make the serving thread do anything.
    pub fn sent(&mut self, badge: u64, words: &Words) {
        if self.reports == Some(badge) && words[0] == crate::BROKEN {
            self.break_device();
        }
    }

    pub fn broken(&self) -> bool { self.broken }

    pub fn tx(&self) -> &TxQueue { &self.tx }

    /// The device has lied (here, or on the receive thread): reset it so it stops touching the
    /// rings, and answer `failed` from now on.
    pub fn break_device(&mut self) {
        if !self.broken {
            self.broken = true;
            let _ = virtio::reset(&self.t);
        }
    }

    /// Answers one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        let (caller, words, handles) = (request.caller, request.words, request.handles);
        let outcome = answer_with(self, &caller, &words, &handles, request.lend());
        finish(request, &outcome).map(|_| ())
    }

    fn dispatch(&mut self, caller: &Caller, request: Message<'_>) -> Result<Answer<Reply>, ErrorCode> {
        // `netd` is not a sink and serves no user; its one client is an unlabelled system
        // server. Anything else is refused before the device is touched.
        if caller.badge != self.client || !caller.labels.as_slice().is_empty() {
            return Err(ErrorCode::NotPermitted);
        }
        if self.broken {
            return Err(ErrorCode::Failed);
        }
        match request {
            Message::Info(Info {}) => Ok(Answer::new(Reply::Info(InfoReply { mac: self.mac, mtu: MTU }))),
            Message::Transmit(Transmit { frame }) => match self.tx.transmit(&self.t, frame) {
                Ok(Sent::Queued) => Ok(Answer::new(Reply::Transmit(TransmitReply {}))),
                Ok(Sent::Busy) => Err(ErrorCode::Busy),
                Ok(Sent::BadLength) => Err(ErrorCode::TooMany),
                Err(error) => Err(self.fail(error)),
            },
        }
    }

    fn fail(&mut self, _error: DeviceError) -> ErrorCode {
        self.break_device();
        ErrorCode::Failed
    }
}

/// The protocol, for `redoubt-rt`'s typed dispatch.
pub struct Netif;

impl Protocol for Netif {
    type Error = ErrorCode;
    type Reply<'a> = Reply;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

impl<T: Transport> TypedServer<Netif> for NetServer<T> {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        handles: &[Handle],
    ) -> Result<Answer<Reply>, ErrorCode> {
        // No message of `netif` carries a handle; the codec refuses one that brings any, and the
        // dispatch closes it (servers/serving.md, "Authority").
        let _ = handles;
        self.dispatch(caller, request)
    }
}

/// Answers one request without making any system call: the entry host tests drive, and what
/// [`NetServer::serve`] calls.
pub fn answer_with<T: Transport>(
    server: &mut NetServer<T>,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
) -> Outcome {
    answer::<Netif, _>(server, caller, words, handles, buf)
}
