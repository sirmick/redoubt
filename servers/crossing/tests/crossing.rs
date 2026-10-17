//! The crossing program's one decision (servers/steward.md, "Declassification and push"; R42):
//! it answers the one call its mode allows, cuts an item at one past the bound (the core's
//! `too_big`), refuses a second call, and is done once its call is answered.

use redoubt_crossing::{Crossing, CrossingProtocol, DECLASSIFY_MAX, Mode, READ_MAX, Refused, Volume};
use redoubt_rt::abi::ReceivedHandles;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::typed::answer;
use redoubt_rt::wire::proto::crossing::{ErrorCode, Message, Read, Reply, Write};

/// A volume of one item, which records what was written.
struct Items {
    name: String,
    bytes: Vec<u8>,
    written: Vec<(String, Vec<u8>)>,
}

impl Volume for Items {
    fn read(&mut self, path: &str, max: usize) -> Result<Vec<u8>, Refused> {
        if path != self.name {
            return Err(Refused);
        }
        Ok(self.bytes[..self.bytes.len().min(max)].to_vec())
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Refused> {
        self.written.push((path.into(), bytes.to_vec()));
        Ok(())
    }
}

fn items(bytes: &[u8]) -> Items {
    Items { name: "notes/note".into(), bytes: bytes.to_vec(), written: Vec::new() }
}

fn call<'b>(c: &mut Crossing<Items>, m: Message<'_>, buf: &'b mut [u8]) -> Result<Reply<'b>, ErrorCode> {
    let words = m.encode(buf).unwrap();
    let caller = Caller { badge: 1, account: 0, labels: Default::default() };
    let outcome = answer::<CrossingProtocol, _>(c, &caller, &words, &ReceivedHandles::new(), buf);
    Reply::decode(words[0] as u32, &outcome.words, buf, 0).unwrap()
}

#[test]
fn a_read_answers_the_item_once_and_is_then_served() {
    let mut c = Crossing::new(Mode::Read, items(b"forty bytes of text, printable, as asked"));
    let mut buf = vec![0u8; 4096];
    assert!(!c.served());
    let reply = call(&mut c, Message::Read(Read { path: "notes/note" }), &mut buf);
    assert!(matches!(reply, Ok(Reply::Read(r)) if r.bytes == b"forty bytes of text, printable, as asked"));
    assert!(c.served(), "the program exits once its one call is answered");
    let again = call(&mut c, Message::Read(Read { path: "notes/note" }), &mut buf);
    assert_eq!(again.unwrap_err(), ErrorCode::Served);
}

/// An item of `DECLASSIFY_MAX` bytes is answered whole; a longer one is cut at `READ_MAX`, one
/// past the bound, which is all the core needs to say `too_big`, and nothing past it is ever
/// asked of the volume.
#[test]
fn an_item_over_the_bound_is_cut_one_past_declassify_max() {
    assert_eq!(READ_MAX, DECLASSIFY_MAX + 1);
    let mut buf = vec![0u8; 4096];
    let mut c = Crossing::new(Mode::Read, items(&vec![b'x'; DECLASSIFY_MAX]));
    let reply = call(&mut c, Message::Read(Read { path: "notes/note" }), &mut buf);
    assert!(matches!(reply, Ok(Reply::Read(r)) if r.bytes.len() == DECLASSIFY_MAX), "at the bound, whole");
    for over in [DECLASSIFY_MAX + 1, DECLASSIFY_MAX + 300] {
        let mut c = Crossing::new(Mode::Read, items(&vec![b'x'; over]));
        let reply = call(&mut c, Message::Read(Read { path: "notes/note" }), &mut buf);
        assert!(matches!(reply, Ok(Reply::Read(r)) if r.bytes.len() == READ_MAX), "{over}: cut at READ_MAX");
        assert!(c.served());
    }
}

#[test]
fn a_write_writes_the_bytes_as_the_item_and_the_other_mode_s_call_is_failed() {
    let mut buf = vec![0u8; 8192];
    let mut c = Crossing::new(Mode::Write, items(b""));
    let bytes = vec![7u8; 4096];
    let reply = call(&mut c, Message::Write(Write { path: "secret/in", bytes: &bytes }), &mut buf);
    assert!(matches!(reply, Ok(Reply::Write(_))));
    assert!(c.served());
    let mut c = Crossing::new(Mode::Write, items(b""));
    let reply = call(&mut c, Message::Read(Read { path: "notes/note" }), &mut buf);
    assert_eq!(reply.unwrap_err(), ErrorCode::Failed);
    let mut c = Crossing::new(Mode::Read, items(b"x"));
    let reply = call(&mut c, Message::Write(Write { path: "secret/in", bytes: b"y" }), &mut buf);
    assert_eq!(reply.unwrap_err(), ErrorCode::Failed);
}

#[test]
fn a_missing_item_is_failed() {
    let mut buf = vec![0u8; 4096];
    let mut c = Crossing::new(Mode::Read, items(b"x"));
    let reply = call(&mut c, Message::Read(Read { path: "notes/other" }), &mut buf);
    assert_eq!(reply.unwrap_err(), ErrorCode::Failed);
}
