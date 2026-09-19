//! Typed-message dispatch (WIRE.md) over `redoubt-wire`'s generated codecs: decode the request,
//! hand it to the server, and reply with the encoded reply or an error status.
//!
//! The generated modules share a shape (`Message::decode`, `Reply::encode`, `ErrorCode::encode`)
//! but no trait, so a server names its protocol by implementing [`Protocol`], a few lines.
//! Typed messages are served as calls; a protocol's one-way messages (`send`) are few enough that
//! each server decodes them itself.
//!
//! ```
//! use redoubt_rt::abi::{Handle, Handles, ReceivedHandles};
//! use redoubt_rt::ipc::{Caller, Words};
//! use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, answer};
//! use redoubt_rt::wire::Error as WireError;
//! use redoubt_rt::wire::proto::example::{ErrorCode, Message, Reply, Small, SmallReply};
//!
//! /// The protocol, named once.
//! struct Example;
//!
//! impl Protocol for Example {
//!     type Error = ErrorCode;
//!     type Reply<'a> = Reply<'a>;
//!     type Request<'a> = Message<'a>;
//!
//!     fn decode<'a>(
//!         words: &Words,
//!         buf: &'a [u8],
//!         handles: usize,
//!     ) -> Result<Message<'a>, WireError> {
//!         Message::decode(words, buf, handles)
//!     }
//!
//!     fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> {
//!         reply.encode(buf)
//!     }
//!
//!     fn error_words(error: ErrorCode) -> Words { error.encode() }
//! }
//!
//! /// A server that adds.
//! struct Adder;
//!
//! impl TypedServer<Example> for Adder {
//!     fn handle<'s>(
//!         &'s mut self,
//!         _caller: &Caller,
//!         request: Message<'_>,
//!         _handles: &[Handle],
//!     ) -> Result<Answer<Reply<'s>>, ErrorCode> {
//!         match request {
//!             Message::Small(Small { a, b }) => {
//!                 Ok(Answer::new(Reply::Small(SmallReply { c: u32::from(a) + u32::from(b) })))
//!             }
//!             _ => Err(ErrorCode::NotFound),
//!         }
//!     }
//! }
//!
//! // In the receive loop: `Event::Call(request) => serve_call::<Example, _>(&mut adder, request)`.
//! // `answer` is the same without the system calls:
//! let caller = Caller { badge: 1, account: 1, labels: Default::default() };
//! let words = Message::Small(Small { a: 2, b: 40 }).encode(&mut []).unwrap();
//! let outcome =
//!     answer::<Example, _>(&mut Adder, &caller, &words, &ReceivedHandles::new(), &mut []);
//! assert_eq!(
//!     Reply::decode(3, &outcome.words, &[], 0),
//!     Ok(Ok(Reply::Small(SmallReply { c: 42 })))
//! );
//! ```

use redoubt_sys::{Error, Handle, Handles, ReceivedHandles};
use redoubt_wire::Error as WireError;

use crate::ipc::{Caller, Request, Words};

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

/// A successful answer: the reply, the handles that go with it, and what happens to them here.
pub struct Answer<R> {
    pub reply: R,
    /// Copied into the caller with the reply (KERNEL-SPEC.md, Messages).
    pub handles: Handles,
    /// Close `handles` here once the reply is sent: true for handles made for the caller (a
    /// minted connection), false for handles the server keeps using.
    pub close_after_reply: bool,
}

impl<R> Answer<R> {
    /// A reply with no handles.
    pub fn new(reply: R) -> Answer<R> { Answer { reply, handles: Handles::new(), close_after_reply: false } }
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
    ) -> Result<Answer<P::Reply<'s>>, P::Error>;
}

/// What to do with one request: reply with `words` and `send`, then close `close`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub words: Words,
    pub send: Handles,
    /// The request's handles if it did not decode or one was missing (the server never saw
    /// them), the reply's if
    /// the reply did not fit the lend (they cannot travel without it), or the reply's after it
    /// is sent if the server asked for that.
    pub close: Handles,
}

/// The handles, if every slot holds one; otherwise, as the error, the ones that do.
pub(crate) fn present(handles: &ReceivedHandles) -> Result<Handles, Handles> {
    let mut list = Handles::new();
    for handle in handles.as_slice().iter().flatten() {
        // Cannot fail: the two lists have the same capacity.
        let _ = list.push(*handle);
    }
    if list.as_slice().len() == handles.as_slice().len() { Ok(list) } else { Err(list) }
}

/// The outcome of a request; a buffer-shaped reply's fields are written into `buf`. Makes no
/// system call: [`serve_call`] wraps it.
pub fn answer<P: Protocol, S: TypedServer<P>>(
    server: &mut S,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
) -> Outcome {
    let none = Handles::new();
    // A request that does not decode, whose reply does not fit the lend, or missing a handle
    // (revoked on its way: R10) is malformed: status 1 in every protocol (answer 42; WIRE.md).
    let malformed = super::MALFORMED;
    let handles = match present(handles) {
        Ok(handles) => handles,
        Err(present) => return Outcome { words: malformed, send: none, close: present },
    };
    let Ok(request) = P::decode(words, buf, handles.as_slice().len()) else {
        return Outcome { words: malformed, send: none, close: handles };
    };
    match server.handle(caller, request, handles.as_slice()) {
        Ok(answer) => match P::encode_reply(&answer.reply, buf) {
            Ok(words) => {
                let close = if answer.close_after_reply { answer.handles } else { none };
                Outcome { words, send: answer.handles, close }
            }
            Err(_) => Outcome { words: malformed, send: none, close: answer.handles },
        },
        Err(code) => Outcome { words: P::error_words(code), send: none, close: none },
    }
}

/// Answers a `call` of protocol `P`, replies, and closes what the outcome says to close.
pub fn serve_call<P: Protocol, S: TypedServer<P>>(server: &mut S, mut request: Request) -> Result<(), Error> {
    let (caller, words, handles) = (request.caller, request.words, request.handles);
    let outcome = answer::<P, S>(server, &caller, &words, &handles, request.lend());
    let sent = request.reply(&outcome.words, outcome.send.as_slice()).map_err(|(e, _)| e);
    for handle in outcome.close.as_slice() {
        let _ = crate::handle::close(*handle);
    }
    sent
}

#[cfg(test)]
#[path = "typed_tests.rs"]
mod tests;
