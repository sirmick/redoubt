//! `ipd`'s sizing (servers/ipd.md, "Sizing"): the worst case, every
//! override's bucket and every other at the defaults, must leave `MAX_OPEN_CALLS`' headroom
//! (`OPEN_CALL_HEADROOM`) and fit the budget, and the heap must hold every socket's buffers, or
//! `ipd` does not start.

use redoubt_ipd::args::{BadArgs, parse};
use redoubt_ipd::sizing::{HEAP_BASE_PAGES, PARKED_BYTES, SOCKET_BYTES, files_for, heap_pages_for};
use redoubt_rt::abi::Labels;
use redoubt_rt::abi::MAX_OPEN_CALLS;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::admit::OPEN_CALL_HEADROOM;
use redoubt_rt::server::{AdmitKey, LAUNCHED_STACK_PAGES, MAX_BUCKETS, Resource, budget_of};

/// The open calls every bucket at its cap may hold together.
const BOUND: usize = MAX_OPEN_CALLS - OPEN_CALL_HEADROOM;

const PAGE: u64 = redoubt_rt::abi::PAGE_SIZE as u64;
/// The image's `ipd` entry: 4 096 pages with a heap cap of 533.
const IMAGE_PAGES: u64 = 4096;
const IMAGE_HEAP: u64 = 533;
/// Its budget, as the program derives it from its startup block (`redoubt_rt::server::budget`).
const IMAGE_BUDGET: u64 = budget_of(IMAGE_PAGES, IMAGE_HEAP);
/// A budget that holds fewer parked calls than `BOUND`, so that it binds before the headroom.
const TIGHT_BUDGET: u64 = 7 << 20;

fn sizing_of(args: &[&str], budget: u64, heap: u64) -> Result<redoubt_ipd::sizing::Sizing, BadArgs> {
    parse(args.iter().copied()).unwrap().sizing(budget, heap)
}

/// The image's own arguments.
const IMAGE: [&str; 7] = [
    "addr=10.0.2.15/24",
    "gateway=10.0.2.2",
    "ingress=3",
    "scope=4:c:0.0.0.0/0:1-65535",
    "scope=6:l:22",
    "buckets=7",
    "account=1001:5:4:32",
];

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
    assert!(sizing_of(&every_scope, IMAGE_BUDGET, IMAGE_HEAP).is_ok());
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
    let sizing = sizing_of(&milestone, IMAGE_BUDGET, IMAGE_HEAP).unwrap();
    // Sockets are `State` units: sshd's 0 + 20, the steward's 32 + 0, and four defaults of 4 + 8.
    // At their worst: 20, 32, and 4 x 12, each override slot at the larger of its units and the
    // default's.
    assert_eq!(sizing.max_sockets, 20 + 32 + 4 * 12);
    let caps: Vec<_> = sizing.caps.overrides.iter().map(|o| (o.account, o.badge, o.state)).collect();
    assert_eq!(caps, vec![(0, 5, 20), (0, 4, 32)]);
    // The entry rule (servers/walfsd.md, "Serving"), a unit costed as a connection: sshd's slot
    // 23 parked calls of 64 KiB, 48 fids and 20 units of 512 and the multiplexed 163 840, so
    // 1 705 984; the steward's at the default's 5 calls and 24 fids and its own 32 units, 520 192;
    // four defaults of 509 952; 4 265 984 bytes, 1 042 pages. Its heap holds 100 sockets: 74 +
    // 425 = 499. With the stack's 128, 1 669 of an entry of 4 096; a seventh bucket adds a
    // default, 4 775 936 (its heap aside).
    assert_eq!(sizing.need, 1_705_984 + 520_192 + 4 * 509_952);
    assert_eq!(sizing.need, 4_265_984);
    assert_eq!(sizing.heap_pages, 499);
    assert_eq!(sizing.need.div_ceil(PAGE) + sizing.heap_pages + LAUNCHED_STACK_PAGES, 1669);
    let mut seven = milestone.to_vec();
    seven[6] = "buckets=7";
    assert_eq!(sizing_of(&seven, IMAGE_BUDGET, u64::MAX).unwrap().need, 4_775_936);
    // An override below the default counts as the default: its slot can go to a default bucket.
    let mut small = milestone;
    small[8] = "limits=4:2:2:0";
    assert_eq!(sizing_of(&small, IMAGE_BUDGET, IMAGE_HEAP).unwrap().max_sockets, 20 + 12 + 4 * 12);
}

/// The image's arguments: seven buckets, alice's account (1001) at 5 parked calls, 4 connections
/// and 32 sockets. Her bucket's fids are `files_for(32)`, 2 x 32 + 8 = 72 held to the skeleton's
/// 64, so fids bind her live sockets below 32 (two each beside her directories), while a socket in
/// TIME-WAIT holds no fid; her `State` units are 4 + 32 = 36.
#[test]
fn the_images_account_override_fits() {
    let sizing = sizing_of(&IMAGE, IMAGE_BUDGET, IMAGE_HEAP).unwrap();
    // A default bucket is 5 parked calls of 64 KiB, 24 fids and 12 units of 512 and the
    // multiplexed requests' 128 x 256 and 32 pages, 509 952 bytes; alice's is 40 fids and 24 units
    // more, 542 720; with six defaults 3 602 432 bytes, 880 pages.
    let default = sizing_of(&IMAGE[..6], IMAGE_BUDGET, IMAGE_HEAP).unwrap().need / 7;
    assert_eq!(default, 5 * PARKED_BYTES + (24 + 12) * 512 + 128 * 256 + 32 * PAGE);
    assert_eq!(default, 509_952);
    assert_eq!(default + (40 + 24) * 512, 542_720);
    assert_eq!(sizing.need, 6 * default + 542_720);
    assert_eq!(sizing.need, 3_602_432);
    // Sockets at their worst: six defaults of 12 units and alice's 36, 108 where the seven
    // defaults were 84. Each is 17 408 bytes of heap, 4.25 pages, so the heap holds the base's 74
    // and 459: 533, where the seven defaults need 74 + 357 = 431 and the old cap of 74 held about
    // 8 above its idle peak of 37.
    assert_eq!(sizing.max_sockets, 6 * 12 + 36);
    assert_eq!(SOCKET_BYTES, 17_408);
    assert_eq!(sizing.heap_pages, HEAP_BASE_PAGES + 459);
    assert_eq!(sizing.heap_pages, IMAGE_HEAP);
    assert_eq!(heap_pages_for(84), 431);
    // The entry: 880 + 533 + 128 = 1 541 of 4 096.
    assert_eq!(sizing.need.div_ceil(PAGE) + IMAGE_HEAP + LAUNCHED_STACK_PAGES, 1541);
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

/// The heap holds every socket's buffers or `ipd` does not start: a heap cap at least the base
/// and every socket, or with no cap the budget holding them beside the buckets.
#[test]
fn a_heap_that_cannot_hold_every_socket_stops_ipd() {
    let short = Err(BadArgs("the heap does not hold every socket's buffers"));
    assert!(sizing_of(&IMAGE, budget_of(IMAGE_PAGES, 533), 533).is_ok());
    assert_eq!(sizing_of(&IMAGE, budget_of(IMAGE_PAGES, 532), 532).map(|_| ()), short);
    // The cap that held the seven defaults' idle peak twice over, and not their sockets.
    assert_eq!(sizing_of(&IMAGE[..6], budget_of(IMAGE_PAGES, 74), 74).map(|_| ()), short);
    assert!(sizing_of(&IMAGE[..6], budget_of(IMAGE_PAGES, 431), 431).is_ok());
    // Uncapped, the heap is the budget's: 880 pages of buckets and 533 of heap.
    let need = 3_602_432;
    assert!(sizing_of(&IMAGE, need + 533 * PAGE, 0).is_ok());
    assert_eq!(sizing_of(&IMAGE, need + 533 * PAGE - 1, 0).map(|_| ()), short);
}

/// Sizing: the worst case, every override's bucket and every other at the defaults, must leave
/// `MAX_OPEN_CALLS`' headroom and fit the budget, or `ipd` does not start.
#[test]
fn the_worst_case_must_fit_or_ipd_does_not_start() {
    let with = |extra: &[&str]| {
        let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "scope=4:c:0.0.0.0/0:1-65535", "scope=5:l:22"];
        args.extend_from_slice(extra);
        sizing_of(&args, IMAGE_BUDGET, IMAGE_HEAP).map(|_| ())
    };
    // 6 x 5 = 30.
    assert!(with(&["buckets=6"]).is_ok());
    // A tight budget binds before the headroom: defaults alone stay inside `BOUND` (`MAX_BUCKETS`
    // x 5), and the budget holds fewer parked calls than `BOUND`, so a worst case at `BOUND`,
    // overrides at the largest `limits=` takes (64) and defaults, is refused by the budget, the
    // heap aside.
    assert!(MAX_BUCKETS as usize * 5 <= BOUND);
    assert!(TIGHT_BUDGET / PARKED_BYTES < BOUND as u64);
    let full = BOUND / 64;
    let defaults = BOUND % 64 / 5;
    let at = |buckets: usize| {
        let mut args = vec![format!("buckets={buckets}"), "addr=10.0.2.15/24".into(), "ingress=3".into()];
        args.extend((10..10 + full).map(|b| format!("scope={b}:l:22")));
        args.extend((10..10 + full).map(|b| format!("limits={b}:64:0:8")));
        sizing_of(&args.iter().map(String::as_str).collect::<Vec<_>>(), TIGHT_BUDGET, u64::MAX).map(|_| ())
    };
    assert_eq!(at(full + defaults), Err(BadArgs("every bucket at its cap does not fit the budget")));
    // An override of 1 is below the smallest useful cap.
    assert!(with(&["buckets=4", "limits=5:1:0:8"]).is_err());
    // More overrides than buckets.
    assert!(with(&["buckets=1", "limits=4:2:2:0", "limits=5:2:0:2"]).is_err());
    // Sockets an uncapped heap cannot hold in a tight budget: 8 buckets of 64, 2 176 pages.
    let mut args = vec!["addr=10.0.2.15/24", "ingress=3", "buckets=8"];
    let scopes: Vec<String> = (10..18).map(|b| format!("scope={b}:l:22")).collect();
    let limits: Vec<String> = (10..18).map(|b| format!("limits={b}:2:0:64")).collect();
    args.extend(scopes.iter().map(String::as_str));
    args.extend(limits.iter().map(String::as_str));
    assert_eq!(heap_pages_for(8 * 64), HEAP_BASE_PAGES + 2176);
    assert_eq!(
        sizing_of(&args, TIGHT_BUDGET, 0).map(|_| ()),
        Err(BadArgs("the heap does not hold every socket's buffers")),
        "8 x 64 sockets of 17 KiB fit 7 MiB?"
    );
}
