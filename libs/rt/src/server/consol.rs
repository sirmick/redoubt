//! The `consol` protocol's server side (servers/consoled.md, "The `consol` protocol"), shared by
//! every server of a `/dev/cons` on the same endpoint as its 9P: `size` is answered at once with
//! the console's size; `resize` waits ([`super::typed::TypedServer::waits`]), is parked by the
//! server beside its waiting reads, and is answered with [`reply_resize`] when the size changes.
//! The server counts its own changes and keeps, for each parked `resize`, the count it waited from.
//! `ended` is a `send`, decoded by the server itself; as a call it is malformed.

use redoubt_sys::{Error, Handle, ReplyOutcome};
use redoubt_wire::Error as WireError;
use redoubt_wire::proto::consol::{ErrorCode, Message, Reply, ResizeReply, SizeReply};

use super::typed::{self, Answer, Protocol, TypedServer};
use crate::ipc::{Caller, Request, Words};

/// The protocol, in the server's direction.
pub struct Consol;

impl Protocol for Consol {
    type Error = ErrorCode;
    type Reply<'a> = Reply;
    type Request<'a> = Message;

    fn decode(words: &Words, buf: &[u8], handles: usize) -> Result<Message, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// Whether `words` are a `consol` call this module answers: `size` or `resize`.
pub fn asks(words: &Words) -> bool { matches!(words[0], 16 | 17) }

/// A console's size now, `(cols, rows)`: the server of one request.
pub struct Now(pub (u16, u16));

impl TypedServer<Consol> for Now {
    fn handle<'s>(
        &'s mut self,
        _: &Caller,
        request: Message,
        _: &[Handle],
    ) -> Result<Answer<Reply>, ErrorCode> {
        let (cols, rows) = self.0;
        match request {
            Message::Size(_) => Ok(Answer::new(Reply::Size(SizeReply { cols, rows }))),
            // Only when the server answers it as it comes, which a server that parks never does.
            Message::Resize(_) => Ok(Answer::new(Reply::Resize(ResizeReply { cols, rows }))),
            Message::Ended(_) => Err(ErrorCode::Malformed),
        }
    }

    fn waits(&mut self, _: &Caller, request: &Message) -> bool { matches!(request, Message::Resize(_)) }
}

/// Answers `request`, a `size` or `resize` call, for a console of `size`: `size` at once; `resize`
/// handed back to be parked.
pub fn serve(size: (u16, u16), request: Request) -> Result<Option<Request>, Error> {
    typed::serve_parking::<Consol, _>(&mut Now(size), request)
}

/// Answers a parked `resize` with the console's new `size`.
pub fn reply_resize(request: Request, (cols, rows): (u16, u16)) -> Result<ReplyOutcome, Error> {
    typed::reply::<Consol>(request, &Reply::Resize(ResizeReply { cols, rows }))
}
