//! Every decoder in redoubt-sys on arbitrary input: none may panic, and whatever decodes must
//! re-encode to exactly the input (one encoding per value).

#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_sys::{
    BODY_SLOTS, BUDGET_SPEC_SLOTS, Body, BudgetSpec, Call, Number, RECEIVED_SLOTS, REGS, Received,
    ReceivedBody, USAGE_SLOTS, Usage, decode_result, encode_result,
};

/// Fills `N` slots from little-endian bytes; missing bytes are 0.
fn slots<const N: usize>(bytes: &[u8]) -> [u64; N] {
    let mut out = [0; N];
    for (slot, chunk) in out.iter_mut().zip(bytes.chunks(8)) {
        let mut word = [0; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        *slot = u64::from_le_bytes(word);
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, bytes)) = data.split_first() else { return };
    match selector % 7 {
        0 => {
            let regs = slots::<REGS>(bytes);
            if let Ok(call) = Call::decode(&regs) {
                assert_eq!(call.encode(), regs);
            }
        }
        1 => {
            let number = Number::ALL[usize::from(selector / 7) % Number::ALL.len()];
            let regs = slots::<REGS>(bytes);
            if let Ok(value) = decode_result(number, &regs) {
                assert_eq!(encode_result(&Ok(value)), regs);
            }
        }
        2 => {
            let rec = slots::<BODY_SLOTS>(bytes);
            if let Ok(body) = Body::decode(&rec) {
                assert_eq!(body.encode(), rec);
            }
        }
        3 => {
            let rec = slots::<RECEIVED_SLOTS>(bytes);
            if let Ok(received) = Received::decode(&rec) {
                assert_eq!(received.encode(), rec);
            }
        }
        6 => {
            // A reply as `call`'s caller reads it back: a handle slot may be 0.
            let rec = slots::<BODY_SLOTS>(bytes);
            if let Ok(body) = ReceivedBody::decode(&rec) {
                assert_eq!(body.encode(), rec);
            }
        }
        4 => {
            let rec = slots::<USAGE_SLOTS>(bytes);
            if let Ok(usage) = Usage::decode(&rec) {
                assert_eq!(usage.encode(), rec);
            }
        }
        _ => {
            let rec = slots::<BUDGET_SPEC_SLOTS>(bytes);
            if let Ok(spec) = BudgetSpec::decode(&rec) {
                assert_eq!(spec.encode(), rec);
            }
        }
    }
});
