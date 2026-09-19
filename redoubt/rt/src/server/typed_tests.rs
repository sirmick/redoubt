//! Typed dispatch over the `example` protocol's generated codec, through `answer` (no system
//! calls).

use alloc::vec;
use alloc::vec::Vec;

use redoubt_sys::Labels;
use redoubt_wire::proto::example::{
    Blob, BlobReply, ErrorCode, Grant, GrantReply, Message, Named, NamedReply, Read, ReadReply, Reply, Small,
    SmallReply,
};

use super::*;

/// The `example` protocol, named the way a server names its own.
struct Example;

impl Protocol for Example {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// Stores blobs, serves them back by `read`, and hands back the second handle of a `grant`.
#[derive(Default)]
struct Store {
    stored: Vec<u8>,
    kept: Vec<Handle>,
}

impl TypedServer<Example> for Store {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        match request {
            Message::Small(Small { a, b }) => {
                Ok(Answer::new(Reply::Small(SmallReply { c: u32::from(a) + u32::from(b) })))
            }
            Message::Named(Named { name: "deny", .. }) => Err(ErrorCode::Denied),
            Message::Named(Named { id, .. }) => {
                Ok(Answer::new(Reply::Named(NamedReply { id: id ^ caller.account as u32 })))
            }
            Message::Blob(Blob { data, .. }) => {
                self.stored = data.to_vec();
                Ok(Answer::new(Reply::Blob(BlobReply {})))
            }
            Message::Read(Read { offset, count }) => {
                let start = (offset as usize).min(self.stored.len());
                let end = start.saturating_add(count as usize).min(self.stored.len());
                Ok(Answer::new(Reply::Read(ReadReply { data: &self.stored[start..end] })))
            }
            Message::Grant(Grant { .. }) => {
                self.kept.push(handles[0]);
                // The second handle goes back to the caller, and is not ours to keep.
                let handles = Handles::from_slice(&handles[1..]).unwrap();
                Ok(Answer { reply: Reply::Grant(GrantReply {}), handles, close_after_reply: true })
            }
            _ => Err(ErrorCode::NotFound),
        }
    }
}

fn caller() -> Caller { Caller { badge: 3, account: 0xff, labels: Labels::new() } }

fn h(i: u32) -> Handle { Handle::new(i).unwrap() }

/// Sends `request` with `handles` through `answer` in a lend of `lend` bytes: the lend
/// afterwards, the reply's words and handles, and the handles to close.
fn round_trip(
    store: &mut Store,
    request: Message<'_>,
    handles: &[Handle],
    lend: usize,
) -> (Vec<u8>, Words, Handles, Handles) {
    let mut buf = vec![0u8; lend.max(64)];
    let words = request.encode(&mut buf).unwrap();
    buf.truncate(lend);
    let handles = Handles::from_slice(handles).unwrap();
    let outcome = answer::<Example, Store>(store, &caller(), &words, &handles, &mut buf);
    (buf, outcome.words, outcome.send, outcome.close)
}

#[test]
fn requests_replies_and_errors() {
    let mut store = Store::default();
    // Inline: fields in the words, no buffer at all.
    let (buf, words, _, _) = round_trip(&mut store, Message::Small(Small { a: 2, b: 40 }), &[], 0);
    assert_eq!(Reply::decode(3, &words, &buf, 0), Ok(Ok(Reply::Small(SmallReply { c: 42 }))));
    // Buffer-shaped: the reply is written over the request in the lend.
    let (buf, words, _, _) = round_trip(&mut store, Message::Named(Named { id: 0x100, name: "x" }), &[], 64);
    assert_eq!(Reply::decode(5, &words, &buf, 0), Ok(Ok(Reply::Named(NamedReply { id: 0x1ff }))));
    // An error is a status alone.
    let (buf, words, reply_handles, _) =
        round_trip(&mut store, Message::Named(Named { id: 1, name: "deny" }), &[], 64);
    assert_eq!(words, ErrorCode::Denied.encode());
    assert!(reply_handles.as_slice().is_empty());
    assert_eq!(Reply::decode(5, &words, &buf, 0), Ok(Err(ErrorCode::Denied)));
    // Reply data comes from the server's state, not the request's buffer.
    let blob = Message::Blob(Blob { offset: 0, data: b"stored bytes", label: "l" });
    let (_, words, _, _) = round_trip(&mut store, blob, &[], 64);
    assert_eq!(words[0], 0);
    let (buf, words, _, _) = round_trip(&mut store, Message::Read(Read { offset: 7, count: 100 }), &[], 64);
    assert_eq!(Reply::decode(8, &words, &buf, 0), Ok(Ok(Reply::Read(ReadReply { data: b"bytes" }))));
}

#[test]
fn handles_travel_and_unread_ones_come_back() {
    let mut store = Store::default();
    let (_, words, reply_handles, close) =
        round_trip(&mut store, Message::Grant(Grant { pages: 1 }), &[h(5), h(6)], 0);
    assert_eq!(words, [0, 0, 0, 0]);
    // Sent, then closed here: a reply's handles are copies.
    assert_eq!((reply_handles.as_slice(), close.as_slice()), (&[h(6)][..], &[h(6)][..]));
    assert_eq!(store.kept, [h(5)]);
    // Wrong handle count: the request does not decode, so its handles come back unread.
    let (_, words, reply_handles, unread) =
        round_trip(&mut store, Message::Grant(Grant { pages: 1 }), &[h(5)], 0);
    assert_eq!(words, crate::server::MALFORMED);
    assert!(reply_handles.as_slice().is_empty());
    assert_eq!(unread.as_slice(), &[h(5)]);
}

#[test]
fn malformed_requests_and_oversized_replies() {
    let mut store = Store::default();
    let bad = crate::server::MALFORMED;
    // The generated `Malformed` (code 1 in every protocol) is the same reply.
    assert_eq!(bad, ErrorCode::Malformed.encode());
    let mut buf = [0u8; 16];
    let none = Handles::new();
    for words in [[0, 0, 0, 0], [99, 0, 0, 0], [3, 1 << 32, 0, 0], [5, 100, 0, 0], [3, 0, 0, 1]] {
        let reply = answer::<Example, Store>(&mut store, &caller(), &words, &none, &mut buf).words;
        assert_eq!(reply, bad, "{words:?}");
    }
    // A buffer-shaped request with no buffer.
    let reply = answer::<Example, Store>(&mut store, &caller(), &[5, 6, 0, 0], &none, &mut []).words;
    assert_eq!(reply, bad);
    // A reply that does not fit the lend.
    round_trip(&mut store, Message::Blob(Blob { offset: 0, data: &[7; 40], label: "" }), &[], 64);
    let (_, words, _, _) = round_trip(&mut store, Message::Read(Read { offset: 0, count: 40 }), &[], 20);
    assert_eq!(words, bad);
}

#[test]
fn random_words_never_panic() {
    let mut store = Store::default();
    let mut x = 0x5eed_u64;
    let mut buf = vec![0u8; 256];
    for _ in 0..50_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let words = [x % 10, (x >> 8) % 300, (x >> 20) & 3, 0];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (x >> (i % 56)) as u8;
        }
        let handles = Handles::from_slice(&[h(1), h(2), h(3)][..(x >> 40) as usize % 4]).unwrap();
        let _ = answer::<Example, Store>(&mut store, &caller(), &words, &handles, &mut buf);
    }
}
