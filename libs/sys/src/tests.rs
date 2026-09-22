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

fn nz(value: u64) -> NonZeroU64 { NonZeroU64::new(value).unwrap() }

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
        Call::ProcessStart {
            process: h(5),
            entry: 0x1000,
            sp: 0x8000_0000,
            arg: 0x4000,
            handles_rec: 0x3000,
            count: 7
        },
        Call::ProcessStart {
            process: h(5),
            entry: 0x1000,
            sp: 0x8000_0000,
            arg: 0,
            handles_rec: 0,
            count: 0
        },
        Call::EndpointCreate,
        Call::Mint { source: MintSource::Message(nz(BIG)), badge: nz(!BIG), budget: None },
        Call::Mint { source: MintSource::Handle(h(u32::MAX)), badge: nz(1), budget: Some(h(1)) },
        Call::Mint { source: MintSource::Message(nz(1)), badge: nz(u64::MAX), budget: Some(h(u32::MAX)) },
        Call::Call { endpoint: h(6), body_rec: 0x5000, lend: None, timeout: FOREVER },
        Call::Call {
            endpoint: h(6),
            body_rec: 0x5000,
            lend: Some(pages(0x6000, MAX_LEND_PAGES)),
            timeout: BIG
        },
        Call::Send { endpoint: h(7), body_rec: 0x5000, transfer: None, timeout: 0 },
        Call::Send { endpoint: h(7), body_rec: 0x5000, transfer: Some(pages(0x6000, 3)), timeout: 10 },
        Call::Receive { from: Some(h(1)), timeout: BIG, max_transfer: 16, received_rec: 0x7000 },
        Call::Receive { from: None, timeout: SLICE, max_transfer: 0, received_rec: 0x7000 },
        Call::Reply { msg_id: nz(BIG), body_rec: 0x5000 },
        Call::Serve { msg_id: nz(BIG) },
        Call::Serve { msg_id: nz(u64::MAX) },
        Call::HandleClose { handle: h(9) },
        Call::BudgetCreate { parent: h(1), spec_rec: 0x8000 },
        Call::BudgetDestroy { budget: h(10) },
        Call::BudgetUsage { budget: h(11), usage_rec: 0xa000 },
        Call::TimeNow,
        Call::Random,
        Call::SystemReset { device: h(12), kind: ResetKind::PowerOff },
        Call::SystemReset { device: h(12), kind: ResetKind::Reboot },
    ]
}

/// A successful result of the right shape for `number` (several where the shape has variety).
fn sample_returns(number: Number) -> Vec<Return> {
    match number {
        Number::Call => std::vec![Return::Call(CallOutcome {
            status: Ok(()),
            lend: LendDisposition::Returned,
            reply_present: true,
        })],
        Number::Reply => std::vec![
            Return::Reply(ReplyOutcome { delivered: false, installed: 0 }),
            Return::Reply(ReplyOutcome { delivered: true, installed: 5 }),
        ],
        Number::MapAnon => std::vec![Return::Addr(0), Return::Addr(0xffff_f000)],
        Number::MapDevice => {
            std::vec![Return::Mapping { addr: 0, len: 0 }, Return::Mapping { addr: 0xffff_f000, len: 0x1000 },]
        }
        Number::DmaAlloc => std::vec![Return::Dma { addr: 0x2000_0000, phys: BIG }],
        Number::ThreadCreate => std::vec![Return::Tid(MAX_THREADS as u32)],
        Number::ProcessCreate | Number::EndpointCreate | Number::Mint | Number::BudgetCreate => {
            std::vec![Return::Handle(h(1)), Return::Handle(h(u32::MAX))]
        }
        Number::TimeNow => std::vec![Return::Time(0), Return::Time(BIG)],
        Number::Random => std::vec![Return::Random(0), Return::Random(BIG), Return::Random(u64::MAX)],
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
            if number == Number::Call {
                let value = Return::Call(CallOutcome {
                    status: Err(error),
                    lend: LendDisposition::Returned,
                    reply_present: false,
                });
                assert_eq!(decode_result(number, &encode_result(&Ok(value))), Ok(value));
                continue;
            }
            let regs = encode_result(&Err(error));
            assert_eq!(regs, [error as u64, 0, 0, 0, 0, 0, 0, 0]);
            assert_eq!(decode_result(number, &regs), Err(error));
        }
    }
}

#[test]
fn ipc_outcomes_round_trip_and_reject_impossible_combinations() {
    // Independent allowed rows from KERNEL-SPEC's IPC completion table, not the decoder's
    // Boolean predicate: rejection retains memory; normal/partial reply commits a record;
    // only taken-call Timeout/Dead consumes memory. Each returning row also has a no-lend form.
    let mut allowed = Vec::new();
    for lend in [LendDisposition::None, LendDisposition::Returned] {
        for error in Error::ALL {
            allowed.push(Return::Call(CallOutcome { status: Err(error), lend, reply_present: false }));
        }
        for status in [Ok(()), Err(Error::OutOfMemory)] {
            allowed.push(Return::Call(CallOutcome { status, lend, reply_present: true }));
        }
    }
    for error in [Error::Timeout, Error::Dead] {
        allowed.push(Return::Call(CallOutcome {
            status: Err(error),
            lend: LendDisposition::Consumed,
            reply_present: false,
        }));
    }
    for lend in [LendDisposition::None, LendDisposition::Returned, LendDisposition::Consumed] {
        for status in core::iter::once(Ok(())).chain(Error::ALL.into_iter().map(Err)) {
            for reply_present in [false, true] {
                let value = Return::Call(CallOutcome { status, lend, reply_present });
                let regs = encode_result(&Ok(value));
                assert!(fits_rv32(&regs));
                assert_eq!(
                    decode_result(Number::Call, &regs),
                    if allowed.contains(&value) { Ok(value) } else { Err(Error::InvalidArgument) }
                );
            }
        }
    }
    for delivered in [false, true] {
        for installed in 0..32 {
            let value = Return::Reply(ReplyOutcome { delivered, installed });
            let regs = encode_result(&Ok(value));
            assert!(fits_rv32(&regs));
            let valid = installed < 16 && (delivered || installed == 0);
            assert_eq!(
                decode_result(Number::Reply, &regs),
                if valid { Ok(value) } else { Err(Error::InvalidArgument) }
            );
        }
    }
    for number in [Number::Call, Number::Reply] {
        for slot in 1..REGS {
            let mut regs = [0; REGS];
            regs[2] = u64::from(number == Number::Call);
            regs[slot] = 1 << 32;
            assert_eq!(decode_result(number, &regs), Err(Error::InvalidArgument));
        }
    }
    assert_eq!(decode_result(Number::Call, &[0; REGS]), Err(Error::InvalidArgument), "old success ABI");
    assert_eq!(decode_result(Number::Call, &[0, 3, 1, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::Call, &[0, 1, 2, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::Reply, &[0, 2, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert!(ReplyOutcome { delivered: true, installed: 2 }.validate(1).is_err());
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
        assert_eq!(*number as u64, u64::from(NUMBER_BASE) + i as u64 + 1);
        assert_eq!(Number::from_raw(*number as u64), Some(*number));
        // The spec's name, typed once in the table, agrees with the variant's.
        assert_eq!(number.name(), snake_case(&std::format!("{number:?}")));
    }
    assert_eq!(Number::from_raw(0), None);
    assert_eq!(Number::from_raw(u64::from(NUMBER_BASE)), None);
    assert_eq!(Number::from_raw(u64::from(NUMBER_BASE) + CALLS + 1), None);
    for (i, error) in Error::ALL.iter().enumerate() {
        assert_eq!(*error as u64, i as u64 + 1);
        assert_eq!(Error::from_code(*error as u64), Some(*error));
    }
    assert_eq!(Error::from_code(0), None);
    assert_eq!(Error::from_code(ERRORS + 1), None);
}

fn body(nhandles: u32) -> Body {
    let handles: Vec<Handle> = (0..nhandles).map(|i| h(i * 7 + 1)).collect();
    Body { words: [1, usize::MAX >> 32, 0, 42], handles: Handles::from_slice(&handles).unwrap() }
}

/// `body(n)` as received, every handle present.
fn received_body(nhandles: u32) -> ReceivedBody {
    let b = body(nhandles);
    let handles: Vec<Option<Handle>> = b.handles.as_slice().iter().map(|h| Some(*h)).collect();
    ReceivedBody { words: b.words, handles: ReceivedHandles::from_slice(&handles).unwrap() }
}

fn labels(n: u64) -> Labels { Labels::from_slice(&(0..n).map(|i| BIG ^ i).collect::<Vec<_>>()).unwrap() }

fn sample_received() -> Vec<Received> {
    let message = |kind| {
        Received::Message(Message {
            msg_id: nz(BIG),
            badge: 1,
            account: !BIG,
            labels: labels(3),
            body: received_body(2),
            kind,
        })
    };
    std::vec![
        message(MessageKind::Call { lend: None }),
        message(MessageKind::Call { lend: Some(pages(0x6000, MAX_LEND_PAGES)) }),
        message(MessageKind::Send { transfer: None }),
        message(MessageKind::Send { transfer: Some(pages(0x6000, 1)) }),
        Received::Message(Message {
            msg_id: nz(1),
            badge: u64::MAX,
            account: 0,
            labels: labels(MAX_LABELS as u64),
            body: received_body(MAX_MSG_HANDLES as u32),
            kind: MessageKind::Send { transfer: None },
        }),
        Received::Message(Message {
            msg_id: nz(2),
            badge: 0,
            account: 0,
            labels: Labels::new(),
            // A handle revoked in flight arrives as 0 and keeps its slot (R10).
            body: ReceivedBody {
                words: [0; WORDS],
                handles: ReceivedHandles::from_slice(&[None, Some(h(3)), None]).unwrap(),
            },
            kind: MessageKind::Call { lend: None },
        }),
        Received::Interrupt,
        exit(3, Cause::Exited, 0, 0, 0),
        exit(4, Cause::Faulted, u32::MAX, BIG, 3),
        exit(u32::MAX, Cause::Faulted, 101, 1, MAX_LABELS as u64),
        exit(4, Cause::Faulted, 101, 0, 0),
        exit(5, Cause::Killed, 1, 0, 0),
        Received::Abandoned(nz(BIG)),
        Received::Abandoned(nz(1)),
    ]
}

fn exit(pid: u32, cause: Cause, code: u32, blamed_account: u64, nlabels: u64) -> Received {
    Received::Exit(ExitNotice { pid, cause, code, blamed_account, blamed_labels: labels(nlabels) })
}

/// The record's one layout: where each kind puts its fields.
#[test]
fn received_layout() {
    const WORD0: usize = 4 + 1 + MAX_LABELS;
    const HANDLES: usize = WORD0 + WORDS;
    assert_eq!(RECEIVED_SLOTS, 24);
    let m = sample_received()[1].encode();
    assert_eq!(m[..5], [1, BIG, 1, !BIG, 3]);
    assert_eq!(m[5..8], [BIG, BIG ^ 1, BIG ^ 2]);
    assert_eq!(m[WORD0..HANDLES], [1, u64::from(u32::MAX), 0, 42]);
    assert_eq!(m[HANDLES..HANDLES + 3], [2, 1, 8]);
    assert_eq!(m[RECEIVED_SLOTS - 2..], [0x6000, MAX_LEND_PAGES as u64]);
    assert_eq!(sample_received()[2].encode()[0], 2, "send");
    let mut interrupt = [0; RECEIVED_SLOTS];
    interrupt[0] = 3;
    assert_eq!(Received::Interrupt.encode(), interrupt);
    let mut e = [0; RECEIVED_SLOTS];
    e[0] = 4;
    e[3] = BIG;
    e[4] = 3;
    e[5..8].copy_from_slice(&[BIG, BIG ^ 1, BIG ^ 2]);
    e[WORD0..WORD0 + 3].copy_from_slice(&[4, 2, u64::from(u32::MAX)]);
    assert_eq!(exit(4, Cause::Faulted, u32::MAX, BIG, 3).encode(), e);
    let mut a = [0; RECEIVED_SLOTS];
    a[0] = 5;
    a[1] = BIG;
    assert_eq!(Received::Abandoned(nz(BIG)).encode(), a);
}

fn sample_usage() -> Usage {
    Usage {
        pages_limit: BIG,
        pages_usage: !BIG,
        processes_limit: 40,
        processes_usage: u32::MAX,
        weight_limit: 100,
        weight_carved: 20,
    }
}

fn sample_specs() -> Vec<BudgetSpec> {
    std::vec![
        BudgetSpec {
            pages: 0,
            processes: 0,
            weight: 0,
            labels: Labels::new(),
            account: 0,
            deadline: FOREVER,
        },
        BudgetSpec {
            pages: BIG,
            processes: 4,
            weight: 20,
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
    let usage = sample_usage();
    assert_eq!(Usage::decode(&usage.encode()), Ok(usage));
    // The spec has no scheduling flag (answer 103): slot 3 is the label count.
    assert_eq!(sample_specs()[1].encode()[3], MAX_LABELS as u64, "labels");
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
    let past_last = u64::from(NUMBER_BASE) + CALLS + 1;
    assert_eq!(decode([past_last, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "unknown call");
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
    assert_eq!(
        decode([random, 0x9000, 8, 0, 0, 0, 0, 0]),
        Err(Error::InvalidArgument),
        "random's old arguments"
    );
    assert_eq!(decode([random, 0, 0, 0, 0, 0, 0, 1]), Err(Error::InvalidArgument), "random, a7");
    assert_eq!(decode([random, 0, 0, 0, 0, 0, 0, 0]), Ok(Call::Random));
    let serve = Number::Serve as u64;
    assert_eq!(decode([serve, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "serve id 0");
    assert_eq!(decode([serve, wide, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "serve, wide half");
    assert_eq!(decode([serve, 1, 0, 1, 0, 0, 0, 0]), Err(Error::InvalidArgument), "serve, a3");
    assert_eq!(decode([serve, 0, 1, 0, 0, 0, 0, 0]), Ok(Call::Serve { msg_id: nz(1 << 32) }));
    let call = Number::Call as u64;
    assert_eq!(decode([call, 6, 0x5000, 0x6000, 0, 0, 0, 0]), Err(Error::InvalidArgument), "lend of 0 pages");
    assert_eq!(decode([call, 6, 0x5000, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "lend at 0");
    let exit = Number::ProcessExit as u64;
    assert_eq!(decode([exit, wide, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "wide code");
    let close = Number::HandleClose as u64;
    assert_eq!(decode([close, 0, 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle), "handle 0");
    assert_eq!(decode([close, wide, 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle), "wide handle");
    let mint = Number::Mint as u64;
    assert_eq!(decode([mint, 1, 5, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "badge 0");
    assert_eq!(decode([mint, 3, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "unknown mint source");
    assert_eq!(decode([mint, 0, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "mint source 0");
    assert_eq!(decode([mint, 2, wide, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "wide half");
    assert_eq!(decode([mint, 2, 0, 1, 1, 0, 0, 0]), Err(Error::BadHandle), "wide mint source handle");
    assert_eq!(decode([mint, 2, 0, 0, 1, 0, 0, 0]), Err(Error::BadHandle), "mint source handle 0");
    assert_eq!(decode([mint, 1, 0, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument), "message id 0");
    assert_eq!(
        decode([mint, 1, 5, 0, 1, 0, 0, 0]),
        Ok(Call::Mint { source: MintSource::Message(nz(5)), badge: nz(1), budget: None })
    );
    let reply = Number::Reply as u64;
    assert_eq!(decode([reply, 0, 0, 0x5000, 0, 0, 0, 0]), Err(Error::InvalidArgument), "reply to id 0");
    let start = Number::ProcessStart as u64;
    let too_many = MAX_START_HANDLES as u64 + 1;
    assert_eq!(
        decode([start, 1, 0x1000, 0x2000, 0x4000, 0x3000, too_many, 0]),
        Err(Error::TooLarge),
        "start list"
    );
    assert_eq!(
        decode([start, 1, 0x1000, 0x2000, 0x4000, 0x3000, too_many, 1]),
        Err(Error::TooLarge),
        "order"
    );
    assert_eq!(
        decode([start, 0, 0x1000, 0x2000, 0x4000, 0x3000, too_many, 0]),
        Err(Error::BadHandle),
        "order"
    );
    assert_eq!(decode([start, 1, 0x1000, 0x2000, 0, 0x3000, 1, 1]), Err(Error::InvalidArgument), "a7");
    assert_eq!(
        decode([start, 1, 0x1000, 0x2000, 0, 0x3000, wide, 0]),
        Err(Error::InvalidArgument),
        "wide count"
    );
    assert_eq!(
        decode([start, 1, 0x1000, 0x2000, u64::from(u32::MAX), 0x3000, MAX_START_HANDLES as u64, 0]),
        Ok(Call::ProcessStart {
            process: h(1),
            entry: 0x1000,
            sp: 0x2000,
            arg: u32::MAX as usize,
            handles_rec: 0x3000,
            count: MAX_START_HANDLES as u32
        }),
        "arg is not checked"
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
    assert_eq!(decode_result(Number::Mint, &[0, 0, 0, 0, 0, 0, 0, 0]), Err(Error::BadHandle), "handle 0");
}

#[test]
fn malformed_records_are_refused() {
    // Body: too many handles, a stray handle slot, handle 0.
    let mut slots = body(1).encode();
    slots[WORDS] = MAX_MSG_HANDLES as u64 + 1;
    assert_eq!(Body::decode(&slots), Err(Error::TooLarge));
    let mut slots = body(1).encode();
    slots[WORDS + 2] = 9;
    assert_eq!(Body::decode(&slots), Err(Error::InvalidArgument));
    let mut slots = body(1).encode();
    slots[WORDS + 1] = 0;
    assert_eq!(Body::decode(&slots), Err(Error::BadHandle));

    // ReceivedBody: the same slots, but 0 within the count is a missing handle, not an error.
    let mut slots = body(2).encode();
    slots[WORDS + 1] = 0;
    let got = ReceivedBody::decode(&slots).unwrap();
    assert_eq!(got.handles.as_slice(), &[None, Some(h(8))]);
    assert_eq!(got.encode(), slots);
    for (slot, value, error) in [
        (WORDS, MAX_MSG_HANDLES as u64 + 1, Error::TooLarge),
        (WORDS + 3, 9, Error::InvalidArgument),
        (WORDS + 1, 1 << 32, Error::BadHandle),
    ] {
        let mut slots = body(2).encode();
        slots[slot] = value;
        assert_eq!(ReceivedBody::decode(&slots), Err(error), "received body slot {slot} = {value}");
    }

    // BudgetSpec: too many labels, a stray label slot, wide fields.
    let spec = sample_specs()[0];
    for (slot, value, error) in [
        (3, MAX_LABELS as u64 + 1, Error::TooLarge),
        (3, u64::MAX, Error::TooLarge),
        (4, 1, Error::InvalidArgument),
        (1, 1 << 32, Error::InvalidArgument),
        (2, 1 << 32, Error::InvalidArgument),
    ] {
        let mut slots = spec.encode();
        slots[slot] = value;
        assert_eq!(BudgetSpec::decode(&slots), Err(error), "spec slot {slot} = {value}");
    }

    // Received: unknown kinds; each kind with any field it does not use set; bad fields.
    const WORD0: usize = 4 + 1 + MAX_LABELS;
    for kind in [0, 6, u64::MAX] {
        let mut slots = [0; RECEIVED_SLOTS];
        slots[0] = kind;
        assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "kind {kind}");
    }
    let unused = |r: Received, used: &[usize]| {
        for slot in (1..RECEIVED_SLOTS).filter(|s| !used.contains(s)) {
            let mut slots = r.encode();
            slots[slot] = 1;
            assert!(Received::decode(&slots).is_err(), "{r:?} with slot {slot} set");
        }
    };
    unused(Received::Interrupt, &[]);
    unused(Received::Abandoned(nz(BIG)), &[1]);
    let labels_and_words: Vec<usize> = (3..WORD0 + 3).collect();
    unused(exit(1, Cause::Killed, 0, 0, 0), &labels_and_words);
    let mut slots = exit(1, Cause::Killed, 0, 0, 0).encode();
    for (slot, value, what) in [
        (WORD0 + 1, 4, "unknown cause"),
        (WORD0 + 1, 0, "cause 0"),
        (WORD0, 1 << 32, "wide pid"),
        (WORD0 + 2, 1 << 32, "wide code"),
        (4, MAX_LABELS as u64 + 1, "too many labels"),
    ] {
        let old = core::mem::replace(&mut slots[slot], value);
        assert!(Received::decode(&slots).is_err(), "exit: {what}");
        slots[slot] = old;
    }
    let mut slots = Received::Abandoned(nz(1)).encode();
    slots[1] = 0;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "abandoned id 0");
    let mut slots = sample_received()[0].encode();
    slots[RECEIVED_SLOTS - 2] = 0x6000;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "an address and no pages");
    let mut slots = sample_received()[1].encode();
    slots[RECEIVED_SLOTS - 1] = 0;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "a lend of 0 pages");
    let mut slots = sample_received()[0].encode();
    slots[1] = 0;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "message id 0");
    let mut slots = sample_received()[0].encode();
    slots[WORD0 + WORDS] = MAX_MSG_HANDLES as u64 + 1;
    assert_eq!(Received::decode(&slots), Err(Error::TooLarge), "too many handles");
    let mut slots = sample_received()[0].encode();
    slots[WORD0 + WORDS + 1] = 1 << 32;
    assert_eq!(Received::decode(&slots), Err(Error::BadHandle), "wide handle");
    let mut slots = sample_received()[0].encode();
    slots[WORD0 + WORDS + 3] = 5;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "a handle past the count");

    // Usage: a counter too wide for its field.
    let mut slots = sample_usage().encode();
    slots[USAGE_SLOTS - 1] = 1 << 32;
    assert_eq!(Usage::decode(&slots), Err(Error::InvalidArgument));
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
/// registers it came from. Many decode (checked), so the canonical check has work.
#[test]
fn random_registers() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut calls, mut results) = (0, 0);
    for _ in 0..200_000 {
        // Mostly zeros, so calls with few arguments decode often.
        let mut regs = [0; REGS].map(|_| if rng.next().is_multiple_of(2) { 0 } else { rng.value() });
        regs[0] = u64::from(NUMBER_BASE) + rng.next() % (CALLS + 2);
        if let Ok(call) = Call::decode(&regs) {
            assert_eq!(call.encode(), regs);
            calls += 1;
        }
        let number = Number::ALL[(rng.next() % CALLS) as usize];
        regs[0] = rng.next() % (ERRORS + 2);
        if let Ok(value) = decode_result(number, &regs) {
            assert_eq!(encode_result(&Ok(value)), regs);
            results += 1;
        }
    }
    assert!(calls > 1000 && results > 100, "{calls} calls and {results} results decoded");
}

/// Random slots, and valid records with a few slots changed: decoding never panics, and whatever
/// decodes re-encodes to its input.
#[test]
fn random_records() {
    let mut rng = Rng(0xd1b5_4a32_d192_ed03);
    let valid = sample_received();
    let mut decoded = [0; 6];
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
        received[0] %= 7;
        if let Ok(r) = Received::decode(&received) {
            assert_eq!(r.encode(), received);
        }
        let mut received = valid[(rng.next() % valid.len() as u64) as usize].encode();
        for _ in 0..rng.next() % 3 {
            received[(rng.next() % RECEIVED_SLOTS as u64) as usize] = rng.value();
        }
        if let Ok(r) = Received::decode(&received) {
            assert_eq!(r.encode(), received);
            decoded[received[0] as usize] += 1;
        }
        let usage = [0; USAGE_SLOTS].map(|_| rng.value());
        if let Ok(u) = Usage::decode(&usage) {
            assert_eq!(u.encode(), usage);
        }
    }
    assert!(decoded[1..].iter().all(|n| *n > 100), "decoded per kind: {decoded:?}");
}

/// Each call's error row (KERNEL-SPEC.md, the error table), where answers 72-119 changed it.
#[test]
fn error_rows() {
    use Error::*;
    let has = |n: Number, errors: &[Error]| errors.iter().all(|e| n.can_return(*e));
    let lacks = |n: Number, errors: &[Error]| errors.iter().all(|e| !n.can_return(*e));
    for n in Number::ALL {
        // Decoding's general error, for every call (a non-zero unused register).
        assert!(n.can_return(InvalidArgument), "{n:?}");
    }
    // A reply's handles that do not fit the caller arrive as 0 and its `call` is `OutOfMemory`
    // (answers 107 and 116); `TooLarge` here means only a lend over `MAX_LEND_PAGES`.
    assert!(has(Number::Call, &[Refused, LabelDenied, Busy, Timeout, Dead, TooLarge, OutOfMemory]));
    assert!(lacks(Number::Call, &[NotPermitted]));
    assert!(has(Number::Send, &[Refused, LabelDenied, Busy, Timeout, Dead]));
    assert!(lacks(Number::Send, &[OutOfMemory, NotPermitted]));
    assert!(has(Number::Receive, &[BadHandle, WrongObject, NotPermitted, Timeout, Dead]));
    assert!(lacks(Number::Receive, &[Busy, OutOfMemory, Refused, LabelDenied, TooLarge]));
    assert!(has(Number::ProcessCreate, &[NotPermitted, OutOfProcesses, OutOfMemory]));
    assert!(has(Number::ProcessStart, &[TooLarge, BadHandle, NotPermitted, OutOfMemory]));
    assert!(has(Number::BudgetCreate, &[ClassDenied, LabelDenied, TooLarge, OutOfProcesses]));
    assert!(lacks(Number::BudgetCreate, &[NotPermitted, Busy]));
    assert!(has(Number::Mint, &[Dead, NotPermitted]));
    assert!(has(Number::Reply, &[InvalidArgument, BadHandle, TooLarge]));
    assert!(lacks(Number::Reply, &[OutOfMemory, Dead, Refused]));
    assert!(lacks(Number::Serve, &[BadHandle, TooLarge, Dead, NotPermitted]));
    assert!(lacks(Number::Random, &[TooLarge, BadHandle, OutOfMemory]));
    assert!(lacks(Number::TimeNow, &[BadHandle, OutOfMemory]));
    assert!(lacks(Number::ThreadExit, &[BadHandle, OutOfMemory]));
    // Answer 102: every call that adds a handle to its caller's table.
    for n in [Number::ProcessCreate, Number::EndpointCreate, Number::Mint, Number::BudgetCreate] {
        assert!(has(n, &[OutOfMemory, TooLarge]), "{n:?}");
    }
}
