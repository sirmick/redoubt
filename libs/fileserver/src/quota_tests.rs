use alloc::string::String;
use alloc::vec;

use super::{Ledger, Refusal, under};

/// A count that finds `held` bytes and `reserve` kept below.
fn found(held: u64, reserve: u64) -> impl FnOnce() -> Result<(u64, u64), ()> { move || Ok((held, reserve)) }

/// A count that must not run: the root is live already.
fn uncounted() -> Result<(u64, u64), ()> { panic!("a live root counted again") }

/// A volume of 1000 bytes holding 100, its root the directory 7, with the directory 5 at `a`
/// minted by badge 1 with 300 bytes, which finds 50 of the root's bytes there.
fn carved() -> Ledger {
    let mut ledger = Ledger::new(1000, 100, 7);
    assert_eq!(ledger.mint(0, 1, 5, "a", 300, found(50, 0)), Ok(()));
    ledger
}

/// A path is under a directory only at a `/`, and everything is under the volume's root.
#[test]
fn a_path_is_under_a_directory_only_past_a_slash() {
    assert!(under("a", "a"));
    assert!(under("a/b", "a"));
    assert!(!under("ab", "a"));
    assert!(!under("a", "a/b"));
    assert!(under("x/y", ""));
}

/// The first mint at a directory counts it: what it holds leaves the root above, which keeps the
/// new root's quota in reserve, and the new root may grow to its quota and no further.
#[test]
fn a_mint_at_a_new_root_counts_it_and_charges_the_root_above() {
    let ledger = carved();
    let roots = ledger.roots();
    assert_eq!(roots, vec![(7, String::new(), 1000, 50, 300), (5, String::from("a"), 300, 50, 0)]);
    assert_eq!(ledger.charges(), Some(vec![(7, 1000), (5, 300)]));
    assert_eq!((ledger.holder("a/b"), ledger.holder("ab"), ledger.holder("")), (1, 0, 0));
    assert_eq!(ledger.spare(1), 250);
    assert!(ledger.fits(1, 250));
    assert!(!ledger.fits(1, 251));
    assert_eq!(ledger.spare(0), 650);
    assert!(ledger.holds_live("a") && ledger.holds_live("") && !ledger.holds_live("b"));
}

/// A quota the root above has no room for is refused and recorded nowhere; a count that fails is
/// the count's error, and records nothing either.
#[test]
fn a_mint_past_the_room_above_is_refused() {
    let mut ledger = carved();
    assert_eq!(ledger.mint(0, 2, 6, "b", 651, found(0, 0)), Err(Refusal::Refused));
    assert_eq!(ledger.mint(1, 2, 8, "a/c", 251, found(0, 0)), Err(Refusal::Refused));
    assert_eq!(ledger.mint(0, 2, 6, "b", 10, || Err("unreadable")), Err(Refusal::Count("unreadable")));
    assert_eq!(ledger.roots().len(), 2);
    assert_eq!(ledger.mint(0, 2, 6, "b", 650, found(0, 0)), Ok(()));
    assert_eq!(ledger.spare(0), 0);
}

/// A connection minted at its granter's own root is that root and carves nothing: a quota there
/// is refused. The volume's root is the directory the ledger was made with.
#[test]
fn a_quota_at_the_granters_own_root_is_refused() {
    let mut ledger = carved();
    assert_eq!(ledger.mint(0, 2, 7, "", 1, uncounted), Err(Refusal::Refused));
    assert_eq!(ledger.mint(1, 2, 5, "a", 1, uncounted), Err(Refusal::Refused));
    assert_eq!(ledger.mint(1, 2, 5, "a", 0, uncounted), Ok(()));
    assert_eq!(ledger.roots()[1].2, 300);
}

/// Two connections at one root sum their quotas, the second charged to the root above for what
/// it adds; the root's record goes with its last connection, and what it held returns above.
#[test]
fn connections_at_one_root_sum_and_the_last_disconnect_returns_its_bytes() {
    let mut ledger = carved();
    ledger.change(1, 20, 0);
    assert_eq!(ledger.mint(0, 2, 5, "a", 200, uncounted), Ok(()));
    assert_eq!(ledger.roots()[1], (5, String::from("a"), 500, 70, 0));
    assert_eq!(ledger.spare(0), 450);
    assert_eq!(ledger.mint(0, 3, 5, "a", 451, uncounted), Err(Refusal::Refused));

    ledger.disconnect(1);
    assert_eq!(ledger.roots()[1].2, 200);
    ledger.disconnect(2);
    assert_eq!(ledger.roots(), vec![(7, String::new(), 1000, 120, 0)]);
    ledger.disconnect(2);
    assert_eq!(ledger.spare(0), 880);
}

/// A root whose quota shrinks with a disconnect below what it holds stays charged for what it
/// holds, so no room is freed above it, and grows no further until it is under its quota again.
#[test]
fn a_root_over_its_quota_after_a_disconnect_grows_no_further() {
    let mut ledger = carved();
    assert_eq!(ledger.mint(0, 2, 5, "a", 200, uncounted), Ok(()));
    assert!(ledger.fits(1, 450));
    ledger.change(1, 400, 0);
    ledger.disconnect(2);
    assert_eq!(ledger.roots()[1], (5, String::from("a"), 300, 450, 0));
    assert_eq!(ledger.charges(), Some(vec![(7, 1000), (5, 450)]));
    assert_eq!((ledger.spare(1), ledger.spare(0)), (0, 500));
    ledger.change(1, 0, 150);
    assert!(!ledger.fits(1, 1));
    ledger.change(1, 0, 1);
    assert!(ledger.fits(1, 1));
}
