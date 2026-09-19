//! Strict JSON, differentially against serde_json:
//! - parsing never panics or hangs;
//! - whatever we accept, serde_json accepts too, with the same value (the profile only
//!   narrows JSON);
//! - whatever we refuse as plain syntax (not a profile rule), serde_json refuses too;
//! - the unknown-member mechanism refuses exactly the members a decoder did not take.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_wire::json::{self, ErrorKind, Value};

fn same(ours: &Value<'_>, theirs: &serde_json::Value) -> bool {
    use serde_json::Value as S;
    match (ours, theirs) {
        (Value::Null, S::Null) => true,
        (Value::Bool(a), S::Bool(b)) => a == b,
        (Value::Int(a), S::Number(b)) => b.as_i64() == Some(*a),
        (Value::Str(a), S::String(b)) => a.as_ref() == b.as_str(),
        (Value::Array(a), S::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)),
        (Value::Object(a), S::Object(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka.as_ref() == kb.as_str() && same(va, vb))
        }
        _ => false,
    }
}

fn check_members(v: &Value<'_>) {
    match v {
        Value::Object(members) => {
            let mut m = v.members().unwrap();
            for (name, _) in members {
                assert!(m.optional(name).is_some());
            }
            assert_eq!(m.finish(), Ok(()));
            if let Some((skipped, _)) = members.first() {
                let mut m = v.members().unwrap();
                for (name, _) in members.iter().skip(1) {
                    m.optional(name);
                }
                assert_eq!(m.finish(), Err(json::SchemaError::Unknown(skipped.to_string())));
            }
            members.iter().for_each(|(_, v)| check_members(v));
        }
        Value::Array(items) => items.iter().for_each(check_members),
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let theirs = serde_json::from_slice::<serde_json::Value>(data);
    match json::parse(data) {
        Ok(ours) => {
            let theirs = theirs.expect("we accepted what serde_json refuses");
            assert!(same(&ours, &theirs), "values differ: {ours:?} vs {theirs:?}");
            check_members(&ours);
            if let Value::Str(s) = &ours {
                assert_eq!(ours.as_u64(), s.parse::<u64>().ok().filter(|_| !s.starts_with(['+', '0']) || s.as_ref() == "0"));
            }
        }
        Err(e) => {
            let syntax = matches!(e.kind, ErrorKind::Syntax | ErrorKind::ControlCharacter | ErrorKind::BadEscape | ErrorKind::Trailing);
            if syntax {
                assert!(theirs.is_err(), "serde_json accepts what we call {e:?}");
            }
        }
    }
});
