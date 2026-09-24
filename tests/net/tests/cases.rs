//! The rig's list of the box's own addresses and the bench's are one list: every case that boots
//! the rig forbids exactly what its `ipd` refuses, so a SYN the capture finds to one of them is a
//! failure of `ipd`, and one `ipd` refuses is never allowed by the bench.

use std::path::PathBuf;

use redoubt_ipd::scope::{Prefix, SelfSet};
use redoubt_net_tests::{SELF_ALWAYS, SELF_ARGS};

fn prefix(text: &str) -> (u32, u8) {
    let (addr, len) = text.split_once('/').unwrap();
    let octets: Vec<u8> = addr.split('.').map(|o| o.parse().unwrap()).collect();
    (u32::from_be_bytes(octets.try_into().unwrap()), len.parse().unwrap())
}

/// Every case booting one of this crate's rigs, with its `[net]` table.
fn rig_cases() -> Vec<(String, toml::Value)> {
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(&tests).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let case: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let rig = case.get("programs").and_then(|p| p.as_array()).is_some_and(|programs| {
            programs.iter().any(|p| p.get("package").and_then(|p| p.as_str()) == Some("redoubt-net-tests"))
        });
        if rig {
            let net = case.get("net").cloned().unwrap_or(toml::Value::Table(Default::default()));
            cases.push((path.file_name().unwrap().to_string_lossy().into_owned(), net));
        }
    }
    cases.sort_by(|a, b| a.0.cmp(&b.0));
    cases
}

#[test]
fn every_rig_case_forbids_exactly_the_rigs_own_addresses() {
    let expected: Vec<&str> = SELF_ARGS.iter().chain(SELF_ALWAYS).copied().collect();
    let cases = rig_cases();
    assert!(cases.len() >= 6, "found only {:?}", cases.iter().map(|c| &c.0).collect::<Vec<_>>());
    for (name, net) in cases {
        // The probe case has no network to judge.
        let Some(list) = net.get("self_forbidden") else {
            assert!(net.get("peer").is_none(), "{name}: peers without self_forbidden");
            continue;
        };
        let list: Vec<&str> = list.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(list, expected, "{name}");
    }
}

/// `SELF_ALWAYS` is what `ipd` refuses without being told: each of its prefixes, first and last
/// address, is in the self set of an `ipd` given no `self=` at all, and nothing just outside is.
#[test]
fn the_always_list_is_ipds_own() {
    let (addr, len) = prefix("10.0.2.15/24");
    let bare = SelfSet::new(addr, len, &[]);
    for text in SELF_ALWAYS {
        let (base, len) = prefix(text);
        let last = base | (u32::MAX.checked_shr(u32::from(len)).unwrap_or(0));
        assert!(bare.contains(base) && bare.contains(last), "{text}");
    }
    assert!(
        !bare.contains(u32::from_be_bytes([1, 0, 0, 0]))
            && !bare.contains(u32::from_be_bytes([126, 255, 255, 255]))
    );
    // And the rig's arguments add exactly SELF_ARGS.
    let extra: Vec<Prefix> =
        SELF_ARGS.iter().map(|p| prefix(p)).map(|(a, l)| Prefix::new(a, l).unwrap()).collect();
    let rig = SelfSet::new(addr, len, &extra);
    for text in SELF_ARGS {
        assert!(
            rig.contains(prefix(text).0) && !bare.contains(u32::from_be_bytes([10, 0, 9, 102])),
            "{text}"
        );
    }
}
