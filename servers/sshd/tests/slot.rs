//! A slot (servers/sshd.md, "The box's platform"): how many there are, how long one is held
//! before login, and that it comes back after every connection, whichever side ends it first.
//!
//! - **The count is the manifest's.** `slots` takes `buckets=N` and the entry's budget: the image's entry
//!   gives four, and their stacks' pages are the arithmetic on the page; an entry whose budget cannot hold
//!   them is refused. Every manifest in the tree keeps N + 1 within the parked calls `ipd`'s bucket for
//!   `sshd` holds, read from `ipd`'s own arguments there.
//! - **The bound.** A connection not logged in is due at `LOGIN_US` and not before; one logged in is never
//!   due.
//! - **The reader's end.** Before, a driver that had already taken the reader's `EOF` (the client hung up
//!   first) waited for a second one that never came, and the slot never returned: four ordinary sessions
//!   spent every slot.

use std::path::{Path, PathBuf};

use redoubt_rt::abi::PAGE_SIZE;
use redoubt_rt::server::{LAUNCHED_STACK_PAGES, budget_of};
use redoubt_sshd::slot::{
    DATA, DRIVER_STACK_PAGES, EOF, Gate, LOGIN_US, READER_STACK_PAGES, Read, Reader, Unsized,
    WATCHER_STACK_PAGES, slots, stack_pages,
};

/// The image's `sshd` entry: its pages, its heap cap and its `buckets=N`
/// (`image/manifest.json`), as [`the_image_entry_holds_its_slots`] reads them back.
const IMAGE_PAGES: u64 = 1024;
const IMAGE_HEAP: u64 = 384;
const IMAGE_BUCKETS: u32 = 4;

/// The image's slots, from its entry.
fn image_slots() -> usize { slots(IMAGE_BUCKETS, budget_of(IMAGE_PAGES, IMAGE_HEAP)).unwrap() }

/// One connection on `busy`'s first free slot, as the program's main thread and driver run it:
/// the reader's calls in `during` reach the driver while it runs the connection, and once it is
/// over the driver waits for the reader's last call only if it has not had it. Returns the slot,
/// or `None` if every slot was busy.
fn connection(busy: &mut [bool], during: &[u64]) -> Option<usize> {
    let s = busy.iter().position(|b| !b)?;
    busy[s] = true;
    let mut reader = Reader::default();
    for word in during {
        reader.took(*word);
    }
    if reader.waits() {
        // The socket is closed now; the reader's last call comes, and is taken.
        assert_eq!(reader.took(EOF), Read::End);
    }
    assert!(!reader.waits());
    busy[s] = false;
    Some(s)
}

#[test]
fn a_slot_returns_when_the_client_hangs_up_first() {
    let mut busy = vec![false; image_slots()];
    for n in 0..3 * busy.len() {
        assert_eq!(connection(&mut busy, &[DATA, DATA, EOF]), Some(0), "connection {n}");
    }
}

#[test]
fn a_slot_returns_when_the_server_ends_the_connection_first() {
    let mut busy = vec![false; image_slots()];
    for n in 0..3 * busy.len() {
        assert_eq!(connection(&mut busy, &[DATA]), Some(0), "connection {n}");
    }
}

#[test]
fn only_data_is_data() {
    let mut reader = Reader::default();
    assert_eq!(reader.took(DATA), Read::Data);
    assert!(reader.waits());
    assert_eq!(reader.took(EOF), Read::End);
    assert!(!reader.waits());
}

/// The bound is twice the slowest login the bench recorded, 2.41 s, rounded up to a second.
#[test]
fn the_bound_is_twice_the_slowest_login_rounded_up() {
    let slowest_us = 2_410_000u64;
    assert_eq!(LOGIN_US, (2 * slowest_us).div_ceil(1_000_000) * 1_000_000);
    assert_eq!(LOGIN_US, 5_000_000);
}

#[test]
fn a_connection_not_logged_in_is_closed_at_the_bound_and_not_before() {
    let started = 1_000_000;
    let gate = Gate::new(started);
    assert_eq!(gate.deadline(), Some(started + LOGIN_US));
    assert!(!gate.due(started));
    assert!(!gate.due(started + LOGIN_US - 1));
    assert!(gate.due(started + LOGIN_US));
    assert!(gate.due(started + 10 * LOGIN_US));
}

#[test]
fn a_connection_logged_in_is_never_closed_by_the_bound() {
    let started = 1_000_000;
    let mut gate = Gate::new(started);
    gate.logged_in();
    assert_eq!(gate.deadline(), None);
    assert!(!gate.due(started + LOGIN_US));
    assert!(!gate.due(u64::MAX));
}

/// The arithmetic on the page: a slot's two stacks are 48 + 4 pages, the watcher's 4; the image's
/// four slots take 212 of the 512 pages its entry leaves (1,024 less the heap cap's 384 and the
/// first stack's bound of 128); nine would fit, ten would not.
#[test]
fn the_image_entry_holds_its_slots() {
    assert_eq!(DRIVER_STACK_PAGES, 48);
    assert_eq!(READER_STACK_PAGES, 4);
    assert_eq!(WATCHER_STACK_PAGES, 4);
    assert_eq!(stack_pages(4), 212);
    let budget = budget_of(IMAGE_PAGES, IMAGE_HEAP);
    assert_eq!(budget, (1024 - 384 - LAUNCHED_STACK_PAGES) * PAGE_SIZE as u64);
    assert_eq!(budget / PAGE_SIZE as u64, 512);
    assert_eq!(slots(IMAGE_BUCKETS, budget), Ok(4));
    assert_eq!(slots(9, budget), Ok(9));
    assert_eq!(slots(10, budget), Err(Unsized { slots: 10, need: 524, have: 512 }));
    // The entry the refusal case boots: the image's 1,024 pages with a 700-page heap cap leave 196.
    assert_eq!(slots(4, budget_of(1024, 700)), Err(Unsized { slots: 4, need: 212, have: 196 }));
    // The image's entry, read back, is what the constants say.
    let image = manifest(&root().join("image/manifest.json"));
    let entry = sshd_entries(&image).next().expect("the image's sshd entry");
    assert_eq!((entry.pages, entry.heap, entry.buckets), (IMAGE_PAGES, IMAGE_HEAP, Some(IMAGE_BUCKETS)));
}

/// Every manifest's `sshd` entry keeps its slots within what its `ipd` holds for them: N parked
/// reads and the listener's accept within the in-flight calls `ipd`'s bucket for `sshd`'s badge
/// allows, its default or the entry's `limits=` override for that badge, and its sockets within
/// the bucket's. `sshd` cannot check this at start, since it does not read `ipd`'s entry; the
/// tree's manifests are checked here instead, so no boot meets `too many` on every slot's read.
#[test]
fn every_manifests_slots_fit_ipds_parked_calls() {
    let root = root();
    let mut found = vec![root.join("image/manifest.json")];
    manifests_under(&root.join("tests/data"), &mut found);
    let mut checked = 0;
    for path in found {
        let json = manifest(&path);
        let Some(ipd) = ipd_config(&json) else { continue };
        for entry in sshd_entries(&json) {
            let Some(buckets) = entry.buckets else { continue };
            let slots = slots(buckets, budget_of(entry.pages, entry.heap))
                .unwrap_or_else(|e| panic!("{path:?} {}: {e:?}", entry.name));
            let badge = entry.ipd_badge.unwrap_or_else(|| panic!("{path:?} {}: no ipd badge", entry.name));
            let held = ipd.limits.iter().find(|l| l.badge == badge);
            let in_flight = held.map_or(redoubt_ipd::sizing::DEFAULT_IN_FLIGHT, |l| l.in_flight);
            let sockets = held.map_or(redoubt_ipd::sizing::DEFAULT_SOCKETS, |l| l.sockets);
            assert!(
                slots + 1 <= in_flight as usize,
                "{path:?} {}: {slots} slots park {slots} reads beside the accept, {} calls, and ipd \
                 holds {in_flight} for badge {badge}",
                entry.name,
                slots + 1
            );
            assert!(
                slots + 1 <= sockets as usize,
                "{path:?} {}: {slots} slots and the listener are {} sockets, and ipd holds {sockets} \
                 for badge {badge}",
                entry.name,
                slots + 1
            );
            checked += 1;
        }
    }
    assert!(checked >= 10, "the tree's manifests with an sshd entry were not found ({checked})");
}

/// The checkout's root: this crate is `servers/sshd`.
fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }

fn manifest(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// Every `.json` under `dir`, into `out`.
fn manifests_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            manifests_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// A manifest's number: a JSON number or a decimal string, as `init` reads them.
fn number(v: &serde_json::Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or_else(|| panic!("{v}"))
}

/// An `sshd` entry as the tests read it.
struct Entry {
    name: String,
    pages: u64,
    heap: u64,
    buckets: Option<u32>,
    /// The badge it is handed at `ipd`.
    ipd_badge: Option<u64>,
}

fn sshd_entries(json: &serde_json::Value) -> impl Iterator<Item = Entry> + '_ {
    json["servers"].as_array().into_iter().flatten().filter(|s| s["program"] == "sshd").map(|s| {
        let args: Vec<&str> = s["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
        let ipd_badge = s["handed"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|h| h["endpoint"] == "ipd")
            .map(|h| number(&h["badge"]));
        Entry {
            name: s["name"].as_str().unwrap_or("").to_string(),
            pages: number(&s["budget"]["pages"]),
            heap: s.get("heap_pages").map_or(0, number),
            buckets: redoubt_rt::server::buckets(&args).ok(),
            ipd_badge,
        }
    })
}

/// The manifest's `ipd` entry, parsed as `ipd` parses it; `None` without one that parses (an
/// `init` refusal case's).
fn ipd_config(json: &serde_json::Value) -> Option<redoubt_ipd::args::Config> {
    let ipd = json["servers"].as_array()?.iter().find(|s| s["program"] == "ipd")?;
    let args: Vec<&str> = ipd["args"].as_array()?.iter().filter_map(|a| a.as_str()).collect();
    redoubt_ipd::args::parse(args.iter().copied()).ok()
}
