//! `crossing`: the steward's reader or writer of one item (servers/steward.md,
//! "Declassification and push"; R42). The steward launches it in a short-lived budget carrying
//! exactly the labelled side's label set, with one connection to that side's volume at its root
//! (the steward, unlabelled, may mint nothing below it: R25), and calls it once on an endpoint
//! the steward made: `read` walks to the item and answers its bytes, `write` writes the bytes the
//! call brought as it. The program decides nothing: it answers the one call its mode allows and
//! exits, and its budget's deadline ends it if it hangs.
//!
//! Everything it does is here, behind [`Volume`], so host tests drive the same code
//! (`tests/crossing.rs`); the program's side of the trait is `src/bin/crossing.rs`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_rt::abi::Handle;
use redoubt_rt::ipc::{Caller, Words};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::crossing::{ErrorCode, Message, Reply, WriteReply};
pub use redoubt_steward::consts::DECLASSIFY_MAX;

/// The most a read answers: an item is cut at one byte past `DECLASSIFY_MAX`, so the steward's
/// core sees an item over the bound as `too_big` and the lend is bounded whatever the item's
/// size. The program judges nothing of the size.
pub const READ_MAX: usize = DECLASSIFY_MAX + 1;

/// Which one call the program answers, from its first argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Read,
    Write,
}

impl Mode {
    pub fn parse(arg: &str) -> Option<Mode> {
        match arg {
            "read" => Some(Mode::Read),
            "write" => Some(Mode::Write),
            _ => None,
        }
    }
}

/// Why the volume did not do it: no such item, a write the server refused, the connection gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

/// The one connection, as the program uses it: one item read, or one written whole.
pub trait Volume {
    /// The item at `path` below the connection's root, its first `max` bytes at most.
    fn read(&mut self, path: &str, max: usize) -> Result<Vec<u8>, Refused>;
    /// The item at `path` written as exactly `bytes`, made in its directory or truncated.
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Refused>;
}

/// The protocol, for `redoubt-rt`'s typed dispatch.
pub struct CrossingProtocol;

impl Protocol for CrossingProtocol {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// The program's state: its mode, its volume, and whether its one call was answered.
pub struct Crossing<V> {
    mode: Mode,
    volume: V,
    served: bool,
    /// What the read answered, which the reply borrows.
    bytes: Vec<u8>,
}

impl<V: Volume> Crossing<V> {
    pub fn new(mode: Mode, volume: V) -> Crossing<V> {
        Crossing { mode, volume, served: false, bytes: Vec::new() }
    }

    /// Whether the one call is answered: the program exits then.
    pub fn served(&self) -> bool { self.served }
}

impl<V: Volume> TypedServer<CrossingProtocol> for Crossing<V> {
    /// The one call the mode allows: a second call of any kind is `served`, a call of the other
    /// kind `failed`, and so is the volume's refusal. A refused call counts as served too: the
    /// steward's step failed, and the budget is destroyed next.
    fn handle<'s>(
        &'s mut self,
        _caller: &Caller,
        request: Message<'_>,
        _handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        if self.served {
            return Err(ErrorCode::Served);
        }
        self.served = true;
        let refused = |Refused| ErrorCode::Failed;
        match (self.mode, request) {
            (Mode::Read, Message::Read(m)) => {
                self.bytes = self.volume.read(m.path, READ_MAX).map_err(refused)?;
                Ok(Answer::new(Reply::Read(redoubt_rt::wire::proto::crossing::ReadReply {
                    bytes: &self.bytes,
                })))
            }
            (Mode::Write, Message::Write(m)) => {
                self.volume.write(m.path, m.bytes).map_err(refused)?;
                Ok(Answer::new(Reply::Write(WriteReply {})))
            }
            _ => Err(ErrorCode::Failed),
        }
    }
}
