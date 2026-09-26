//! Every property the `blkd` protocol claims, with a test that tries to break it
//! (servers/blkd.md; TENETS.md 6).
//!
//! These drive [`answer_with`] against the hostile fake device, so every path runs with no system
//! call and no hardware. `blkd` mints nothing, so there is no fake kernel here either: answering
//! a request is pure but for what the device does.

use alloc::vec;
use alloc::vec::Vec;

use redoubt_rt::abi::Labels;

use super::*;
use crate::fake::{FakeDevice, Policy};
use crate::image::{Entry, Image};
use crate::virtio::MAX_SECTORS;

const SECTORS: u64 = 8192;
const SECTOR: usize = SECTOR_SIZE as usize;
/// A lend big enough for the largest reply: `MAX_LEND_PAGES` is 16 pages, 64 KiB (kernel/ipc.md).
const LEND: usize = 64 * 1024;

/// The two partitions every test starts with, and the badges that name them.
const FIRST: Entry = Entry { first_lba: 64, last_lba: 1063 };
const SECOND: Entry = Entry { first_lba: 2048, last_lba: 4095 };
const FIRST_BADGE: u64 = 1;
const SECOND_BADGE: u64 = 2;

fn device() -> FakeDevice {
    let image = Image::new(SECTORS, &[FIRST, SECOND]);
    let device = FakeDevice::with_image(image.bytes);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    device
}

fn server(device: &FakeDevice) -> BlockServer<&FakeDevice> {
    let mut disk = Disk::new(device).expect("bring-up");
    let roots = crate::read_partitions(&mut disk).expect("a partition table");
    BlockServer::new(disk, roots)
}

fn caller(badge: u64, account: u64, labels: &[u64]) -> Caller {
    Caller { badge, account, labels: Labels::from_slice(labels).unwrap() }
}

/// What one request answered with.
#[derive(Debug, PartialEq, Eq)]
enum Answered {
    Info {
        sectors: u64,
        sector_size: u32,
        read_only: u32,
    },
    Data(Vec<u8>),
    Written,
    Flushed,
    Err(ErrorCode),
    /// Status 1 in every protocol: the request did not decode, or its reply did not fit.
    Malformed,
}

/// Sends `request` from `caller` and takes the answer apart.
fn ask<T: Transport>(server: &mut BlockServer<T>, caller: &Caller, request: &Message<'_>) -> Answered {
    ask_with_lend(server, caller, request, LEND)
}

/// The same, with a lend of `lend` bytes: a caller that lent less than its reply needs.
fn ask_with_lend<T: Transport>(
    server: &mut BlockServer<T>,
    caller: &Caller,
    request: &Message<'_>,
    lend: usize,
) -> Answered {
    let mut buf = vec![0u8; lend.max(LEND)];
    let words = request.encode(&mut buf).expect("the request encodes");
    let opcode = redoubt_rt::wire::typed::opcode(&words).unwrap();
    // An inline message travels in its words alone, so its caller lends nothing
    // (servers/wire.md); a client that lends anyway is refused, which
    // `malformed_requests_are_refused` checks.
    if inline(request) {
        let outcome = answer_with(server, caller, &words, &ReceivedHandles::new(), &mut []);
        return decode(opcode, &outcome, &[]);
    }
    buf.truncate(lend);
    let outcome = answer_with(server, caller, &words, &ReceivedHandles::new(), &mut buf);
    decode(opcode, &outcome, &buf)
}

/// Whether the message's layout is inline: `flush` has neither fields nor a reply that needs a
/// buffer.
fn inline(request: &Message<'_>) -> bool { matches!(request, Message::Flush(_)) }

/// Sends raw words and buffer bytes: for requests no encoder would produce.
fn ask_raw<T: Transport>(
    server: &mut BlockServer<T>,
    caller: &Caller,
    words: &Words,
    body: &[u8],
) -> Answered {
    let mut buf = vec![0u8; LEND];
    buf[..body.len()].copy_from_slice(body);
    let outcome = answer_with(server, caller, words, &ReceivedHandles::new(), &mut buf);
    decode(redoubt_rt::wire::typed::opcode(words).unwrap_or(0), &outcome, &buf)
}

fn decode(opcode: u32, outcome: &Outcome, buf: &[u8]) -> Answered {
    let handles = outcome.send.as_slice();
    if outcome.words[0] == u64::from(redoubt_rt::wire::typed::MALFORMED) {
        return Answered::Malformed;
    }
    match Reply::decode(opcode, &outcome.words, buf, handles.len()) {
        Ok(Ok(reply)) => match reply {
            Reply::Info(r) => {
                Answered::Info { sectors: r.sectors, sector_size: r.sector_size, read_only: r.read_only }
            }
            Reply::Read(r) => Answered::Data(r.data.to_vec()),
            Reply::Write(_) => Answered::Written,
            Reply::Flush(_) => Answered::Flushed,
        },
        Ok(Err(code)) => Answered::Err(code),
        Err(_) => Answered::Malformed,
    }
}

fn read(sector: u64, count: u32) -> Message<'static> { Message::Read(Read { sector, count }) }

fn data_of(answered: &Answered) -> &[u8] {
    match answered {
        Answered::Data(bytes) => bytes,
        other => panic!("expected data, got {other:?}"),
    }
}

// ---------------------------------------------------------------- the happy path

/// A block round-trips through a root badge, and the sectors it names are its partition's.
#[test]
fn a_block_round_trips_through_a_range_badge() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);

    let payload: Vec<u8> = (0..SECTOR).map(|i| (i % 251) as u8).collect();
    assert_eq!(
        ask(&mut server, &alice, &Message::Write(Write { sector: 0, data: &payload })),
        Answered::Written
    );
    assert_eq!(ask(&mut server, &alice, &Message::Flush(Flush {})), Answered::Flushed);
    assert_eq!(data_of(&ask(&mut server, &alice, &read(0, 1))), &payload[..]);
    // Sector 0 of the range is LBA 64 of the disk, which is where the bytes landed.
    assert_eq!(device.sector(FIRST.first_lba), payload);
    assert_eq!(device.strayed(), 0);
}

/// `info` answers about the range the badge names, never about the disk.
#[test]
fn info_describes_the_range_not_the_disk() {
    let device = device();
    let mut server = server(&device);
    assert_eq!(
        ask(&mut server, &caller(FIRST_BADGE, 0, &[]), &Message::Info(Info {})),
        Answered::Info { sectors: 1000, sector_size: 512, read_only: 0 }
    );
    assert_eq!(
        ask(&mut server, &caller(SECOND_BADGE, 0, &[]), &Message::Info(Info {})),
        Answered::Info { sectors: 2048, sector_size: 512, read_only: 0 }
    );
}

/// The largest request the protocol allows.
#[test]
fn a_full_run_round_trips() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    let payload: Vec<u8> = (0..MAX_SECTORS as usize * SECTOR).map(|i| (i * 3 % 249) as u8).collect();
    assert_eq!(
        ask(&mut server, &alice, &Message::Write(Write { sector: 8, data: &payload })),
        Answered::Written
    );
    assert_eq!(data_of(&ask(&mut server, &alice, &read(8, MAX_SECTORS))), &payload[..]);
}

// ---------------------------------------------------------------- a filesystem sees only its partition

/// The whole point of a range: no sector number a client can write down names a sector outside
/// its own partition, and the two partitions cannot see each other's bytes.
#[test]
fn a_range_cannot_name_a_sector_outside_itself() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    let bob = caller(SECOND_BADGE, 0, &[]);

    // Alice's range is 1000 sectors, so 1000 and beyond are not hers.
    for sector in [1000u64, 1001, 2048, SECTORS, u64::MAX] {
        assert_eq!(
            ask(&mut server, &alice, &read(sector, 1)),
            Answered::Err(ErrorCode::OutOfRange),
            "sector {sector}"
        );
    }
    // A run that starts inside and ends outside is refused whole.
    assert_eq!(ask(&mut server, &alice, &read(999, 2)), Answered::Err(ErrorCode::OutOfRange));
    // A count that would overflow the addition is refused, not wrapped.
    assert_eq!(ask(&mut server, &alice, &read(u64::MAX, MAX_SECTORS)), Answered::Err(ErrorCode::OutOfRange));

    // Bob writes his own sector 0; Alice cannot see it anywhere in her range.
    let mark = vec![0xbb; SECTOR];
    assert_eq!(ask(&mut server, &bob, &Message::Write(Write { sector: 0, data: &mark })), Answered::Written);
    assert_eq!(device.sector(SECOND.first_lba), mark);
    for sector in 0..8u64 {
        assert_ne!(data_of(&ask(&mut server, &alice, &read(sector, 1))), &mark[..]);
    }
    // And a write outside her range does not reach his partition.
    let before = device.sector(SECOND.first_lba);
    assert_eq!(
        ask(&mut server, &alice, &Message::Write(Write { sector: 1984, data: &mark })),
        Answered::Err(ErrorCode::OutOfRange)
    );
    assert_eq!(device.sector(SECOND.first_lba), before);
}

/// A badge that names no range gets the same answer whether it is the receive right, an entry the
/// table does not use, or one past the end of the array.
#[test]
fn a_badge_that_names_no_range_is_refused() {
    let device = device();
    let mut server = server(&device);
    // Badge 0 is the receive right; 3 and 99 are entries this table leaves unused; the last two
    // are past the array and past `usize` on rv32.
    for badge in [0u64, 3, 99, 129, 1 << 63, u64::MAX] {
        assert_eq!(
            ask(&mut server, &caller(badge, 0, &[]), &Message::Info(Info {})),
            Answered::Err(ErrorCode::NotPermitted),
            "badge {badge}"
        );
    }
}

// ---------------------------------------------------------------- bounds

#[test]
fn a_request_over_the_bound_is_refused_and_one_of_no_sectors_is_malformed() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    assert_eq!(ask(&mut server, &alice, &read(0, 0)), Answered::Malformed);
    assert_eq!(ask(&mut server, &alice, &read(0, MAX_SECTORS + 1)), Answered::Err(ErrorCode::TooMany));
    assert_eq!(ask(&mut server, &alice, &read(0, u32::MAX)), Answered::Err(ErrorCode::TooMany));

    // A write must be a whole number of sectors, and not empty.
    for len in [0usize, 1, SECTOR - 1, SECTOR + 1] {
        let data = vec![0; len];
        assert_eq!(
            ask(&mut server, &alice, &Message::Write(Write { sector: 0, data: &data })),
            Answered::Malformed,
            "a write of {len} bytes"
        );
    }
    let data = vec![0; (MAX_SECTORS as usize + 1) * SECTOR];
    assert_eq!(
        ask(&mut server, &alice, &Message::Write(Write { sector: 0, data: &data })),
        Answered::Err(ErrorCode::TooMany)
    );
}

/// A caller that lends less than its reply needs is told so before the disk is touched, rather
/// than after, with the answer thrown away.
#[test]
fn a_read_whose_reply_would_not_fit_the_lend_is_refused_before_the_disk_is_touched() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    let before = device.notified();
    assert_eq!(ask_with_lend(&mut server, &alice, &read(0, 8), 1024), Answered::Err(ErrorCode::TooMany));
    assert_eq!(device.notified(), before, "the device was asked to do something");
    // Exactly enough: the data plus its `u32` length.
    assert!(matches!(ask_with_lend(&mut server, &alice, &read(0, 8), 8 * SECTOR + 4), Answered::Data(_)));
}

// ---------------------------------------------------------------- labels

/// A range carries no labels in milestone 1, so `check` lets any caller read one and only an
/// unlabelled caller write to it (servers/serving.md R25).
#[test]
fn a_labelled_caller_may_read_but_not_write() {
    let device = device();
    let mut server = server(&device);
    let vault = caller(FIRST_BADGE, 7, &[42]);
    assert!(matches!(ask(&mut server, &vault, &Message::Info(Info {})), Answered::Info { .. }));
    assert!(matches!(ask(&mut server, &vault, &read(0, 1)), Answered::Data(_)));
    let data = vec![0; SECTOR];
    assert_eq!(
        ask(&mut server, &vault, &Message::Write(Write { sector: 0, data: &data })),
        Answered::Err(ErrorCode::NotPermitted)
    );
    assert_eq!(ask(&mut server, &vault, &Message::Flush(Flush {})), Answered::Err(ErrorCode::NotPermitted));
}

// ---------------------------------------------------------------- malformed requests

/// Words and bytes no encoder would produce. None of them may do anything but `malformed`.
#[test]
fn malformed_requests_are_refused() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    let cases: &[(&str, Words, &[u8])] = &[
        ("opcode 0 is 9P, not this protocol", [0, 0, 0, 0], &[]),
        ("an opcode the table does not hold", [7, 0, 0, 0], &[]),
        ("an opcode past the table", [u64::MAX, 0, 0, 0], &[]),
        ("a read whose fields are not there", [2, 0, 0, 0], &[]),
        ("a read with a length longer than the buffer", [2, u64::MAX, 0, 0], &[]),
        ("a write whose `bytes` length overflows", [3, 12, 0, 0], &[0; 12]),
        ("trailing bytes after the fields", [2, 13, 0, 0], &[0; 13]),
        ("a buffer message sent inline", [2, 0, 1, 0], &[]),
    ];
    for (why, words, body) in cases {
        assert_eq!(ask_raw(&mut server, &alice, words, body), Answered::Malformed, "{why}");
    }
    assert!(!server.disk().is_broken(), "a malformed request never touches the device");
}

/// Arbitrary bytes as a request, from every badge a caller could hold. The assertion is that it
/// answers something and never panics; the fuzz target is the same body under coverage.
#[test]
fn arbitrary_requests_never_panic() {
    let device = device();
    let mut server = server(&device);
    let mut state = 0xa5a5_1234_5678_9abcu64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..20_000 {
        let words = [next() % 9, next() % 200, next(), next()];
        let len = (next() % 64) as usize;
        let body: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
        let badge = match next() % 4 {
            0 => FIRST_BADGE,
            1 => SECOND_BADGE,
            2 => next() % 200,
            _ => next(),
        };
        let who = caller(badge, next() % 3, &[]);
        let _ = ask_raw(&mut server, &who, &words, &body);
    }
    assert_eq!(device.strayed(), 0);
}

/// Alice's write leaves 32 KiB in the DMA buffer; Bob reads from his own partition against a
/// device that writes only half of what it was given. One client's bytes must not reach another's
/// reply, and that must not depend on the device behaving (servers/blkd.md R52).
#[test]
fn one_clients_read_never_carries_anothers_bytes() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    let bob = caller(SECOND_BADGE, 0, &[]);
    let secret = vec![0x5a; MAX_SECTORS as usize * SECTOR];
    for count in [1u32, MAX_SECTORS] {
        assert_eq!(
            ask(&mut server, &alice, &Message::Write(Write { sector: 0, data: &secret })),
            Answered::Written
        );
        device.set_policy(Policy { defer: true, short_write: true, ..Policy::default() });
        let got = ask(&mut server, &bob, &read(0, count));
        let bytes = data_of(&got);
        assert_eq!(bytes.len(), count as usize * SECTOR);
        assert!(!bytes.contains(&0x5a), "Alice's bytes reached Bob's reply of {count} sectors");
        device.set_policy(Policy { defer: true, ..Policy::default() });
    }
}

/// A badge names a GPT entry, so `roots` has one slot per entry and the slots are where the
/// entries are, gaps included.
#[test]
fn roots_have_one_slot_per_gpt_entry() {
    let device = device();
    let server = server(&device);
    assert_eq!(server.roots().len(), crate::image::ENTRIES as usize);
    assert_eq!(server.roots()[0], Some(Range::new(FIRST.first_lba, 1000, SECTORS).unwrap()));
    assert_eq!(server.roots()[1], Some(Range::new(SECOND.first_lba, 2048, SECTORS).unwrap()));
    assert!(server.roots()[2..].iter().all(Option::is_none));
}

// ---------------------------------------------------------------- the device underneath

/// A device that lies is a `failed` to the client, and stays failed: `blkd` says no more than
/// that, and never answers a read with bytes it did not get.
#[test]
fn a_lying_device_becomes_failed_and_stays_failed() {
    let device = device();
    let mut server = server(&device);
    let alice = caller(FIRST_BADGE, 0, &[]);
    device.set_policy(Policy { defer: true, used_id: Some(3), ..Policy::default() });
    assert_eq!(ask(&mut server, &alice, &read(0, 1)), Answered::Err(ErrorCode::Failed));
    device.set_policy(Policy { defer: true, ..Policy::default() });
    assert_eq!(ask(&mut server, &alice, &read(0, 1)), Answered::Err(ErrorCode::Failed));
    assert_eq!(ask(&mut server, &alice, &Message::Flush(Flush {})), Answered::Err(ErrorCode::Failed));
    // Everything that does not touch the device still answers.
    // `info` still answers, from numbers taken at bring-up: it touches no device.
    assert!(matches!(ask(&mut server, &alice, &Message::Info(Info {})), Answered::Info { .. }));
    assert_eq!(device.strayed(), 0);
}
