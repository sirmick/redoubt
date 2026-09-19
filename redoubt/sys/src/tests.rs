//! Host tests: every call, result, error and buffer round-trips on both register widths, and
//! decoding rejects malformed input (and never panics on random input).

extern crate std;
use std::format;
use std::vec::Vec;

use crate::regs::Writer;
use crate::*;

const BIG: u64 = 0x1234_5678_9abc_def0; // a u64 whose halves differ, to catch a swapped pair

fn h(index: u32) -> Handle { Handle::new(index).unwrap() }

fn pages(addr: usize, npages: usize) -> Option<Pages> { Some(Pages { addr, npages }) }

/// At least one of every call, with every optional argument both present and absent. Addresses
/// fit in 32 bits so the 32-bit encodings can hold them.
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
        Call::ProcessStart { process: h(5), entry: 0x1000, sp: 0x8000_0000, handles_buf: 0x3000, count: 7 },
        Call::EndpointCreate,
        Call::Mint { source: MintSource::Message(BIG), badge: !BIG, budget: None },
        Call::Mint { source: MintSource::Handle(h(u32::MAX - 1)), badge: 1, budget: Some(h(0)) },
        Call::Call { endpoint: h(6), body_buf: 0x5000, lend: None, timeout: FOREVER },
        Call::Call { endpoint: h(6), body_buf: 0x5000, lend: pages(0x6000, MAX_LEND_PAGES), timeout: BIG },
        Call::Send { endpoint: h(7), body_buf: 0x5000, transfer: None, timeout: 0 },
        Call::Send { endpoint: h(7), body_buf: 0x5000, transfer: pages(0x6000, 3), timeout: 10 },
        Call::Receive { from: Some(h(0)), timeout: BIG, max_transfer: 16, record: 0x7000 },
        Call::Receive { from: None, timeout: SLICE, max_transfer: 0, record: 0x7000 },
        Call::Reply { msg_id: BIG, body_buf: 0x5000 },
        Call::HandleClose { handle: h(9) },
        Call::BudgetCreate { parent: h(1), spec_buf: 0x8000 },
        Call::BudgetDestroy { budget: h(10) },
        Call::BudgetUsage { budget: h(11) },
        Call::TimeNow,
        Call::Random { buf: 0x9000, len: 64 },
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
            pages_used: !BIG,
            processes_limit: 40,
            processes_used: u32::MAX,
        })],
        Number::TimeNow => std::vec![Return::Time(0), Return::Time(BIG)],
        _ => std::vec![Return::Nothing],
    }
}

fn check_calls<R: Register>() {
    let calls = sample_calls();
    for number in Number::ALL {
        assert!(calls.iter().any(|c| c.number() == number), "no sample for {}", number.name());
    }
    for call in calls {
        let mut regs = [R::ZERO; REGS];
        let mut w = Writer::new(&mut regs);
        call.write(&mut w);
        assert!(w.used() <= REGS, "{call:?} needs {} registers", w.used());
        assert_eq!(regs, call.encode::<R>());
        assert_eq!(regs[0].widen(), call.number() as u64);
        assert_eq!(Call::decode(&regs), Ok(call), "{call:?} encoded as {regs:?}");
    }
}

fn check_results<R: Register>() {
    for number in Number::ALL {
        for value in sample_returns(number) {
            let mut regs = [R::ZERO; REGS];
            let mut w = Writer::new(&mut regs);
            crate::ret::write_result(&Ok(value), &mut w);
            assert!(w.used() <= REGS, "{value:?} needs {} registers", w.used());
            assert_eq!(regs[0], R::ZERO);
            assert_eq!(decode_result(number, &regs), Ok(value), "{value:?} encoded as {regs:?}");
        }
        for error in Error::ALL {
            let regs = encode_result::<R>(&Err(error));
            assert_eq!(regs[0].widen(), u64::from(error.code()));
            assert!(regs[1..].iter().all(|r| *r == R::ZERO));
            assert_eq!(decode_result(number, &regs), Err(error));
        }
    }
}

#[test]
fn every_call_round_trips_64() { check_calls::<u64>() }

#[test]
fn every_call_round_trips_32() { check_calls::<u32>() }

#[test]
fn every_result_and_error_round_trips_64() { check_results::<u64>() }

#[test]
fn every_result_and_error_round_trips_32() { check_results::<u32>() }

#[test]
fn names_match_the_spec() {
    let calls = [
        "map_anon",
        "unmap",
        "set_flags",
        "map_device",
        "dma_alloc",
        "thread_create",
        "thread_exit",
        "process_exit",
        "process_create",
        "process_map",
        "process_start",
        "endpoint_create",
        "mint",
        "call",
        "send",
        "receive",
        "reply",
        "handle_close",
        "budget_create",
        "budget_destroy",
        "budget_usage",
        "time_now",
        "random",
        "system_reset",
    ];
    let names: Vec<_> = Number::ALL.iter().map(|n| n.name()).collect();
    assert_eq!(names, calls);
    for (i, number) in Number::ALL.iter().enumerate() {
        assert_eq!(*number as u64, i as u64 + 1);
        assert_eq!(Number::from_raw(*number as u64), Some(*number));
    }
    assert_eq!(Number::from_raw(0), None);
    assert_eq!(Number::from_raw(Number::ALL.len() as u64 + 1), None);

    let errors = [
        "BadHandle",
        "WrongObject",
        "InvalidArgument",
        "OutOfMemory",
        "OutOfProcesses",
        "TooManyThreads",
        "NotPermitted",
        "ClassDenied",
        "LabelDenied",
        "Busy",
        "Refused",
        "TooLarge",
        "Timeout",
        "Dead",
    ];
    let names: Vec<_> = Error::ALL.iter().map(|e| format!("{e:?}")).collect();
    assert_eq!(names, errors);
    for (i, error) in Error::ALL.iter().enumerate() {
        assert_eq!(error.code(), i as u32 + 1);
        assert_eq!(Error::from_code(error.code().into()), Some(*error));
    }
    assert_eq!(Error::from_code(0), None);
    assert_eq!(Error::from_code(Error::ALL.len() as u64 + 1), None);
}

#[test]
fn constants_match_the_spec() {
    assert_eq!(
        (WORDS, MAX_MSG_HANDLES, MAX_LEND_PAGES, MAX_THREADS, MAX_LABELS, MAX_DEPTH, WAIT_CAP),
        (4, 4, 16, 31, 8, 8, 16)
    );
    assert_eq!((STRIDE, SLICE, FOREVER), (1 << 20, 10_000, u64::MAX));
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
        message(Some(Buffer::Lend(Pages { addr: 0x6000, npages: MAX_LEND_PAGES }))),
        message(Some(Buffer::Transfer(Pages { addr: 0x6000, npages: 1 }))),
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
fn buffers_round_trip() {
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
fn check_malformed_calls<R: Register>() {
    let t = |v: u64| R::truncate(v);
    let decode = |regs: [u64; REGS]| Call::decode(&regs.map(t));
    assert_eq!(decode([0; REGS]), Err(Error::InvalidArgument), "call number 0");
    assert_eq!(decode([25, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown call");
    // A non-zero register after the last argument, for every sample call.
    for call in sample_calls() {
        let mut regs = [R::ZERO; REGS];
        let mut w = Writer::new(&mut regs);
        call.write(&mut w);
        for unused in w.used()..REGS {
            let mut bad = regs;
            bad[unused] = t(1);
            assert_eq!(Call::decode(&bad), Err(Error::InvalidArgument), "{call:?} with a{unused} set");
        }
    }
    let map_anon = Number::MapAnon as u64;
    assert_eq!(decode([map_anon, 0x1000, 8, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown flag");
    let close = Number::HandleClose as u64;
    assert_eq!(decode([close, u32::MAX.into(), 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle), "reserved handle");
    let mint = Number::Mint as u64;
    assert_eq!(decode([mint, 3, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "unknown mint source");
    assert_eq!(decode([mint, 0, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "mint source 0");
    let reset = Number::SystemReset as u64;
    assert_eq!(decode([reset, 1, 3, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown reset kind");
    assert_eq!(decode([reset, 1, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "reset kind 0");
}

#[test]
fn malformed_calls_are_refused_64() {
    check_malformed_calls::<u64>();
    // Only 64-bit registers can hold values too wide for a 32-bit field.
    let wide = 1 << 32;
    let exit = Number::ProcessExit as u64;
    assert_eq!(Call::decode(&[exit, wide, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    let close = Number::HandleClose as u64;
    assert_eq!(Call::decode(&[close, wide, 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle));
    let mint = Number::Mint as u64;
    assert_eq!(Call::decode(&[mint, 2, wide, 1, 0, 0, 0, 0]), Err(Error::BadHandle), "mint source handle");
    assert_eq!(Call::decode(&[mint, 1, 5, 1, wide, 0, 0, 0]), Err(Error::BadHandle), "mint budget");
    let map_anon = Number::MapAnon as u64;
    assert_eq!(Call::decode(&[map_anon, 0x1000, wide | 1, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
}

#[test]
fn malformed_calls_are_refused_32() { check_malformed_calls::<u32>() }

#[test]
fn malformed_results_are_refused() {
    let regs = encode_result::<u64>(&Ok(Return::Addr(0x1000)));
    assert_eq!(decode_result(Number::Unmap, &regs), Err(Error::InvalidArgument), "wrong shape");
    assert_eq!(decode_result(Number::Unmap, &[15u64, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::Unmap, &[1u64, 1, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(
        decode_result(Number::ThreadCreate, &[0u64, 1 << 32, 0, 0, 0, 0, 0, 0]),
        Err(Error::InvalidArgument)
    );
    let none = u64::from(u32::MAX);
    assert_eq!(decode_result(Number::Mint, &[0u64, none, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
}

#[test]
fn malformed_buffers_are_refused() {
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
fn check_random_registers<R: Register>(seed: u64) {
    let mut rng = Rng(seed);
    for _ in 0..200_000 {
        let mut regs = [R::ZERO; REGS].map(|_| R::truncate(rng.value()));
        regs[0] = R::truncate(rng.next() % 26);
        if let Ok(call) = Call::decode(&regs) {
            assert_eq!(call.encode::<R>(), regs);
        }
        let number = Number::ALL[(rng.next() % 24) as usize];
        regs[0] = R::truncate(rng.next() % 16);
        if let Ok(value) = decode_result(number, &regs) {
            assert_eq!(encode_result::<R>(&Ok(value)), regs);
        }
    }
}

#[test]
fn random_registers_64() { check_random_registers::<u64>(0x9e37_79b9_7f4a_7c15) }

#[test]
fn random_registers_32() { check_random_registers::<u32>(0x2545_f491_4f6c_dd1d) }

#[test]
fn random_buffers() {
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
