//! `ipd`'s sizing (servers/ipd.md, "Sizing"): the worst case, every
//! override's bucket and every other at the defaults, must leave `MAX_OPEN_CALLS`' headroom
//! (`OPEN_CALL_HEADROOM`) and fit the budget, or `ipd` does not start.

use redoubt_ipd::args::{BadArgs, parse};
use redoubt_ipd::sizing::{PARKED_BYTES, SOCKET_BYTES, files_for};
use redoubt_rt::abi::Labels;
use redoubt_rt::abi::MAX_OPEN_CALLS;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::admit::OPEN_CALL_HEADROOM;
use redoubt_rt::server::budget_of;
use redoubt_rt::server::{AdmitKey, MAX_BUCKETS, Resource};

/// The open calls every bucket at its cap may hold together.
const BOUND: usize = MAX_OPEN_CALLS - OPEN_CALL_HEADROOM;

/// The image's `ipd` entry's budget: 4 096 pages with a heap cap of 74, as the program derives
/// it from its startup block (`redoubt_rt::server::budget`).
const IMAGE_BUDGET: u64 = budget_of(4096, 74);
/// A budget that holds fewer parked calls than `BOUND`, so that it binds before the headroom.
const TIGHT_BUDGET: u64 = 7 << 20;

fn sizing_of(args: &[&str], budget: u64) -> Result<redoubt_ipd::sizing::Sizing, redoubt_ipd::args::BadArgs> {
    parse(args.iter().copied()).unwrap().sizing(budget)
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
    assert!(sizing_of(&every_scope, IMAGE_BUDGET).is_ok());
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
    let sizing = sizing_of(&milestone, IMAGE_BUDGET).unwrap();
    // The entry rule (servers/walfsd.md, "Serving"): these six buckets at their caps, the two
    // overrides and four defaults of 712 704 bytes, are 5 955 584 bytes, 1 454 pages, which with
    // the heap cap's 74 and the stack's bound 128 is 1 656, so 1 792 of the image's entry of 4 096
    // (the image's own seven defaults are 4 988 928, 1 218 pages); a seventh here adds a default,
    // 6 668 288; the entry leaves 3 894 pages of budget.
    assert_eq!(sizing.need, 5_955_584);
    let page = redoubt_rt::abi::PAGE_SIZE as u64;
    assert_eq!(sizing.need.div_ceil(page) + 74 + redoubt_rt::server::LAUNCHED_STACK_PAGES, 1656);
    let mut seven = milestone.to_vec();
    seven[6] = "buckets=7";
    assert_eq!(sizing_of(&seven, IMAGE_BUDGET).unwrap().need, 6_668_288);
    assert_eq!(IMAGE_BUDGET, 3894 * page);
    // Sockets are `State` units: sshd's 0 + 20, the steward's 32 + 0, and four defaults of 4 + 8.
    // At their worst: 20, 32, and 4 x 12, each override slot at the larger of its units and the
    // default's.
    assert_eq!(sizing.max_sockets, 20 + 32 + 4 * 12);
    let caps: Vec<_> = sizing.caps.overrides.iter().map(|o| (o.account, o.badge, o.state)).collect();
    assert_eq!(caps, vec![(0, 5, 20), (0, 4, 32)]);
    // An override below the default counts as the default: its slot can go to a default bucket.
    let mut small = milestone;
    small[8] = "limits=4:2:2:0";
    assert_eq!(sizing_of(&small, IMAGE_BUDGET).unwrap().max_sockets, 20 + 12 + 4 * 12);
}

/// The image's arguments: seven buckets, alice's account (1001) at 5 parked calls, 4 connections
/// and 32 sockets. Her bucket's fids are `files_for(32)`, 2 x 32 + 8 = 72 held to the skeleton's
/// 64, so fids bind her live sockets below 32 (two each beside her directories), while a socket in
/// TIME-WAIT holds no fid; her `State` units are 4 + 32 = 36.
#[test]
fn the_images_account_override_fits() {
    let image = [
        "addr=10.0.2.15/24",
        "gateway=10.0.2.2",
        "ingress=3",
        "scope=4:c:0.0.0.0/0:1-65535",
        "scope=6:l:22",
        "buckets=7",
        "account=1001:5:4:32",
    ];
    let sizing = sizing_of(&image, IMAGE_BUDGET).unwrap();
    // A default bucket is 712 704 bytes; alice's is 40 fids of 512 and 24 units of 17 408 more,
    // 1 150 976; with six defaults 5 427 200 bytes, 1 325 pages, which with the heap cap's 74 and
    // the stack's 128 is 1 527 of the entry's 4 096.
    let default = sizing_of(&image[..6], IMAGE_BUDGET).unwrap().need / 7;
    assert_eq!(default, 712_704);
    assert_eq!(SOCKET_BYTES, 17_408);
    assert_eq!(default + 40 * 512 + 24 * SOCKET_BYTES, 1_150_976);
    assert_eq!(sizing.need, 6 * default + 1_150_976);
    assert_eq!(sizing.need, 5_427_200);
    let page = redoubt_rt::abi::PAGE_SIZE as u64;
    assert_eq!(sizing.need.div_ceil(page) + 74 + redoubt_rt::server::LAUNCHED_STACK_PAGES, 1527);
    // Sockets at their worst: six defaults of 12 units and alice's 36, 108 where the seven
    // defaults were 84.
    assert_eq!(sizing.max_sockets, 6 * 12 + 36);
    let caps: Vec<_> = sizing.caps.overrides.iter().map(|o| (o.account, o.badge, o.state)).collect();
    assert_eq!(caps, vec![(1001, 0, 36)]);
    assert_eq!(files_for(32), 64);
    // The override is alice's unlabelled bucket's alone: her vault's bucket, bob's, and an
    // account-0 badge numbered 1001 keep the default 12, at the stack and at the admission.
    let key = |badge, account, labels: &[u64]| {
        AdmitKey::of(&Caller { badge, account, labels: Labels::from_slice(labels).unwrap() })
    };
    let mut admission = sizing.admission;
    let alice = key(1 << 63, 1001, &[]);
    assert_eq!(sizing.caps.of(alice), 36);
    let taken = (0..64).take_while(|_| admission.admit(alice, 1, Resource::State).is_ok()).count();
    assert_eq!(taken, 18, "a lone share takes half of 36");
    for other in [key(1 << 63, 1001, &[7]), key(1 << 63, 1002, &[]), key(1001, 0, &[])] {
        assert_eq!(sizing.caps.of(other), 12, "{other:?}");
        while admission.admit(other, 1, Resource::State).is_ok() {}
        assert!(admission.held(other, Resource::State) <= 12, "{other:?}");
    }
}

/// Sizing: the worst case, every override's bucket and every other at the defaults, must leave
/// `MAX_OPEN_CALLS`' headroom and fit the budget, or `ipd` does not start.
#[test]
fn the_worst_case_must_fit_or_ipd_does_not_start() {
    let with = |extra: &[&str]| {
        let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "scope=4:c:0.0.0.0/0:1-65535", "scope=5:l:22"];
        args.extend_from_slice(extra);
        sizing_of(&args, IMAGE_BUDGET).map(|_| ())
    };
    // 6 x 5 = 30.
    assert!(with(&["buckets=6"]).is_ok());
    // A tight budget binds before the headroom: defaults alone stay inside `BOUND` (`MAX_BUCKETS`
    // x 5), and the budget holds fewer parked calls than `BOUND`, so a worst case at `BOUND`,
    // overrides at the largest `limits=` takes (64) and defaults, is refused by the budget.
    assert!(MAX_BUCKETS as usize * 5 <= BOUND);
    assert!(TIGHT_BUDGET / PARKED_BYTES < BOUND as u64);
    let full = BOUND / 64;
    let defaults = BOUND % 64 / 5;
    let at = |buckets: usize| {
        let mut args = vec![format!("buckets={buckets}"), "addr=10.0.2.15/24".into(), "ingress=3".into()];
        args.extend((10..10 + full).map(|b| format!("scope={b}:l:22")));
        args.extend((10..10 + full).map(|b| format!("limits={b}:64:0:8")));
        sizing_of(&args.iter().map(String::as_str).collect::<Vec<_>>(), TIGHT_BUDGET).map(|_| ())
    };
    assert_eq!(at(full + defaults), Err(BadArgs("every bucket at its cap does not fit the budget")));
    // An override of 1 is below the smallest useful cap.
    assert!(with(&["buckets=4", "limits=5:1:0:8"]).is_err());
    // More overrides than buckets.
    assert!(with(&["buckets=1", "limits=4:2:2:0", "limits=5:2:0:2"]).is_err());
    // Sockets that do not fit a tight budget: 8 buckets of 64 sockets.
    let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "buckets=8"];
    let scopes: Vec<String> = (10..18).map(|b| format!("scope={b}:l:22")).collect();
    let limits: Vec<String> = (10..18).map(|b| format!("limits={b}:2:0:64")).collect();
    args.extend(scopes.iter().map(String::as_str));
    args.extend(limits.iter().map(String::as_str));
    assert!(sizing_of(&args, TIGHT_BUDGET).is_err(), "8 x 64 sockets of 17 KiB fit 7 MiB?");
}
