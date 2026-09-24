//! The `netif` protocol (IO-ARCHITECTURE.md, `netd`) against the fake device: one client, no
//! labelled callers, `too_many` for a frame of the wrong length, `busy` for a full ring, and
//! `failed` for good once the device has lied.

use redoubt_netd::fake::{FakeNic, Policy, View};
use redoubt_netd::ring::QUEUE_SIZE;
use redoubt_netd::server::answer_with;
use redoubt_netd::{NetServer, Up, bring_up};
use redoubt_rt::abi::{Labels, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Words};
use redoubt_rt::wire::proto::netif::{ErrorCode, Info, InfoReply, Message, Reply, Transmit};

const CLIENT: u64 = 7;

fn caller(badge: u64, labels: &[u64]) -> Caller {
    Caller { badge, account: 0, labels: Labels::from_slice(labels).unwrap() }
}

fn server(nic: &FakeNic) -> NetServer<View<'_>> {
    let Up { mac, tx, .. } = bring_up(&nic.rx_view(), &nic.tx_view()).unwrap();
    NetServer::new(nic.tx_view(), tx, mac, CLIENT)
}

/// One call: encodes `message` into a one-page lend as `ipd` would, answers it, and decodes the
/// reply.
fn call(server: &mut NetServer<View<'_>>, who: &Caller, message: Message<'_>) -> Result<Reply, ErrorCode> {
    // An inline request (`info`) comes with no lend, a buffer one (`transmit`) with one page.
    let mut lend = vec![0u8; if matches!(message, Message::Info(_)) { 0 } else { 4096 }];
    let words = message.encode(&mut lend).unwrap();
    raw(server, who, &words, &mut lend, opcode(&message))
}

fn raw(server: &mut NetServer<View<'_>>, who: &Caller, words: &Words, lend: &mut [u8], op: u32) -> Result<Reply, ErrorCode> {
    let outcome = answer_with(server, who, words, &ReceivedHandles::new(), lend);
    Reply::decode(op, &outcome.words, lend, 0).expect("a reply that decodes")
}

fn opcode(message: &Message<'_>) -> u32 {
    match message {
        Message::Info(_) => 1,
        Message::Transmit(_) => 2,
    }
}

#[test]
fn info_and_transmit_for_the_client() {
    let nic = FakeNic::new();
    let mut s = server(&nic);
    let me = caller(CLIENT, &[]);
    let Ok(Reply::Info(InfoReply { mac, mtu })) = call(&mut s, &me, Message::Info(Info {})) else { panic!() };
    assert_eq!((mac & 0xff, mtu), (0x52, 1500));
    let frame = [0x42u8; 60];
    assert!(call(&mut s, &me, Message::Transmit(Transmit { frame: &frame })).is_ok());
    assert_eq!(&nic.wire()[0][12..], &frame[..]);
    assert_eq!(nic.strayed(), 0);
}

#[test]
fn anyone_else_is_not_permitted() {
    let nic = FakeNic::new();
    let mut s = server(&nic);
    for who in [caller(CLIENT + 1, &[]), caller(1 << 63, &[]), caller(CLIENT, &[5])] {
        assert_eq!(call(&mut s, &who, Message::Info(Info {})), Err(ErrorCode::NotPermitted), "{who:?}");
        let frame = [0u8; 60];
        assert_eq!(call(&mut s, &who, Message::Transmit(Transmit { frame: &frame })), Err(ErrorCode::NotPermitted));
    }
    assert!(nic.wire().is_empty(), "nothing a stranger asked for reached the wire");
}

#[test]
fn a_frame_of_the_wrong_length_is_too_many() {
    let nic = FakeNic::new();
    let mut s = server(&nic);
    let me = caller(CLIENT, &[]);
    for len in [0, 13, 1515, 4000] {
        let frame = vec![1u8; len];
        assert_eq!(call(&mut s, &me, Message::Transmit(Transmit { frame: &frame })), Err(ErrorCode::TooMany));
    }
    assert!(!s.broken());
}

#[test]
fn a_full_ring_is_busy_and_a_lie_is_failed_for_good() {
    let nic = FakeNic::new();
    nic.set_policy(Policy { tx_never_complete: true, ..Default::default() });
    let mut s = server(&nic);
    let me = caller(CLIENT, &[]);
    let frame = [0u8; 60];
    for _ in 0..QUEUE_SIZE {
        assert!(call(&mut s, &me, Message::Transmit(Transmit { frame: &frame })).is_ok());
    }
    assert_eq!(call(&mut s, &me, Message::Transmit(Transmit { frame: &frame })), Err(ErrorCode::Busy));
    // Ten seconds on, the device has kept a slot too long: it is reset and never trusted again.
    nic.advance(redoubt_netd::virtio::TX_TIMEOUT_US);
    assert_eq!(call(&mut s, &me, Message::Transmit(Transmit { frame: &frame })), Err(ErrorCode::Failed));
    assert!(s.broken());
    assert_eq!(nic.status(), 0, "a broken device is reset");
    nic.set_policy(Policy::default());
    assert_eq!(call(&mut s, &me, Message::Info(Info {})), Err(ErrorCode::Failed));
}

#[test]
fn a_request_that_does_not_decode_is_malformed() {
    let nic = FakeNic::new();
    let mut s = server(&nic);
    let me = caller(CLIENT, &[]);
    let mut lend = vec![0u8; 64];
    for words in [[9, 0, 0, 0], [2, 4096, 0, 0], [2, 3, 0, 0]] {
        assert_eq!(raw(&mut s, &me, &words, &mut lend, 2), Err(ErrorCode::Malformed), "{words:?}");
    }
}

/// `netd`'s one argument: `client=BADGE`, decimal, nonzero, below 2^63, and nothing else.
#[test]
fn arguments_are_exactly_one_client_badge() {
    use redoubt_netd::parse_client;
    assert_eq!(parse_client(["client=7"].into_iter()), Some(7));
    assert_eq!(parse_client(["client=9223372036854775807"].into_iter()), Some((1 << 63) - 1));
    let bad: [&[&str]; 10] = [
        &[],
        &["client=0"],
        &["client=07"],
        &["client=9223372036854775808"],
        &["client=99999999999999999999999"],
        &["client=7", "client=8"],
        &["client="],
        &["client=-1"],
        &["client=1a"],
        &["badge=7"],
    ];
    for args in bad {
        assert_eq!(parse_client(args.iter().copied()), None, "{args:?}");
    }
}

/// The `request` fuzz target's run, as a seeded sweep (the fuzzer needs `cargo-fuzz`): arbitrary
/// words, lends and callers, and only the client's well-formed transmits reach the wire.
#[test]
fn randomized_requests_reach_the_wire_only_from_the_client() {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut byte = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 24) as u8
    };
    let nic = FakeNic::new();
    let mut s = server(&nic);
    for _ in 0..20_000 {
        let head = byte();
        let badge = if head & 1 == 0 { CLIENT } else { u64::from(head) };
        let labels: &[u64] = if head & 2 == 0 { &[] } else { &[9] };
        let who = caller(badge, labels);
        let words = [u64::from(byte() % 4), u64::from(byte()) * 7, u64::from(byte() % 2), 0];
        let mut lend: Vec<u8> = (0..usize::from(byte()) * 8).map(|_| byte()).collect();
        let before = nic.wire().len();
        let _ = answer_with(&mut s, &who, &words, &ReceivedHandles::new(), &mut lend);
        if badge != CLIENT || !labels.is_empty() {
            assert_eq!(nic.wire().len(), before, "a stranger's request reached the wire");
        }
    }
    assert_eq!(nic.strayed(), 0);
    assert!(nic.wire().iter().all(|sent| (12 + 14..=12 + 1514).contains(&sent.len())));
}
