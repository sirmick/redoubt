use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use redoubt_rt::abi::MAX_LABELS;

use super::{Args, BadArgs, label_set, number, parse_args};

fn ok<'a>(args: &[&'a str]) -> Result<Args<'a>, BadArgs> { parse_args(args.iter().copied()) }

/// `endpoint=` once, `labels=` at most once with distinct canonical IDs in the order given, and
/// nothing else: what `init` writes is taken, and every departure is refused whole.
#[test]
fn arguments_it_does_not_understand_stop_it_before_serving() {
    assert_eq!(ok(&["endpoint=erofsd:system"]), Ok(Args { endpoint: "erofsd:system", labels: Vec::new() }));
    assert_eq!(ok(&["endpoint=e", "labels=3,1"]).map(|a| a.labels), Ok(vec![3, 1]));
    assert_eq!(ok(&["labels=0", "endpoint=e"]).map(|a| a.labels), Ok(vec![0]));
    let ids: Vec<String> = (1..=MAX_LABELS as u64 + 1).map(|id| id.to_string()).collect();
    let too_many = alloc::format!("labels={}", ids.join(","));
    for bad in [
        &[][..],
        &["endpoint=e", "endpoint=f"],
        &["endpoint="],
        &["endpoint=e/f"],
        &["endpoint=e", "labels=01"],
        &["endpoint=e", "labels=1,1"],
        &["endpoint=e", "labels="],
        &["endpoint=e", "labels=x"],
        &["endpoint=e", "labels=1,"],
        &["endpoint=e", "labels=1", "labels=2"],
        &["endpoint=e", too_many.as_str()],
        &["endpoint=e", "volume=x"],
        &["endpoint=e", "readonly"],
        &["endpoint=e", "buckets=4"],
    ] {
        assert_eq!(ok(bad), Err(BadArgs), "{bad:?}");
    }
}

/// The two rules the other parsers borrow: a number is decimal without leading zeros, and a label
/// set holds at most `MAX_LABELS` distinct numbers.
#[test]
fn a_number_and_a_label_set_follow_the_manifests_rules() {
    assert_eq!(number("0"), Ok(0));
    assert_eq!(number("18446744073709551615"), Ok(u64::MAX));
    for bad in ["", "00", "07", "-1", "1x", "18446744073709551616"] {
        assert_eq!(number(bad), Err(BadArgs), "{bad:?}");
    }
    assert_eq!(label_set("5,2,9"), Ok(vec![5, 2, 9]));
    let full: Vec<String> = (1..=MAX_LABELS as u64).map(|id| id.to_string()).collect();
    assert_eq!(label_set(&full.join(",")).map(|s| s.len()), Ok(MAX_LABELS));
    assert_eq!(label_set(&alloc::format!("{},0", full.join(","))), Err(BadArgs));
    assert_eq!(label_set("2,2"), Err(BadArgs));
    assert_eq!(label_set(""), Err(BadArgs));
}
