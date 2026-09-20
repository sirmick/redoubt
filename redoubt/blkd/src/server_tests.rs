//! Every property the `blkd` protocol claims, with a test that tries to break it
//! (BUILD-PLAN.md, WP-D1; TENETS.md 6).
//!
//! These drive [`answer_with`] against a fake kernel of a few lines and the hostile fake device,
//! so every path — minting included — runs with no system call and no hardware.

use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_rt::abi::Labels;
use redoubt_rt::server::{AdmitKey, Resource};

use super::*;
use crate::fake::{FakeDevice, Policy};
use crate::image::{Entry, Image};
use crate::virtio::MAX_SECTORS;

const SECTORS: u64 = 8192;
const SECTOR: usize = SECTOR_SIZE as usize;
/// A lend big enough for the largest reply: `MAX_LEND_PAGES` is 16 pages, 64 KiB (WIRE.md).
const LEND: usize = 64 * 1024;

/// The two partitions every test starts with, and the badges that name them.
const FIRST: Entry = Entry { first_lba: 64, last_lba: 1063 };
const SECOND: Entry = Entry { first_lba: 2048, last_lba: 4095 };
const FIRST_BADGE: u64 = 1;
const SECOND_BADGE: u64 = 2;

/// The word the tests draw the first granted badge from, so a failure reproduces.
const TEST_RANDOM: u64 = 0x0f1e_2d3c_4b5a_6978;

fn device() -> FakeDevice {
    let image = Image::new(SECTORS, &[FIRST, SECOND]);
    let device = FakeDevice::with_image(image.bytes);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    device
}

fn server(device: &FakeDevice) -> BlockServer<&FakeDevice> {
    let mut disk = Disk::new(device).expect("bring-up");
    let roots = crate::read_partitions(&mut disk).expect("a partition table");
    BlockServer::new(disk, roots, LIMITS, &COST, BUDGET, TEST_RANDOM).expect("limits that fit")
}

/// The first badge this table mints, which a test needs so it can speak through a grant without
/// holding the handle the kernel would have given it.
fn first_granted_badge() -> u64 { redoubt_rt::server::minted::first_badge(TEST_RANDOM) }

fn caller(badge: u64, account: u64, labels: &[u64]) -> Caller {
    Caller { badge, account, labels: Labels::from_slice(labels).unwrap() }
}

/// A kernel of a few lines: it hands out handle numbers and draws ids from a fixed sequence, so a
/// failure reproduces.
struct FakeKernel {
    next_handle: u32,
    rng: u64,
    mint_fails: bool,
}

impl FakeKernel {
    fn new() -> FakeKernel { FakeKernel { next_handle: 100, rng: 0x2545_f491_4f6c_dd1d, mint_fails: false } }
}

impl Minter for FakeKernel {
    fn mint(&mut self, _badge: NonZeroU64) -> Result<Handle, Error> {
        if self.mint_fails {
            return Err(Error::OutOfMemory);
        }
        self.next_handle += 1;
        Ok(Handle::new(self.next_handle).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
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
    Granted(u64, Vec<Handle>),
    Released,
    Err(ErrorCode),
    /// Status 1 in every protocol: the request did not decode, or its reply did not fit.
    Malformed,
}

/// Sends `request` from `caller` and takes the answer apart.
fn ask<T: Transport>(
    server: &mut BlockServer<T>,
    kernel: &mut FakeKernel,
    caller: &Caller,
    request: &Message<'_>,
) -> Answered {
    ask_with_lend(server, kernel, caller, request, LEND)
}

/// The same, with a lend of `lend` bytes: a caller that lent less than its reply needs.
fn ask_with_lend<T: Transport>(
    server: &mut BlockServer<T>,
    kernel: &mut FakeKernel,
    caller: &Caller,
    request: &Message<'_>,
    lend: usize,
) -> Answered {
    let mut buf = vec![0u8; lend.max(LEND)];
    let words = request.encode(&mut buf).expect("the request encodes");
    let opcode = redoubt_rt::wire::typed::opcode(&words).unwrap();
    // An inline message travels in its words alone, so its caller lends nothing (WIRE.md); a
    // client that lends anyway is refused, which `malformed_requests_are_refused` checks.
    if inline(request) {
        let outcome = answer_with(server, caller, &words, &ReceivedHandles::new(), &mut [], kernel);
        return decode(opcode, &outcome, &[]);
    }
    buf.truncate(lend);
    let outcome = answer_with(server, caller, &words, &ReceivedHandles::new(), &mut buf, kernel);
    decode(opcode, &outcome, &buf)
}

/// Whether the message's layout is inline: `flush` and `release` have neither fields nor a reply
/// that needs a buffer.
fn inline(request: &Message<'_>) -> bool { matches!(request, Message::Flush(_) | Message::Release(_)) }

/// Sends raw words and buffer bytes: for requests no encoder would produce.
fn ask_raw<T: Transport>(
    server: &mut BlockServer<T>,
    kernel: &mut FakeKernel,
    caller: &Caller,
    words: &Words,
    body: &[u8],
) -> Answered {
    let mut buf = vec![0u8; LEND];
    buf[..body.len()].copy_from_slice(body);
    let outcome = answer_with(server, caller, words, &ReceivedHandles::new(), &mut buf, kernel);
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
            Reply::Grant(r) => Answered::Granted(r.id, handles.to_vec()),
            Reply::Release(_) => Answered::Released,
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
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);

    let payload: Vec<u8> = (0..SECTOR).map(|i| (i % 251) as u8).collect();
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Write(Write { sector: 0, data: &payload })),
        Answered::Written
    );
    assert_eq!(ask(&mut server, &mut kernel, &alice, &Message::Flush(Flush {})), Answered::Flushed);
    assert_eq!(data_of(&ask(&mut server, &mut kernel, &alice, &read(0, 1))), &payload[..]);
    // Sector 0 of the range is LBA 64 of the disk, which is where the bytes landed.
    assert_eq!(device.sector(FIRST.first_lba), payload);
    assert_eq!(device.strayed(), 0);
}

/// `info` answers about the range the badge names, never about the disk.
#[test]
fn info_describes_the_range_not_the_disk() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    assert_eq!(
        ask(&mut server, &mut kernel, &caller(FIRST_BADGE, 0, &[]), &Message::Info(Info {})),
        Answered::Info { sectors: 1000, sector_size: 512, read_only: 0 }
    );
    assert_eq!(
        ask(&mut server, &mut kernel, &caller(SECOND_BADGE, 0, &[]), &Message::Info(Info {})),
        Answered::Info { sectors: 2048, sector_size: 512, read_only: 0 }
    );
}

/// The largest request the protocol allows.
#[test]
fn a_full_run_round_trips() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let payload: Vec<u8> = (0..MAX_SECTORS as usize * SECTOR).map(|i| (i * 3 % 249) as u8).collect();
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Write(Write { sector: 8, data: &payload })),
        Answered::Written
    );
    assert_eq!(data_of(&ask(&mut server, &mut kernel, &alice, &read(8, MAX_SECTORS))), &payload[..]);
}

// ---------------------------------------------------------------- a filesystem sees only its partition

/// The whole point of a range: no sector number a client can write down names a sector outside
/// its own partition, and the two partitions cannot see each other's bytes.
#[test]
fn a_range_cannot_name_a_sector_outside_itself() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let bob = caller(SECOND_BADGE, 0, &[]);

    // Alice's range is 1000 sectors, so 1000 and beyond are not hers.
    for sector in [1000u64, 1001, 2048, SECTORS, u64::MAX] {
        assert_eq!(
            ask(&mut server, &mut kernel, &alice, &read(sector, 1)),
            Answered::Err(ErrorCode::OutOfRange),
            "sector {sector}"
        );
    }
    // A run that starts inside and ends outside is refused whole.
    assert_eq!(ask(&mut server, &mut kernel, &alice, &read(999, 2)), Answered::Err(ErrorCode::OutOfRange));
    // A count that would overflow the addition is refused, not wrapped.
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &read(u64::MAX, MAX_SECTORS)),
        Answered::Err(ErrorCode::OutOfRange)
    );

    // Bob writes his own sector 0; Alice cannot see it anywhere in her range.
    let mark = vec![0xbb; SECTOR];
    assert_eq!(
        ask(&mut server, &mut kernel, &bob, &Message::Write(Write { sector: 0, data: &mark })),
        Answered::Written
    );
    assert_eq!(device.sector(SECOND.first_lba), mark);
    for sector in 0..8u64 {
        assert_ne!(data_of(&ask(&mut server, &mut kernel, &alice, &read(sector, 1))), &mark[..]);
    }
    // And a write outside her range does not reach his partition.
    let before = device.sector(SECOND.first_lba);
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Write(Write { sector: 1984, data: &mark })),
        Answered::Err(ErrorCode::OutOfRange)
    );
    assert_eq!(device.sector(SECOND.first_lba), before);
}

/// A badge that names no range gets the same answer whether it is out of the table, the receive
/// right, or a granted badge from before a restart.
#[test]
fn a_badge_that_names_no_range_is_refused() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    for badge in [0u64, 3, 99, FIRST_GRANTED_BADGE, u64::MAX] {
        assert_eq!(
            ask(&mut server, &mut kernel, &caller(badge, 0, &[]), &Message::Info(Info {})),
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
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    assert_eq!(ask(&mut server, &mut kernel, &alice, &read(0, 0)), Answered::Malformed);
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &read(0, MAX_SECTORS + 1)),
        Answered::Err(ErrorCode::TooMany)
    );
    assert_eq!(ask(&mut server, &mut kernel, &alice, &read(0, u32::MAX)), Answered::Err(ErrorCode::TooMany));

    // A write must be a whole number of sectors, and not empty.
    for len in [0usize, 1, SECTOR - 1, SECTOR + 1] {
        let data = vec![0; len];
        assert_eq!(
            ask(&mut server, &mut kernel, &alice, &Message::Write(Write { sector: 0, data: &data })),
            Answered::Malformed,
            "a write of {len} bytes"
        );
    }
    let data = vec![0; (MAX_SECTORS as usize + 1) * SECTOR];
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Write(Write { sector: 0, data: &data })),
        Answered::Err(ErrorCode::TooMany)
    );
}

/// A caller that lends less than its reply needs is told so before the disk is touched, rather
/// than after, with the answer thrown away.
#[test]
fn a_read_whose_reply_would_not_fit_the_lend_is_refused_before_the_disk_is_touched() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let before = device.notified();
    assert_eq!(
        ask_with_lend(&mut server, &mut kernel, &alice, &read(0, 8), 1024),
        Answered::Err(ErrorCode::TooMany)
    );
    assert_eq!(device.notified(), before, "the device was asked to do something");
    // Exactly enough: the data plus its `u32` length.
    assert!(matches!(
        ask_with_lend(&mut server, &mut kernel, &alice, &read(0, 8), 8 * SECTOR + 4),
        Answered::Data(_)
    ));
}

// ---------------------------------------------------------------- labels

/// A range carries no labels in milestone 1, so `check` lets any caller read one and only an
/// unlabelled caller write to it (IO-ARCHITECTURE.md).
#[test]
fn a_labelled_caller_may_read_but_not_write() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let vault = caller(FIRST_BADGE, 7, &[42]);
    assert!(matches!(ask(&mut server, &mut kernel, &vault, &Message::Info(Info {})), Answered::Info { .. }));
    assert!(matches!(ask(&mut server, &mut kernel, &vault, &read(0, 1)), Answered::Data(_)));
    let data = vec![0; SECTOR];
    assert_eq!(
        ask(&mut server, &mut kernel, &vault, &Message::Write(Write { sector: 0, data: &data })),
        Answered::Err(ErrorCode::NotPermitted)
    );
    assert_eq!(
        ask(&mut server, &mut kernel, &vault, &Message::Flush(Flush {})),
        Answered::Err(ErrorCode::NotPermitted)
    );
    assert_eq!(
        ask(&mut server, &mut kernel, &vault, &Message::Grant(Grant { sector: 0, count: 1 })),
        Answered::Err(ErrorCode::NotPermitted)
    );
}

// ---------------------------------------------------------------- granting and releasing

#[test]
fn a_grant_is_a_window_inside_the_callers_own_range() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let Answered::Granted(id, handles) =
        ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 100, count: 10 }))
    else {
        panic!("a grant inside the range");
    };
    assert_ne!(id, 0, "an id is never 0, which `release` takes to mean everything");
    assert_eq!(handles.len(), 1);
    assert_eq!(server.granted(), 1);

    // The granted badge is the next one the table would mint.
    let granted = caller(first_granted_badge(), 0, &[]);
    assert_eq!(
        ask(&mut server, &mut kernel, &granted, &Message::Info(Info {})),
        Answered::Info { sectors: 10, sector_size: 512, read_only: 0 }
    );
    // Sector 0 of the grant is sector 100 of Alice's range, which is LBA 164 of the disk.
    let mark = vec![0x5a; SECTOR];
    assert_eq!(
        ask(&mut server, &mut kernel, &granted, &Message::Write(Write { sector: 0, data: &mark })),
        Answered::Written
    );
    assert_eq!(device.sector(FIRST.first_lba + 100), mark);
    // And it cannot leave its window.
    assert_eq!(ask(&mut server, &mut kernel, &granted, &read(10, 1)), Answered::Err(ErrorCode::OutOfRange));
}

#[test]
fn a_grant_wider_than_the_callers_own_range_is_refused() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let cases: &[(u64, u64)] = &[
        (0, 1001),     // one sector more than the range
        (999, 2),      // starts inside, ends outside
        (1000, 1),     // starts at the end
        (0, 0),        // an empty window
        (u64::MAX, 1), // an offset that would overflow
        (1, u64::MAX), // a count that would overflow
        (0, u64::MAX), // the whole address space
    ];
    for (sector, count) in cases {
        assert_eq!(
            ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: *sector, count: *count })),
            Answered::Err(ErrorCode::NotPermitted),
            "grant {sector}..+{count}"
        );
    }
    assert_eq!(server.granted(), 0);
}

/// Grants never chain: a granted badge cannot grant another (QUESTIONS.md 141).
#[test]
fn only_a_root_badge_may_grant() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    assert!(matches!(
        ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 0, count: 100 })),
        Answered::Granted(..)
    ));
    let granted = caller(first_granted_badge(), 0, &[]);
    assert_eq!(
        ask(&mut server, &mut kernel, &granted, &Message::Grant(Grant { sector: 0, count: 1 })),
        Answered::Err(ErrorCode::NotPermitted)
    );
    assert_eq!(server.granted(), 1);
}

/// Only the holder of an id may release it, and an id nobody holds gets the same answer as one
/// somebody else holds, so nothing is revealed.
#[test]
fn a_release_of_an_id_the_caller_never_received_is_refused() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let bob = caller(SECOND_BADGE, 0, &[]);
    let Answered::Granted(id, _) =
        ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 0, count: 10 }))
    else {
        panic!("a grant");
    };
    assert_eq!(
        ask(&mut server, &mut kernel, &bob, &Message::Release(Release { id })),
        Answered::Err(ErrorCode::NotPermitted),
        "somebody else's id"
    );
    assert_eq!(
        ask(&mut server, &mut kernel, &bob, &Message::Release(Release { id: id ^ 1 })),
        Answered::Err(ErrorCode::NotPermitted),
        "an id nobody holds"
    );
    assert_eq!(server.granted(), 1);
    assert_eq!(ask(&mut server, &mut kernel, &alice, &Message::Release(Release { id })), Answered::Released);
    assert_eq!(server.granted(), 0);
    // A released badge names nothing, exactly as an unknown one does.
    let granted = caller(first_granted_badge(), 0, &[]);
    assert_eq!(
        ask(&mut server, &mut kernel, &granted, &Message::Info(Info {})),
        Answered::Err(ErrorCode::NotPermitted)
    );
}

/// `release(0)` frees everything the caller granted, which is what a restarted holder asks for.
#[test]
fn release_zero_frees_everything_the_caller_granted() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let bob = caller(SECOND_BADGE, 0, &[]);
    for i in 0..4u64 {
        assert!(matches!(
            ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: i * 10, count: 5 })),
            Answered::Granted(..)
        ));
    }
    assert!(matches!(
        ask(&mut server, &mut kernel, &bob, &Message::Grant(Grant { sector: 0, count: 5 })),
        Answered::Granted(..)
    ));
    assert_eq!(server.granted(), 5);
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Release(Release { id: 0 })),
        Answered::Released
    );
    assert_eq!(server.granted(), 1, "only the caller's own grants went");
}

/// A client at its cap is refused, and its slot comes back when it releases. The bucket is per
/// (account, label set), with account 0 keyed by badge, so one client filling its share does not
/// stop another.
#[test]
fn a_client_at_its_cap_is_refused_and_gets_its_slots_back() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    let bob = caller(SECOND_BADGE, 0, &[]);
    let mut ids = Vec::new();
    for i in 0..LIMITS.state as u64 {
        let Answered::Granted(id, _) =
            ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: i, count: 1 }))
        else {
            panic!("grant {i} inside the cap")
        };
        ids.push(id);
    }
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 900, count: 1 })),
        Answered::Err(ErrorCode::TooMany)
    );
    // Bob has his own bucket.
    assert!(matches!(
        ask(&mut server, &mut kernel, &bob, &Message::Grant(Grant { sector: 0, count: 1 })),
        Answered::Granted(..)
    ));
    assert_eq!(
        server.admission().held(AdmitKey::of(&alice), Resource::State),
        LIMITS.state,
        "Alice's bucket is full"
    );
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Release(Release { id: ids[0] })),
        Answered::Released
    );
    assert!(matches!(
        ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 900, count: 1 })),
        Answered::Granted(..)
    ));
}

/// A kernel that will not mint gives back the admission slot it took, so a client cannot fill its
/// own bucket by asking for grants that cannot be made.
#[test]
fn a_grant_the_kernel_refuses_costs_nothing() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    kernel.mint_fails = true;
    let alice = caller(FIRST_BADGE, 0, &[]);
    for _ in 0..LIMITS.state as usize * 4 {
        assert_eq!(
            ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 0, count: 1 })),
            Answered::Err(ErrorCode::Failed)
        );
    }
    assert_eq!(server.admission().held(AdmitKey::of(&alice), Resource::State), 0);
    kernel.mint_fails = false;
    assert!(matches!(
        ask(&mut server, &mut kernel, &alice, &Message::Grant(Grant { sector: 0, count: 1 })),
        Answered::Granted(..)
    ));
}

// ---------------------------------------------------------------- malformed requests

/// Words and bytes no encoder would produce. None of them may do anything but `malformed`.
#[test]
fn malformed_requests_are_refused() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
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
        assert_eq!(ask_raw(&mut server, &mut kernel, &alice, words, body), Answered::Malformed, "{why}");
    }
    assert!(!server.disk().is_broken(), "a malformed request never touches the device");
}

/// Arbitrary bytes as a request, from every badge a caller could hold. The assertion is that it
/// answers something and never panics; the fuzz target is the same body under coverage.
#[test]
fn arbitrary_requests_never_panic() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
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
            2 => first_granted_badge(),
            _ => next(),
        };
        let who = caller(badge, next() % 3, &[]);
        let _ = ask_raw(&mut server, &mut kernel, &who, &words, &body);
    }
    assert_eq!(device.strayed(), 0);
}

// ---------------------------------------------------------------- the device underneath

/// A device that lies is a `failed` to the client, and stays failed: `blkd` says no more than
/// that, and never answers a read with bytes it did not get.
#[test]
fn a_lying_device_becomes_failed_and_stays_failed() {
    let device = device();
    let mut server = server(&device);
    let mut kernel = FakeKernel::new();
    let alice = caller(FIRST_BADGE, 0, &[]);
    device.set_policy(Policy { defer: true, used_id: Some(3), ..Policy::default() });
    assert_eq!(ask(&mut server, &mut kernel, &alice, &read(0, 1)), Answered::Err(ErrorCode::Failed));
    device.set_policy(Policy { defer: true, ..Policy::default() });
    assert_eq!(ask(&mut server, &mut kernel, &alice, &read(0, 1)), Answered::Err(ErrorCode::Failed));
    assert_eq!(
        ask(&mut server, &mut kernel, &alice, &Message::Flush(Flush {})),
        Answered::Err(ErrorCode::Failed)
    );
    // Everything that does not touch the device still answers.
    assert!(matches!(ask(&mut server, &mut kernel, &alice, &Message::Info(Info {})), Answered::Info { .. }));
    assert_eq!(device.strayed(), 0);
}

/// Limits that would not fit the budget are refused where a build-time mistake can still be
/// caught.
#[test]
fn limits_that_do_not_fit_the_budget_are_refused() {
    let device = device();
    let mut disk = Disk::new(&device).expect("bring-up");
    let roots = crate::read_partitions(&mut disk).expect("a table");
    let too_big = Limits { buckets: 1 << 20, in_flight: 0, files: 0, state: 1 << 20 };
    assert!(BlockServer::new(disk, roots, too_big, &COST, BUDGET, TEST_RANDOM).is_err());
}

/// The shipped limits do fit, and leave the open-call headroom.
#[test]
fn the_shipped_limits_fit_the_shipped_budget() {
    assert!(LIMITS.fits(&COST, BUDGET));
    assert_eq!(LIMITS.in_flight, 0, "blkd parks no call");
}
