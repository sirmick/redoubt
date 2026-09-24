//! `ipd`'s sizing (NAMESPACES.md, the milestone manifest; answer 174): the worst case, every
//! override's bucket and every other at the defaults, must leave `MAX_OPEN_CALLS`' headroom (48)
//! and fit the budget, or `ipd` does not start.

use redoubt_ipd::args::parse;

fn sizing_of(args: &[&str]) -> Result<redoubt_ipd::sizing::Sizing, redoubt_ipd::args::BadArgs> {
    parse(args.iter().copied()).unwrap().sizing()
}

/// The rig's and the milestone manifest's arguments fit: 23 + 5 (the steward's slot at its worst)
/// + 4 x 5 = 48; with sshd at 24 they do not (QA D3-code-review-3).
#[test]
fn the_rig_and_the_milestone_fit() {
    let rig = [
        "addr=10.0.2.15/24",
        "gateway=10.0.2.2",
        "self=10.0.2.0/24",
        "self=10.0.9.102/32",
        "ingress=3",
        "scope=4:c:0.0.0.0/0:1-65535,l:1-65535",
        "buckets=4",
    ];
    assert!(sizing_of(&rig).is_ok());
    let milestone = [
        "addr=10.0.2.15/24",
        "gateway=10.0.2.2",
        "self=10.0.2.0/24",
        "ingress=3",
        "scope=4:c:0.0.0.0/0:1-65535",
        "scope=5:l:22",
        "buckets=6",
        "limits=5:23:0:20",
        "limits=4:2:32:0",
    ];
    let mut over = milestone;
    over[7] = "limits=5:24:0:20";
    assert!(sizing_of(&over).is_err(), "sshd 24 with the steward at 2 admits 49");
    let sizing = sizing_of(&milestone).unwrap();
    // Sockets are `State` units (QA D3-code-review-5): sshd's 0 + 20, the steward's 32 + 0, and
    // four defaults of 4 + 8. At their worst: 20, 32, and 4 x 12, each override slot at the
    // larger of its units and the default's.
    assert_eq!(sizing.max_sockets, 20 + 32 + 4 * 12);
    assert_eq!(sizing.caps.overrides, vec![(5, 20), (4, 32)]);
    // An override below the default counts as the default: its slot can go to a default bucket.
    let mut small = milestone;
    small[8] = "limits=4:2:2:0";
    assert_eq!(sizing_of(&small).unwrap().max_sockets, 20 + 12 + 4 * 12);
}

/// Sizing: the worst case, every override's bucket and every other at the defaults, must leave
/// `MAX_OPEN_CALLS`' headroom (48) and fit the budget, or `ipd` does not start.
#[test]
fn the_worst_case_must_fit_or_ipd_does_not_start() {
    let with = |extra: &[&str]| {
        let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "scope=4:c:0.0.0.0/0:1-65535", "scope=5:l:22"];
        args.extend_from_slice(extra);
        sizing_of(&args).map(|_| ())
    };
    // 6 x 5 = 30.
    assert!(with(&["buckets=6"]).is_ok());
    // 10 x 5 = 50 > 48.
    assert!(with(&["buckets=10"]).is_err());
    // 30 + 3 x 5 = 45; 31 + 3 x 5 = 46; 34 + 3 x 5 = 49 > 48.
    assert!(with(&["buckets=4", "limits=5:30:0:8"]).is_ok());
    assert!(with(&["buckets=4", "limits=5:34:0:8"]).is_err());
    // An override of 1 is below the smallest useful cap.
    assert!(with(&["buckets=4", "limits=5:1:0:8"]).is_err());
    // More overrides than buckets.
    assert!(with(&["buckets=1", "limits=4:2:2:0", "limits=5:2:0:2"]).is_err());
    // Sockets that do not fit the budget: 32 buckets of 64 sockets.
    let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "buckets=8"];
    let scopes: Vec<String> = (10..18).map(|b| format!("scope={b}:l:22")).collect();
    let limits: Vec<String> = (10..18).map(|b| format!("limits={b}:2:0:64")).collect();
    args.extend(scopes.iter().map(String::as_str));
    args.extend(limits.iter().map(String::as_str));
    assert!(sizing_of(&args).is_err(), "8 x 64 sockets of 17 KiB fit 8 MiB?");
}
