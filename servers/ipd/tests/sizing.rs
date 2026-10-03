//! `ipd`'s sizing (servers/ipd.md, "Sizing"): the worst case, every
//! override's bucket and every other at the defaults, must leave `MAX_OPEN_CALLS`' headroom
//! (`OPEN_CALL_HEADROOM`) and fit the budget, or `ipd` does not start.

use redoubt_ipd::args::{BadArgs, parse};
use redoubt_ipd::sizing::{BUDGET, PARKED_BYTES};
use redoubt_rt::abi::MAX_OPEN_CALLS;
use redoubt_rt::server::MAX_BUCKETS;
use redoubt_rt::server::admit::OPEN_CALL_HEADROOM;

/// The open calls every bucket at its cap may hold together.
const BOUND: usize = MAX_OPEN_CALLS - OPEN_CALL_HEADROOM;

fn sizing_of(args: &[&str]) -> Result<redoubt_ipd::sizing::Sizing, redoubt_ipd::args::BadArgs> {
    parse(args.iter().copied()).unwrap().sizing()
}

/// One client given every scope, and the milestone manifest's arguments, fit: sshd 23 + 5 (the
/// steward's slot at its worst) + 4 x 5 parked calls.
#[test]
fn every_scope_and_the_milestone_fit() {
    let every_scope = [
        "addr=10.0.2.15/24",
        "gateway=10.0.2.2",
        "self=10.0.2.0/24",
        "self=10.0.9.102/32",
        "ingress=3",
        "scope=4:c:0.0.0.0/0:1-65535,l:1-65535",
        "buckets=4",
    ];
    assert!(sizing_of(&every_scope).is_ok());
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
    let sizing = sizing_of(&milestone).unwrap();
    // Sockets are `State` units: sshd's 0 + 20, the steward's 32 + 0, and four defaults of 4 + 8.
    // At their worst: 20, 32, and 4 x 12, each override slot at the larger of its units and the
    // default's.
    assert_eq!(sizing.max_sockets, 20 + 32 + 4 * 12);
    assert_eq!(sizing.caps.overrides, vec![(5, 20), (4, 32)]);
    // An override below the default counts as the default: its slot can go to a default bucket.
    let mut small = milestone;
    small[8] = "limits=4:2:2:0";
    assert_eq!(sizing_of(&small).unwrap().max_sockets, 20 + 12 + 4 * 12);
}

/// Sizing: the worst case, every override's bucket and every other at the defaults, must leave
/// `MAX_OPEN_CALLS`' headroom and fit the budget, or `ipd` does not start.
#[test]
fn the_worst_case_must_fit_or_ipd_does_not_start() {
    let with = |extra: &[&str]| {
        let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "scope=4:c:0.0.0.0/0:1-65535", "scope=5:l:22"];
        args.extend_from_slice(extra);
        sizing_of(&args).map(|_| ())
    };
    // 6 x 5 = 30.
    assert!(with(&["buckets=6"]).is_ok());
    // The budget binds before the headroom: defaults alone stay inside `BOUND` (`MAX_BUCKETS` x 5),
    // and the budget holds fewer parked calls than `BOUND`, so a worst case at `BOUND`, overrides
    // at the largest `limits=` takes (64) and defaults, is refused by the budget.
    assert!(MAX_BUCKETS as usize * 5 <= BOUND);
    assert!(BUDGET / PARKED_BYTES < BOUND as u64);
    let full = BOUND / 64;
    let defaults = BOUND % 64 / 5;
    let at = |buckets: usize| {
        let mut args = vec![format!("buckets={buckets}"), "addr=10.0.2.15/24".into(), "ingress=3".into()];
        args.extend((10..10 + full).map(|b| format!("scope={b}:l:22")));
        args.extend((10..10 + full).map(|b| format!("limits={b}:64:0:8")));
        sizing_of(&args.iter().map(String::as_str).collect::<Vec<_>>()).map(|_| ())
    };
    assert_eq!(at(full + defaults), Err(BadArgs("every bucket at its cap does not fit the budget")));
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
