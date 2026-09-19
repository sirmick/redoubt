//! Host tests: every call, result, error and record round-trips, every encoding fits rv32's
//! registers, and decoding rejects malformed input (and never panics on random input).

extern crate std;
use core::num::{NonZeroU64, NonZeroUsize};
use std::string::String;
use std::vec::Vec;

use crate::*;

const BIG: u64 = 0x1234_5678_9abc_def0; // a u64 whose halves differ, to catch a swapped pair

fn h(index: u32) -> Handle { Handle::new(index).unwrap() }

fn pages(addr: usize, npages: usize) -> Pages { Pages { addr, npages: NonZeroUsize::new(npages).unwrap() } }

fn badge(value: u64) -> NonZeroU64 { NonZeroU64::new(value).unwrap() }

const CALLS: u64 = Number::ALL.len() as u64;
const ERRORS: u64 = Error::ALL.len() as u64;

/// At least one of every call, with every optional argument both present and absent. Addresses
/// fit in 32 bits so the encodings are exactly rv32's.
fn sample_calls() -> Vec<Call> {
    let rw = MemFlags::READ | MemFlags::WRITE;
    std::vec![
        Call::MapAnon { len: 0x3000, flags: rw },
        Call::Unmap { addr: 0x2000_0000, len: 0x1000 },
        Call::SetFlags { addr: 0x2000_0000, len: 0x1000, flags: MemFlags::READ | MemFlags::EXECUTE },
        Call::MapDevice { device: h(3) },
        Call::DmaAlloc { device: h(4), npages: 2 },
        Call::ThreadCreate { entry: 0x1_0000, sp: 0x7fff_f000, arg: usize::MAX >> 32 },
        Call::ThreadExit,
        Call::ProcessExit { code: u32::MAX },
        Call::ProcessCreate { budget: h(1), exit_endpoint: h(2) },
        Call::ProcessMap { process: h(5), src: 0x2000_0000, dst: 0x1000, len: 0x4000, flags: MemFlags::NONE },
        Call::ProcessStart { process: h(5), entry: 0x1000, sp: 0x8000_0000, handles_rec: 0x3000, count: 7 },
        Call::EndpointCreate,
        Call::Mint { source: MintSource::Message(BIG), badge: badge(!BIG), budget: None },
        Call::Mint { source: MintSource::Handle(h(u32::MAX - 1)), badge: badge(1), budget: Some(h(0)) },
        Call::Call { endpoint: h(6), body_rec: 0x5000, lend: None, timeout: FOREVER },
        Call::Call {
            endpoint: h(6),
            body_rec: 0x5000,
            lend: Some(pages(0x6000, MAX_LEND_PAGES)),
            timeout: BIG
        },
        Call::Send { endpoint: h(7), body_rec: 0x5000, transfer: None, timeout: 0 },
        Call::Send { endpoint: h(7), body_rec: 0x5000, transfer: Some(pages(0x6000, 3)), timeout: 10 },
        Call::Receive { from: Some(h(0)), timeout: BIG, max_transfer: 16, received_rec: 0x7000 },
        Call::Receive { from: None, timeout: SLICE, max_transfer: 0, received_rec: 0x7000 },
        Call::Reply { msg_id: BIG, body_rec: 0x5000 },
        Call::HandleClose { handle: h(9) },
        Call::BudgetCreate { parent: h(1), spec_rec: 0x8000 },
        Call::BudgetDestroy { budget: h(10) },
        Call::BudgetUsage { budget: h(11) },
        Call::TimeNow,
        Call::Random { bytes: 0x9001, len: MAX_RANDOM },
        Call::SystemReset { device: h(12), kind: ResetKind::PowerOff },
        Call::SystemReset { device: h(12), kind: ResetKind::Reboot },
    ]
}

/// A successful result of the right shape for `number` (several where the shape has variety).
fn sample_returns(number: Number) -> Vec<Return> {
    match number {
        Number::MapAnon | Number::MapDevice => std::vec![Return::Addr(0), Return::Addr(0xffff_f000)],
        Number::DmaAlloc => std::vec![Return::Dma { addr: 0x2000_0000, phys: BIG }],
        Number::ThreadCreate => std::vec![Return::Tid(MAX_THREADS as u32)],
        Number::ProcessCreate | Number::EndpointCreate | Number::Mint | Number::BudgetCreate => {
            std::vec![Return::Handle(h(0)), Return::Handle(h(u32::MAX - 1))]
        }
        Number::BudgetUsage => std::vec![Return::Usage(Usage {
            pages_limit: BIG,
            pages_usage: !BIG,
            processes_limit: 40,
            processes_usage: u32::MAX,
        })],
        Number::TimeNow => std::vec![Return::Time(0), Return::Time(BIG)],
        _ => std::vec![Return::Nothing],
    }
}

/// Every register holds at most 32 bits, so the encoding is valid on rv32 too.
fn fits_rv32(regs: &[u64; REGS]) -> bool { regs.iter().all(|r| *r <= u64::from(u32::MAX)) }

#[test]
fn every_call_round_trips() {
    let calls = sample_calls();
    for number in Number::ALL {
        assert!(calls.iter().any(|c| c.number() == number), "no sample for {number:?}");
    }
    for call in calls {
        let regs = call.encode();
        assert!(fits_rv32(&regs), "{call:?} encoded as {regs:?}");
        assert_eq!(regs[0], call.number() as u64);
        assert_eq!(Call::decode(&regs), Ok(call), "{call:?} encoded as {regs:?}");
        // Every register matters: changing any one changes the call or breaks the encoding.
        for i in 1..REGS {
            let mut bad = regs;
            bad[i] ^= 1;
            assert_ne!(Call::decode(&bad), Ok(call), "{call:?} ignores a{i}");
        }
    }
}

#[test]
fn every_result_and_error_round_trips() {
    for number in Number::ALL {
        for value in sample_returns(number) {
            let regs = encode_result(&Ok(value));
            assert!(fits_rv32(&regs), "{value:?} encoded as {regs:?}");
            assert_eq!(regs[0], 0);
            assert_eq!(decode_result(number, &regs), Ok(value), "{value:?} encoded as {regs:?}");
        }
        for error in Error::ALL {
            let regs = encode_result(&Err(error));
            assert_eq!(regs, [error as u64, 0, 0, 0, 0, 0, 0, 0]);
            assert_eq!(decode_result(number, &regs), Err(error));
        }
    }
}

fn snake_case(camel: &str) -> String {
    let mut snake = String::new();
    for c in camel.chars() {
        if c.is_ascii_uppercase() && !snake.is_empty() {
            snake.push('_');
        }
        snake.push(c.to_ascii_lowercase());
    }
    snake
}

#[test]
fn numbers_and_codes_are_dense_from_one() {
    for (i, number) in Number::ALL.iter().enumerate() {
        assert_eq!(*number as u64, i as u64 + 1);
        assert_eq!(Number::from_raw(*number as u64), Some(*number));
        // The spec's name, typed once in the table, agrees with the variant's.
        assert_eq!(number.name(), snake_case(&std::format!("{number:?}")));
    }
    assert_eq!(Number::from_raw(0), None);
    assert_eq!(Number::from_raw(CALLS + 1), None);
    for (i, error) in Error::ALL.iter().enumerate() {
        assert_eq!(*error as u64, i as u64 + 1);
        assert_eq!(Error::from_code(*error as u64), Some(*error));
    }
    assert_eq!(Error::from_code(0), None);
    assert_eq!(Error::from_code(ERRORS + 1), None);
}

fn body(nhandles: u32) -> Body {
    let handles: Vec<Handle> = (0..nhandles).map(|i| h(i * 7)).collect();
    Body { words: [1, usize::MAX >> 32, 0, 42], handles: Handles::from_slice(&handles).unwrap() }
}

fn labels(n: u64) -> Labels { Labels::from_slice(&(0..n).map(|i| BIG ^ i).collect::<Vec<_>>()).unwrap() }

fn sample_received() -> Vec<Received> {
    let message = |buffer| {
        Received::Message(Message {
            msg_id: BIG,
            badge: 1,
            account: !BIG,
            labels: labels(3),
            body: body(2),
            buffer,
        })
    };
    std::vec![
        message(None),
        message(Some(Buffer::Lend(pages(0x6000, MAX_LEND_PAGES)))),
        message(Some(Buffer::Transfer(pages(0x6000, 1)))),
        Received::Message(Message {
            msg_id: 0,
            badge: u64::MAX,
            account: 0,
            labels: labels(MAX_LABELS as u64),
            body: body(MAX_MSG_HANDLES as u32),
            buffer: None,
        }),
        Received::Interrupt(h(8)),
        Received::Exit(ExitNotice { pid: 3, cause: Cause::Exited, code: 0, blamed_account: 0 }),
        Received::Exit(ExitNotice { pid: 4, cause: Cause::Faulted, code: u32::MAX, blamed_account: BIG }),
        Received::Exit(ExitNotice { pid: 5, cause: Cause::Killed, code: 1, blamed_account: 7 }),
    ]
}

fn sample_specs() -> Vec<BudgetSpec> {
    std::vec![
        BudgetSpec {
            pages: 0,
            processes: 0,
            weight: 0,
            class: Class::User,
            labels: Labels::new(),
            account: 0,
            deadline: FOREVER,
        },
        BudgetSpec {
            pages: BIG,
            processes: 4,
            weight: 20,
            class: Class::System,
            labels: labels(MAX_LABELS as u64),
            account: !BIG,
            deadline: BIG,
        },
    ]
}

#[test]
fn records_round_trip() {
    for n in 0..=MAX_MSG_HANDLES as u32 {
        let b = body(n);
        assert_eq!(Body::decode(&b.encode()), Ok(b));
    }
    for r in sample_received() {
        assert_eq!(Received::decode(&r.encode()), Ok(r), "{r:?}");
    }
    for s in sample_specs() {
        assert_eq!(BudgetSpec::decode(&s.encode()), Ok(s));
    }
    assert!(Class::User < Class::System);
}

#[test]
fn lists_hold_at_most_their_capacity() {
    let five = [h(1); MAX_MSG_HANDLES + 1];
    assert_eq!(Handles::from_slice(&five), Err(Error::TooLarge));
    let mut list = Handles::from_slice(&five[..MAX_MSG_HANDLES]).unwrap();
    assert_eq!(list.as_slice(), &five[..MAX_MSG_HANDLES]);
    assert_eq!(list.push(h(1)), Err(Error::TooLarge));
}

/// Every way a call's registers can be malformed is refused with the right error.
#[test]
fn malformed_calls_are_refused() {
    let decode = |regs: [u64; REGS]| Call::decode(&regs);
    let wide = 1 << 32; // too wide for a 32-bit field (only rv64 registers can hold it)
    assert_eq!(decode([0; REGS]), Err(Error::InvalidArgument), "call number 0");
    assert_eq!(decode([CALLS + 1, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown call");
    let map_anon = Number::MapAnon as u64;
    assert_eq!(decode([map_anon, 0x1000, 8, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown flag");
    assert_eq!(
        decode([map_anon, 0x1000, wide | 1, 0, 0, 0, 0, 0]),
        Err(Error::InvalidArgument),
        "wide flags"
    );
    assert_eq!(decode([map_anon, 0x1000, 6, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "W+X");
    assert_eq!(decode([map_anon, 0x1000, 7, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "RW+X");
    let random = Number::Random as u64;
    let too_many = MAX_RANDOM as u64 + 1;
    assert_eq!(decode([random, 0x9000, too_many, 0, 0, 0, 0, 0]), Err(Error::TooLarge), "random len");
    let call = Number::Call as u64;
    assert_eq!(decode([call, 6, 0x5000, 0x6000, 0, 0, 0, 0]), Err(Error::InvalidArgument), "lend of 0 pages");
    assert_eq!(decode([call, 6, 0x5000, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "lend at 0");
    let exit = Number::ProcessExit as u64;
    assert_eq!(decode([exit, wide, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "wide code");
    let close = Number::HandleClose as u64;
    assert_eq!(decode([close, u32::MAX.into(), 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle), "reserved handle");
    assert_eq!(decode([close, wide, 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle), "wide handle");
    let mint = Number::Mint as u64;
    assert_eq!(decode([mint, 1, 5, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "badge 0");
    assert_eq!(decode([mint, 3, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "unknown mint source");
    assert_eq!(decode([mint, 0, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "mint source 0");
    assert_eq!(decode([mint, 2, wide, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "wide half");
    assert_eq!(decode([mint, 2, 0, 1, 1, 0, 0, 0]), Err(Error::BadHandle), "mint source handle");
    assert_eq!(
        decode([mint, 1, 5, 0, 1, 0, u32::MAX.into(), 0]),
        Ok(Call::Mint { source: MintSource::Message(5), badge: badge(1), budget: None })
    );
    let reset = Number::SystemReset as u64;
    assert_eq!(decode([reset, 1, 3, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown reset kind");
    assert_eq!(decode([reset, 1, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "reset kind 0");
}

#[test]
fn malformed_results_are_refused() {
    let regs = encode_result(&Ok(Return::Addr(0x1000)));
    assert_eq!(decode_result(Number::Unmap, &regs), Err(Error::InvalidArgument), "wrong shape");
    assert_eq!(decode_result(Number::Unmap, &[ERRORS + 1, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::Unmap, &[1, 1, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(
        decode_result(Number::ThreadCreate, &[0, 1 << 32, 0, 0, 0, 0, 0, 0]),
        Err(Error::InvalidArgument)
    );
    let none = u64::from(u32::MAX);
    assert_eq!(decode_result(Number::Mint, &[0, none, 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle));
}

#[test]
fn malformed_records_are_refused() {
    // Body: too many handles, a stray handle slot, a reserved handle.
    let mut slots = body(1).encode();
    slots[WORDS] = MAX_MSG_HANDLES as u64 + 1;
    assert_eq!(Body::decode(&slots), Err(Error::TooLarge));
    let mut slots = body(1).encode();
    slots[WORDS + 2] = 9;
    assert_eq!(Body::decode(&slots), Err(Error::InvalidArgument));
    let mut slots = body(1).encode();
    slots[WORDS + 1] = u32::MAX.into();
    assert_eq!(Body::decode(&slots), Err(Error::BadHandle));

    // BudgetSpec: unknown class, too many labels, a stray label slot, wide processes.
    let spec = sample_specs()[0];
    for (slot, value, error) in [
        (3, 0, Error::InvalidArgument),
        (3, 3, Error::InvalidArgument),
        (4, MAX_LABELS as u64 + 1, Error::TooLarge),
        (5, 1, Error::InvalidArgument),
        (1, 1 << 32, Error::InvalidArgument),
        (2, 1 << 32, Error::InvalidArgument),
    ] {
        let mut slots = spec.encode();
        slots[slot] = value;
        assert_eq!(BudgetSpec::decode(&slots), Err(error), "spec slot {slot} = {value}");
    }

    // Received: unknown kinds, stray slots after each kind, a buffer kind 0 with an address.
    for kind in [0, 4] {
        let mut slots = [0; RECEIVED_SLOTS];
        slots[0] = kind;
        assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument));
    }
    let mut slots = Received::Interrupt(h(1)).encode();
    slots[2] = 1;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument));
    let exit = Received::Exit(ExitNotice { pid: 1, cause: Cause::Killed, code: 0, blamed_account: 0 });
    let mut slots = exit.encode();
    slots[2] = 4;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "unknown cause");
    let mut slots = sample_received()[0].encode();
    slots[RECEIVED_SLOTS - 2] = 0x6000;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "no buffer, but an address");
    let mut slots = sample_received()[0].encode();
    slots[RECEIVED_SLOTS - 3] = 3;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "unknown buffer kind");
    let mut slots = sample_received()[1].encode();
    slots[RECEIVED_SLOTS - 1] = 0;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "a lend of 0 pages");
    let mut slots = sample_received()[1].encode();
    slots[RECEIVED_SLOTS - 3] = 0;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "pages, but no buffer");
}

/// xorshift64: deterministic, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Mostly small values, so random input often decodes and the canonical check has work.
    fn value(&mut self) -> u64 {
        match self.next() % 4 {
            0 => 0,
            1 => self.next() % 8,
            2 => u64::from(u32::MAX) - self.next() % 2,
            _ => self.next(),
        }
    }
}

/// Decoding random registers never panics, and whatever decodes has exactly one encoding: the
/// registers it came from.
#[test]
fn random_registers() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..200_000 {
        let mut regs = [0; REGS].map(|_| rng.value());
        regs[0] = rng.next() % (CALLS + 2);
        if let Ok(call) = Call::decode(&regs) {
            assert_eq!(call.encode(), regs);
        }
        let number = Number::ALL[(rng.next() % CALLS) as usize];
        regs[0] = rng.next() % (ERRORS + 2);
        if let Ok(value) = decode_result(number, &regs) {
            assert_eq!(encode_result(&Ok(value)), regs);
        }
    }
}

#[test]
fn random_records() {
    let mut rng = Rng(0xd1b5_4a32_d192_ed03);
    for _ in 0..200_000 {
        let mut body = [0; BODY_SLOTS].map(|_| rng.value());
        body[WORDS] %= 6;
        if let Ok(b) = Body::decode(&body) {
            assert_eq!(b.encode(), body);
        }
        let mut spec = [0; BUDGET_SPEC_SLOTS].map(|_| rng.value());
        spec[3] %= 3;
        spec[4] %= 10;
        if let Ok(s) = BudgetSpec::decode(&spec) {
            assert_eq!(s.encode(), spec);
        }
        let mut received = [0; RECEIVED_SLOTS].map(|_| rng.value());
        received[0] %= 4;
        if let Ok(r) = Received::decode(&received) {
            assert_eq!(r.encode(), received);
        }
    }
}
