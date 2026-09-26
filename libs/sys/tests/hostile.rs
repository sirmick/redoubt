//! Hostile inputs to the decoders, from WP-A2's review: per-kind field sets, bit flips, width
//! and count edges, the order of `mint`'s checks, and a revoked handle in a received message.

use core::num::{NonZeroU64, NonZeroUsize};

use redoubt_sys::*;

const BIG: u64 = 0x1234_5678_9abc_def0;
const WORD0: usize = 4 + 1 + MAX_LABELS; // first word slot of a Received record
const HCOUNT: usize = WORD0 + WORDS; // handle count slot

fn h(i: u32) -> Handle { Handle::new(i).unwrap() }
fn nz(v: u64) -> NonZeroU64 { NonZeroU64::new(v).unwrap() }
fn pages(addr: usize, n: usize) -> Pages { Pages { addr, npages: NonZeroUsize::new(n).unwrap() } }

fn received(handles: &[u32]) -> ReceivedBody {
    let handles: Vec<Option<Handle>> = handles.iter().map(|i| Handle::new(*i)).collect();
    ReceivedBody { words: [1, 2, 3, 4], handles: ReceivedHandles::from_slice(&handles).unwrap() }
}

fn message(kind: MessageKind) -> Received {
    Received::Message(Message {
        kind,
        msg_id: nz(BIG),
        badge: 7,
        account: 9,
        labels: Labels::from_slice(&[1, 2]).unwrap(),
        body: received(&[5, 6]),
    })
}

fn exit(cause: Cause) -> Received {
    Received::Exit(ExitNotice { pid: 3, cause, code: 1, blamed_account: 0, blamed_labels: Labels::new() })
}

/// R10: a handle revoked while its message is queued "arrives as 0", and WIRE.md numbers
/// handles by slot, so the 0 keeps its slot and the message still decodes.
#[test]
fn revoked_handle_in_a_received_message() {
    let mut slots = message(MessageKind::Call { lend: None }).encode();
    assert_eq!(slots[HCOUNT], 2);
    slots[HCOUNT + 2] = 0; // the second handle was revoked in flight
    let Ok(Received::Message(m)) = Received::decode(&slots) else { panic!("refused") };
    assert_eq!(m.body.handles.as_slice(), &[Some(h(5)), None]);
    assert_eq!(Received::Message(m).encode(), slots);
    // All of them revoked.
    slots[HCOUNT + 1] = 0;
    let Ok(Received::Message(m)) = Received::decode(&slots) else { panic!("refused") };
    assert_eq!(m.body.handles.as_slice(), &[None, None]);
}

/// The kind tag alone separates the kinds. Retagging an abandoned notice as a call yields a
/// well-formed message (badge 0, no labels); that is the kernel's trust, not an ABI hole, but
/// it means nothing but slot 0 tells the kinds apart.
#[test]
fn kinds_are_separated_only_by_slot_zero() {
    let mut slots = Received::Abandoned(nz(BIG)).encode();
    slots[0] = 1;
    assert!(matches!(Received::decode(&slots), Ok(Received::Message(_))));
    // An exit record retagged as a message has msg_id 0, so it is refused.
    let mut slots = exit(Cause::Killed).encode();
    slots[0] = 2;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument));
}

/// For every kind, exactly the spec's fields may be non-zero (KERNEL-SPEC.md, ABI: "a field a
/// kind does not use is 0 or empty"). Slots outside the set never decode for any value.
#[test]
fn per_kind_field_sets_match_the_spec() {
    let all: Vec<usize> = (1..RECEIVED_SLOTS).collect();
    let exit_set: Vec<usize> = std::iter::once(3).chain(4..WORD0).chain(WORD0..WORD0 + 3).collect();
    let full = |kind| {
        Received::Message(Message {
            kind,
            msg_id: nz(BIG),
            badge: 7,
            account: 9,
            labels: Labels::from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
            body: received(&[5, 6, 7, 8]),
        })
    };
    let full_exit = Received::Exit(ExitNotice {
        pid: 3,
        cause: Cause::Faulted,
        code: 1,
        blamed_account: 4,
        blamed_labels: Labels::from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
    });
    let cases: Vec<(Received, Vec<usize>)> = vec![
        (full(MessageKind::Call { lend: Some(pages(0x6000, 2)) }), all.clone()),
        (full(MessageKind::Send { transfer: Some(pages(0x6000, 2)) }), all),
        (Received::Interrupt, vec![]),
        (full_exit, exit_set),
        (Received::Abandoned(nz(1)), vec![1]),
    ];
    for (record, allowed) in cases {
        let base = record.encode();
        for slot in 1..RECEIVED_SLOTS {
            let mut decodes = false;
            for value in [1u64, 2, 3, 4, 8, 0x1000, 1 << 32, u64::MAX] {
                let mut slots = base;
                slots[slot] = value;
                if let Ok(r) = Received::decode(&slots) {
                    assert_eq!(r.encode(), slots, "{record:?} slot {slot} = {value:#x}");
                    decodes = true;
                }
            }
            if allowed.contains(&slot) {
                assert!(decodes, "{record:?}: slot {slot} is used by this kind but never decodes");
            } else {
                assert!(!decodes, "{record:?}: unused slot {slot} accepted a non-zero value");
            }
        }
    }
}

/// Semantic rules the decoder does not enforce (the kernel's to keep): an `exited` notice with a
/// blame, a badge-0 message.
#[test]
fn semantic_rules_are_not_the_decoders() {
    let n = Received::Exit(ExitNotice {
        pid: 1,
        cause: Cause::Exited,
        code: 0,
        blamed_account: 5,
        blamed_labels: Labels::from_slice(&[1]).unwrap(),
    });
    assert_eq!(Received::decode(&n.encode()), Ok(n));
}

/// A list in a field its kind does not fill is refused as a stray slot (`InvalidArgument`),
/// before any list is read: its count is not checked against the capacity, nor its items.
#[test]
fn unused_list_counts_are_stray_slots() {
    for kind in [Received::Interrupt, Received::Abandoned(nz(1))] {
        for (slot, value) in [(HCOUNT, MAX_MSG_HANDLES as u64 + 1), (HCOUNT, 1), (4, u64::MAX), (4, 1)] {
            let mut slots = kind.encode();
            slots[slot] = value;
            assert_eq!(
                Received::decode(&slots),
                Err(Error::InvalidArgument),
                "{kind:?} slot {slot} = {value}"
            );
        }
    }
    let mut slots = exit(Cause::Killed).encode();
    slots[HCOUNT] = 1;
    assert_eq!(Received::decode(&slots), Err(Error::InvalidArgument), "exit with a handle count");
}

/// Every `a0` outside the call table is an unknown number, refused `InvalidArgument` at decoding:
/// all of `0..=NUMBER_BASE`, the first number past the table, and any with high bits set.
#[test]
fn numbers_outside_the_table_are_unknown() {
    let mut hits = 0;
    for raw in 0..=0x400u64 {
        if let Some(n) = Number::from_raw(raw) {
            assert!(raw > u64::from(NUMBER_BASE), "{n:?} at {raw}");
            hits += 1;
        }
    }
    assert_eq!(hits, Number::ALL.len());
    for raw in 0..=u64::from(NUMBER_BASE) {
        assert_eq!(Call::decode(&[raw, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument), "a0 = {raw}");
    }
    assert_eq!(Number::from_raw(u64::from(NUMBER_BASE)), None);
    assert_eq!(Number::from_raw(u64::MAX), None);
    assert_eq!(Number::from_raw(1 << 32 | u64::from(Number::Random as u32)), None, "high bits alias");
}

/// A budget spec carries no scheduling flag (answer 103): the slot that held one is the label
/// count, and a count over `MAX_LABELS` is `TooLarge`, however large.
#[test]
fn a_budget_spec_asks_for_no_place_in_the_queue() {
    let spec = BudgetSpec {
        pages: 1,
        processes: 0,
        weight: 0,
        labels: Labels::new(),
        account: 0,
        deadline: FOREVER,
    };
    for v in [MAX_LABELS as u64 + 1, 0xff, 1 << 31, 1 << 32, 1 << 63, u64::MAX] {
        let mut slots = spec.encode();
        slots[3] = v;
        assert_eq!(BudgetSpec::decode(&slots), Err(Error::TooLarge), "labels = {v:#x}");
    }
    let mut slots = spec.encode();
    slots[3] = 1;
    slots[4] = 7;
    assert_eq!(BudgetSpec::decode(&slots).map(|s| s.labels.as_slice().to_vec()), Ok(std::vec![7]));
}

/// Huge counts and tags: errors, never overflow or panic.
#[test]
fn huge_counts_and_tags() {
    let mut body = [0u64; BODY_SLOTS];
    body[WORDS] = u64::MAX;
    assert_eq!(Body::decode(&body), Err(Error::TooLarge));
    let mut spec = [0u64; BUDGET_SPEC_SLOTS];
    spec[3] = u64::MAX;
    spec[BUDGET_SPEC_SLOTS - 1] = FOREVER;
    assert_eq!(BudgetSpec::decode(&spec), Err(Error::TooLarge));
    let start = Number::ProcessStart as u64;
    assert_eq!(
        Call::decode(&[start, 1, 0, 0, 0, 0, u64::from(u32::MAX), 0]),
        Err(Error::TooLarge),
        "count u32::MAX"
    );
    let reset = Number::SystemReset as u64;
    for tag in [u64::MAX, 1 << 32, 1 << 63, 3] {
        assert_eq!(
            Call::decode(&[reset, 1, tag, 0, 0, 0, 0, 0]),
            Err(Error::InvalidArgument),
            "tag {tag:#x}"
        );
    }
    let mint = Number::Mint as u64;
    assert_eq!(Call::decode(&[mint, u64::MAX, 1, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument));
    // Timeout halves: each must fit 32 bits; FOREVER is (u32::MAX, u32::MAX).
    let call = Number::Call as u64;
    let m = u64::from(u32::MAX);
    assert_eq!(
        Call::decode(&[call, 1, 0x5000, 0, 0, m, m, 0]),
        Ok(Call::Call { endpoint: h(1), body_rec: 0x5000, lend: None, timeout: FOREVER })
    );
}

/// `mint` from a handle: the value travels as a u64 in two registers. A wide low half (rv64
/// only) is `InvalidArgument`, a handle that needs the high half is `BadHandle`: two errors for
/// "a handle that does not fit in 32 bits", depending on which register carries the excess.
#[test]
fn mint_handle_source_width_errors() {
    let mint = Number::Mint as u64;
    assert_eq!(Call::decode(&[mint, 2, 1 << 32, 0, 1, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(Call::decode(&[mint, 2, 0, 1, 1, 0, 0, 0]), Err(Error::BadHandle));
}

/// Every `Received` sample and every `Call` sample decode with one bit flipped in every
/// position: whatever still decodes re-encodes exactly (no aliasing), across all 64 bits.
#[test]
fn bit_flips_never_alias() {
    for record in [
        message(MessageKind::Call { lend: Some(pages(0x6000, 1)) }),
        message(MessageKind::Send { transfer: None }),
        Received::Interrupt,
        exit(Cause::Exited),
        Received::Abandoned(nz(BIG)),
    ] {
        let base = record.encode();
        for slot in 0..RECEIVED_SLOTS {
            for bit in 0..64 {
                let mut slots = base;
                slots[slot] ^= 1 << bit;
                if let Ok(r) = Received::decode(&slots) {
                    assert_eq!(r.encode(), slots, "{record:?} slot {slot} bit {bit}");
                    assert_ne!(r, record, "{record:?} ignores slot {slot} bit {bit}");
                }
            }
        }
    }
    let spec = BudgetSpec {
        pages: BIG,
        processes: 4,
        weight: 20,
        labels: Labels::from_slice(&[1, 2, 3]).unwrap(),
        account: 5,
        deadline: BIG,
    };
    let base = spec.encode();
    for slot in 0..BUDGET_SPEC_SLOTS {
        for bit in 0..64 {
            let mut slots = base;
            slots[slot] ^= 1 << bit;
            if let Ok(s) = BudgetSpec::decode(&slots) {
                assert_eq!(s.encode(), slots, "spec slot {slot} bit {bit}");
                assert_ne!(s, spec, "spec ignores slot {slot} bit {bit}");
            }
        }
    }
}

/// Results: an error with a stray register, a success with a wide half, and `Random` with
/// each half at its limit.
#[test]
fn result_edges() {
    let m = u64::from(u32::MAX);
    assert_eq!(decode_result(Number::Random, &[0, m, m, 0, 0, 0, 0, 0]), Ok(Return::Random(u64::MAX)));
    assert_eq!(decode_result(Number::Random, &[0, 1 << 32, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::Random, &[0, 0, 0, 1, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::Random, &[13, 0, 0, 0, 0, 0, 0, 1]), Err(Error::InvalidArgument));
    assert_eq!(decode_result(Number::TimeNow, &[1 << 32, 0, 0, 0, 0, 0, 0, 0]), Err(Error::InvalidArgument));
    assert_eq!(encode_result(&Ok(Return::Random(u64::MAX))), [0, m, m, 0, 0, 0, 0, 0]);
}
