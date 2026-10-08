//! One call for any typed protocol, over the module the generator wrote from its table
//! (servers/wire.md, "Wire tables and the generator"): for every server but `littlefsd` this and the
//! generated module are the binding.
//!
//! The generated codec does all the encoding and decoding, and the protocol's layout says whether
//! the call lends pages at all: an inline message travels in its words alone, since one sent with
//! a lend is malformed.

use redoubt_rt::abi::{FOREVER, Handle, MAX_MSG_HANDLES};
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::wire::typed::Protocol;

use crate::error::Error;

/// The handles a successful reply brought, by slot. [`Received::take`] moves one out to its new
/// owner; every handle left is closed when this is dropped, so none leaks whatever `read` does.
pub struct Received {
    slots: [Option<Handle>; MAX_MSG_HANDLES],
    len: usize,
}

impl Received {
    /// The handle in `slot`, now the caller's; `None` if the slot was empty (revoked on its way)
    /// or taken already.
    pub fn take(&mut self, slot: usize) -> Option<Handle> { self.slots.get_mut(slot).and_then(Option::take) }

    pub fn len(&self) -> usize { self.len }

    pub fn is_empty(&self) -> bool { self.len == 0 }
}

impl Drop for Received {
    fn drop(&mut self) {
        for handle in self.slots.iter().flatten() {
            let _ = redoubt_rt::handle::close(*handle);
        }
    }
}

/// Calls `to` with `message` and `handles`, and hands `read` the decoded reply and the handles
/// it brought. A buffer-shaped message is encoded into `lend` and the reply read from it; an
/// inline one lends nothing. The server's error code is [`Error::Server`]; an error reply, and a
/// reply that does not decode, keep none of the handles they brought.
pub fn call<P: Protocol, T>(
    to: &Endpoint,
    lend: &mut Lend,
    message: &P::Message<'_>,
    handles: &[Handle],
    read: impl FnOnce(P::Reply<'_>, &mut Received) -> T,
) -> Result<T, Error> {
    call_within::<P, T>(to, lend, message, handles, FOREVER, read)
}

/// [`call`], waiting at most `timeout` µs for the reply ([`FOREVER`] never gives up): a call no
/// server takes in time is the kernel's `Timeout`.
pub fn call_within<P: Protocol, T>(
    to: &Endpoint,
    lend: &mut Lend,
    message: &P::Message<'_>,
    handles: &[Handle],
    timeout: u64,
    read: impl FnOnce(P::Reply<'_>, &mut Received) -> T,
) -> Result<T, Error> {
    let layout = P::layout(message)?;
    let outcome = if layout.inline {
        to.call(&P::encode(message, &mut [])?, handles, None, timeout)
    } else {
        let words = P::encode(message, lend.pages()?)?;
        lend.call(to, &words, handles, timeout)
    };
    // A failed call's reply, if one came, is dropped here with its handles (R13).
    let (reply, _) = outcome.into_result()?;
    let mut received = Received { slots: [None; MAX_MSG_HANDLES], len: reply.handles.as_slice().len() };
    received.slots[..received.len].copy_from_slice(reply.handles.as_slice());
    let body = if layout.inline { &[][..] } else { lend.bytes() };
    match P::decode_reply(layout.opcode, &reply.words, body, received.len)? {
        Ok(reply) => Ok(read(reply, &mut received)),
        Err(code) => Err(Error::Server(P::code(code))),
    }
}
