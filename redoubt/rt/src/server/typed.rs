//! Typed-message dispatch (WIRE.md) over `redoubt-wire`'s generated codecs: decode the request,
//! hand it to the server, and reply with the encoded reply or an error status.
//!
//! The generated modules share a shape (`Message::decode`, `Reply::encode`, `ErrorCode::encode`)
//! but no trait, so a server names its protocol by implementing [`Protocol`], a few lines
//! (the tests show `example`'s).

use redoubt_sys::{Error, Handle, Handles};
use redoubt_wire::Error as WireError;

use crate::ipc::{Caller, Delivery, Request, Words};

/// A generated protocol: its request, reply and error types and their codecs.
pub trait Protocol {
    /// `proto::NAME::Message`.
    type Request<'a>;
    /// `proto::NAME::Reply`.
    type Reply<'a>;
    /// `proto::NAME::ErrorCode`.
    type Error: Copy;

    /// `Message::decode`.
    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Self::Request<'a>, WireError>;
    /// `Reply::encode`.
    fn encode_reply(reply: &Self::Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError>;
    /// `ErrorCode::encode`.
    fn error_words(error: Self::Error) -> Words;
}

/// A server of protocol `P`.
pub trait TypedServer<P: Protocol> {
    /// Answers one request. `handles` came with it, in its table's slots (the codec checked
    /// their number); the server owns them from here, whatever it returns. The reply may
    /// borrow from the server, not from the request, whose buffer the reply is written over.
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: P::Request<'_>,
        handles: &[Handle],
    ) -> Result<(P::Reply<'s>, Handles), P::Error>;

    /// The error that answers a request that does not decode, or whose reply does not fit the
    /// caller's lend. WIRE.md gives no protocol-independent code for this, so each protocol's
    /// error table needs one.
    fn malformed(&self) -> P::Error;
}

/// The reply to a request: the words, the handles, and (for a buffer-shaped reply) the fields
/// written into `buf`. Makes no system call: [`serve_call`] wraps it. The second element is the
/// handles to close: the request's if it did not decode (the server never saw them), or the
/// reply's if the reply did not fit `buf` (they cannot travel without it).
pub fn answer<P: Protocol, S: TypedServer<P>>(
    server: &mut S,
    caller: &Caller,
    words: &Words,
    handles: &Handles,
    buf: &mut [u8],
) -> ((Words, Handles), Handles) {
    let malformed = P::error_words(server.malformed());
    let request = match P::decode(words, buf, handles.as_slice().len()) {
        Ok(request) => request,
        Err(_) => return ((malformed, Handles::new()), *handles),
    };
    match server.handle(caller, request, handles.as_slice()) {
        Ok((reply, reply_handles)) => match P::encode_reply(&reply, buf) {
            Ok(words) => ((words, reply_handles), Handles::new()),
            Err(_) => ((malformed, Handles::new()), reply_handles),
        },
        Err(code) => ((P::error_words(code), Handles::new()), Handles::new()),
    }
}

/// Answers a `call` of protocol `P` and replies.
pub fn serve_call<P: Protocol, S: TypedServer<P>>(server: &mut S, mut request: Request) -> Result<(), Error> {
    let (caller, words, handles) = (request.caller, request.words, request.handles);
    let ((words, reply_handles), to_close) =
        answer::<P, S>(server, &caller, &words, &handles, request.lend());
    for handle in to_close.as_slice() {
        let _ = crate::handle::close(*handle);
    }
    request.reply(&words, reply_handles.as_slice())
}

/// Handles a `send` of protocol `P`: the fields are in the transfer, and there is no reply, so
/// any handles the server put in one are closed with the rest.
pub fn serve_send<P: Protocol, S: TypedServer<P>>(server: &mut S, delivery: Delivery) {
    let Delivery { caller, words, handles, mut transfer } = delivery;
    let buf: &mut [u8] = transfer.as_deref_mut().unwrap_or(&mut []);
    let ((_, reply_handles), to_close) = answer::<P, S>(server, &caller, &words, &handles, buf);
    for handle in reply_handles.as_slice().iter().chain(to_close.as_slice()) {
        let _ = crate::handle::close(*handle);
    }
}

#[cfg(test)]
#[path = "typed_tests.rs"]
mod tests;
