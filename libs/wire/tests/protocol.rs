//! The generated `typed::Protocol` impls: code generic over every protocol sees each message's
//! layout, encodes and decodes exactly as the module's own codec does, and learns nothing the
//! module does not say.

use redoubt_wire::proto::{consol, example, keyd};
use redoubt_wire::typed::{Layout, Protocol, Words};

/// What a generic caller does: the layout, then the words, through the trait alone.
fn request<P: Protocol>(message: &P::Message<'_>, buf: &mut [u8]) -> (Layout, Words) {
    (*P::layout(message).unwrap(), P::encode(message, buf).unwrap())
}

#[test]
fn layouts_are_the_tables() {
    let mut buf = [0u8; 64];
    let small = example::Message::Small(example::Small { a: 1, b: 2 });
    let (layout, words) = request::<example::Protocol>(&small, &mut buf);
    assert_eq!(layout, Layout { opcode: 3, inline: true, handles: 0 });
    assert_eq!(words, small.encode(&mut [0u8; 64]).unwrap());

    let named = example::Message::Named(example::Named { id: 7, name: "x" });
    let (layout, words) = request::<example::Protocol>(&named, &mut buf);
    assert_eq!(layout, Layout { opcode: 5, inline: false, handles: 0 });
    let mut own = [0u8; 64];
    assert_eq!(words, named.encode(&mut own).unwrap());
    assert_eq!(buf, own, "the same bytes in the buffer");

    let grant = example::Message::Grant(example::Grant { pages: 3 });
    assert_eq!(request::<example::Protocol>(&grant, &mut buf).0.handles, 2);

    // `keyd`'s grant is inline and its sign_record a buffer message; `consol` borrows nothing.
    assert!(request::<keyd::Protocol>(&keyd::Message::Grant(keyd::Grant {}), &mut buf).0.inline);
    let sign = keyd::Message::SignRecord(keyd::SignRecord { record: b"r" });
    assert!(!request::<keyd::Protocol>(&sign, &mut buf).0.inline);
    assert_eq!(request::<consol::Protocol>(&consol::Message::Size(consol::Size {}), &mut buf).0.opcode, 16);
}

#[test]
fn replies_and_error_codes_decode_through_the_trait() {
    let mut buf = [0u8; 64];
    let reply = example::Reply::Read(example::ReadReply { data: b"abc" });
    let words = reply.encode(&mut buf).unwrap();
    let decoded = example::Protocol::decode_reply(8, &words, &buf, 0).unwrap();
    assert_eq!(decoded, Ok(reply));

    let refused = example::ErrorCode::Denied.encode();
    assert_eq!(
        example::Protocol::decode_reply(8, &refused, &[], 0).unwrap(),
        Err(example::ErrorCode::Denied)
    );
    assert_eq!(example::Protocol::code(example::ErrorCode::Denied), 3);
    // A code the table does not name is malformed, not a guess.
    assert!(example::Protocol::decode_reply(8, &[9, 0, 0, 0], &[], 0).is_err());
}
