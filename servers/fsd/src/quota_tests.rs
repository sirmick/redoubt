//! Quotas per attach root (servers/fsd.md, "Quotas"; R48), over the skeleton: connections are
//! minted with `new_connection` through `answer_common`, and every change is made over 9P.

use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_rt::abi::{Error, Handle, Handles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{DMDIR, Minter, mode, ninep_common};
use redoubt_rt::server::typed::TypedServer;
use redoubt_rt::wire::proto::fsd::{CopyFile, ErrorCode, Message, Rename};

use crate::server::tests::{Memory, SECTORS, T, caller};
use crate::typed::Typed;

extern crate std;
use std::string::String as StdString;

/// A kernel for `answer_common`: mints handles 100, 101, ... and remembers the badges.
struct Kernel {
    minted: Vec<u64>,
    rng: u64,
}

impl Minter for Kernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        self.minted.push(badge.get());
        Ok(Handle::new(99 + self.minted.len() as u32).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

/// A volume of [`SECTORS`], the founding caller, and a kernel to mint with.
fn volume() -> (T, Caller, Kernel) { volume_of(SECTORS) }

/// [`volume`] of `sectors`.
fn volume_of(sectors: usize) -> (T, Caller, Kernel) {
    let t = T::on(&Memory::blank(sectors), &[]);
    (t, caller(1, &[]), Kernel { minted: Vec::new(), rng: 0x9e37_79b9_7f4a_7c15 })
}

/// The ledger agrees with a fresh count of every live root, each within its quota.
fn audit(t: &mut T) { t.server.fs.audit(false) }

/// The volume root's room: the volume's quota, less what it holds and keeps in reserve.
fn room(t: &T) -> u64 {
    let (_, _, quota, held, reserve) = t.server.fs.ledger.roots()[0].clone();
    quota - held - reserve
}

/// `rename` as `who`, its fids on directories.
fn rename(
    t: &mut T,
    who: &Caller,
    old_dir: u32,
    old_name: &str,
    new_dir: u32,
    new_name: &str,
) -> Result<(), ErrorCode> {
    let message = Message::Rename(Rename { old_dir, old_name, new_dir, new_name });
    Typed(&mut t.server).handle(who, message, &[]).map(|_| ())
}

/// `new_connection(root, quota)` from `who`: the caller using the new connection, and its id; or
/// the error status.
fn mint(t: &mut T, k: &mut Kernel, who: &Caller, root: &str, quota: u64) -> Result<(Caller, u64), u32> {
    let message = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
    let mut lend = vec![0u8; 4096];
    let words = message.encode(&mut lend).unwrap();
    let outcome = t.server.answer_common(who, &words, &Handles::new(), &mut lend, k);
    if outcome.words[0] != 0 {
        return Err(outcome.words[0] as u32);
    }
    let sent = outcome.send.as_slice().len();
    let reply = ninep_common::Reply::decode(2, &outcome.words, &lend, sent).unwrap().unwrap();
    let ninep_common::Reply::NewConnection(reply) = reply else { panic!("{reply:?}") };
    Ok((Caller { badge: *k.minted.last().unwrap(), ..*who }, reply.id))
}

/// `disconnect(id)` from `who`.
fn disconnect(t: &mut T, k: &mut Kernel, who: &Caller, id: u64) {
    let message = ninep_common::Message::Disconnect(ninep_common::Disconnect { id });
    let words = message.encode(&mut []).unwrap();
    let outcome = t.server.answer_common(who, &words, &Handles::new(), &mut [], k);
    assert_eq!(outcome.words, [0, 0, 0, 0]);
}

/// `ninep_common`'s `refused`.
const REFUSED: u32 = 3;

/// Makes the directories `names` under `who`'s root (fid 0, attached here).
fn dirs(t: &mut T, who: &Caller, names: &[&str]) {
    t.attach(who, 0).unwrap();
    for name in names {
        t.walk(who, 0, 1, &[]).unwrap();
        t.create(who, 1, name, DMDIR | 0o755, mode::OREAD).unwrap();
        t.clunk(who, 1).unwrap();
    }
}

/// Creates `name` under `who`'s root (fid 0) as fid `fid`, open for writing.
fn file(t: &mut T, who: &Caller, fid: u32, name: &str) -> Result<(), StdString> {
    t.walk(who, 0, fid, &[]).unwrap();
    let made = t.create(who, fid, name, 0o644, mode::ORDWR);
    if made.is_err() {
        t.clunk(who, fid).unwrap();
    }
    made
}

/// Writes 4 KiB at a time to the end of `fid` until a write is refused; the bytes written.
fn fill(t: &mut T, who: &Caller, fid: u32) -> u64 {
    let chunk = [5u8; 4096];
    let mut size = 0;
    while t.write(who, fid, size, &chunk).is_ok() {
        size += chunk.len() as u64;
    }
    assert_eq!(t.write(who, fid, size, &chunk).unwrap_err(), "no space");
    size
}

/// R48's attack: Bob fills his root to its quota and is refused, and Alice, on another root of
/// the same volume, still writes, as does the volume's root. The refusal is fsd's, by the
/// quota it recorded at the mint, not anything Bob reports.
#[test]
fn a_write_past_one_roots_quota_is_refused_while_another_still_writes() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["alice", "bob"]);
    let (alice, _) = mint(&mut t, &mut k, &base, "alice", 64 * 1024).unwrap();
    let (bob, _) = mint(&mut t, &mut k, &base, "bob", 64 * 1024).unwrap();
    t.attach(&bob, 0).unwrap();
    file(&mut t, &bob, 1, "hog").unwrap();
    let filled = fill(&mut t, &bob, 1);
    assert!((32 * 1024..64 * 1024).contains(&filled), "Bob wrote {filled}");
    assert_eq!(t.stat(&bob, 1).unwrap().1, filled, "the refused write changed nothing");
    t.attach(&alice, 0).unwrap();
    file(&mut t, &alice, 1, "save").unwrap();
    assert_eq!(t.write(&alice, 1, 0, &[1u8; 32 * 1024]), Ok(32 * 1024));
    file(&mut t, &base, 2, "log").unwrap();
    assert_eq!(t.write(&base, 2, 0, &[2u8; 32 * 1024]), Ok(32 * 1024));
}

/// R48's attack: a root minted with quota 0 can read and remove what is there, but cannot
/// create a file or a directory, or grow a file.
#[test]
fn a_root_with_quota_0_cannot_create_but_can_read_and_remove() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["z"]);
    t.walk(&base, 0, 1, &["z"]).unwrap();
    t.create(&base, 1, "old", 0o644, mode::OWRITE).unwrap();
    t.write(&base, 1, 0, b"kept").unwrap();
    let (z, _) = mint(&mut t, &mut k, &base, "z", 0).unwrap();
    t.attach(&z, 0).unwrap();
    assert_eq!(file(&mut t, &z, 1, "new").unwrap_err(), "no space");
    t.walk(&z, 0, 1, &[]).unwrap();
    assert_eq!(t.create(&z, 1, "sub", DMDIR | 0o755, mode::OREAD).unwrap_err(), "no space");
    t.clunk(&z, 1).unwrap();
    t.walk(&z, 0, 1, &["old"]).unwrap();
    t.open(&z, 1, mode::ORDWR).unwrap();
    assert_eq!(t.read(&z, 1, 0, 64).unwrap(), b"kept");
    assert_eq!(t.write(&z, 1, 4, b"more").unwrap_err(), "no space");
    t.remove(&z, 1).unwrap();
    assert_eq!(t.walk(&base, 0, 2, &["z", "old"]).unwrap_err(), "partial walk", "removed");
}

/// A mint the parent's room cannot take is `refused` and mints nothing; disconnecting gives
/// the carve back, and the same mint then fits.
#[test]
fn a_mint_the_room_cannot_take_is_refused_and_disconnect_gives_it_back() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "b"]);
    // a's directory pair moves out of the volume root's count with it.
    let half = room(&t) / 2 + 16 * 1024;
    assert_eq!(mint(&mut t, &mut k, &base, "a", 4 * half).unwrap_err(), REFUSED);
    assert_eq!(t.server.connections(), 0, "nothing was minted");
    let (_, a) = mint(&mut t, &mut k, &base, "a", half).unwrap();
    assert_eq!(mint(&mut t, &mut k, &base, "b", half).unwrap_err(), REFUSED);
    audit(&mut t);
    disconnect(&mut t, &mut k, &base, a);
    audit(&mut t);
    mint(&mut t, &mut k, &base, "b", half).unwrap();
    audit(&mut t);
}

/// Two connections minted at one directory share its count, and its quota is the sum of
/// theirs; when one goes the other keeps the directory, over its own quota now, and can only
/// remove until it is under.
#[test]
fn two_connections_at_one_directory_share_it() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    let (one, first) = mint(&mut t, &mut k, &base, "a", 40 * 1024).unwrap();
    let (two, _) = mint(&mut t, &mut k, &base, "a", 40 * 1024).unwrap();
    t.attach(&one, 0).unwrap();
    file(&mut t, &one, 1, "big").unwrap();
    let filled = fill(&mut t, &one, 1);
    assert!(filled > 48 * 1024, "the two quotas together: {filled}");
    t.clunk(&one, 1).unwrap();
    audit(&mut t);
    disconnect(&mut t, &mut k, &base, first);
    t.server.fs.audit(true);
    t.attach(&two, 0).unwrap();
    assert_eq!(file(&mut t, &two, 1, "more").unwrap_err(), "no space");
    t.walk(&two, 0, 1, &["big"]).unwrap();
    t.remove(&two, 1).unwrap();
    file(&mut t, &two, 1, "more").unwrap();
    audit(&mut t);
}

/// A connection minted at its granter's own root is that root: with quota 0 it carves
/// nothing and shares the root's count; with any other quota it is refused.
#[test]
fn a_connection_at_its_granters_own_root_carves_nothing() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    assert_eq!(mint(&mut t, &mut k, &base, "", 1).unwrap_err(), REFUSED);
    let before = room(&t);
    mint(&mut t, &mut k, &base, "", 0).unwrap();
    assert_eq!(room(&t), before);
    let (a, _) = mint(&mut t, &mut k, &base, "a", 32 * 1024).unwrap();
    assert_eq!(mint(&mut t, &mut k, &a, "", 4096).unwrap_err(), REFUSED);
    let (again, _) = mint(&mut t, &mut k, &a, "", 0).unwrap();
    t.attach(&a, 0).unwrap();
    file(&mut t, &a, 1, "f").unwrap();
    let filled = fill(&mut t, &a, 1);
    // The new connection shares a's count: it has no more room than a has.
    t.attach(&again, 0).unwrap();
    t.walk(&again, 0, 1, &["f"]).unwrap();
    t.open(&again, 1, mode::OWRITE).unwrap();
    assert_eq!(t.write(&again, 1, filled, &[5u8; 4096]).unwrap_err(), "no space");
    audit(&mut t);
}

/// A root minted over files counts them at the mint, and its parent keeps what they hold in
/// reserve, so the mint frees no room there, whatever the quota under the count; over its quota
/// the root can only read and remove until it is under.
#[test]
fn a_root_minted_over_files_counts_them() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "z"]);
    for (fid, dir) in [(1, "a"), (2, "z")] {
        t.walk(&base, 0, fid, &[dir]).unwrap();
        t.create(&base, fid, "old", 0o644, mode::OWRITE).unwrap();
        t.write(&base, fid, 0, &[3u8; 40 * 1024]).unwrap();
        t.clunk(&base, fid).unwrap();
    }
    let before = room(&t);
    let (a, _) = mint(&mut t, &mut k, &base, "a", 32 * 1024).unwrap();
    assert_eq!(room(&t), before, "a quota under the count frees nothing");
    mint(&mut t, &mut k, &base, "z", 0).unwrap();
    assert_eq!(room(&t), before, "nor does quota 0");
    t.server.fs.audit(true);
    t.attach(&a, 0).unwrap();
    assert_eq!(file(&mut t, &a, 1, "new").unwrap_err(), "no space");
    t.walk(&a, 0, 1, &["old"]).unwrap();
    assert_eq!(t.read(&a, 1, 0, 4).unwrap_err(), "fid not open for this");
    t.remove(&a, 1).unwrap();
    file(&mut t, &a, 1, "new").unwrap();
    t.server.fs.audit(true);
}

/// Red's attack on the carve: Bob fills his root, then mints a quota-0 connection over the
/// directory holding what he wrote, and keeps it. The files stay his parent's to hold, so his
/// next block is still `no space`.
#[test]
fn minting_over_files_frees_no_room() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["bob"]);
    let (bob, _) = mint(&mut t, &mut k, &base, "bob", 64 * 1024).unwrap();
    dirs(&mut t, &bob, &["x"]);
    t.walk(&bob, 0, 1, &["x"]).unwrap();
    t.create(&bob, 1, "f", 0o644, mode::ORDWR).unwrap();
    assert!(fill(&mut t, &bob, 1) > 32 * 1024);
    file(&mut t, &bob, 2, "y").unwrap();
    let end = fill(&mut t, &bob, 2);
    mint(&mut t, &mut k, &bob, "x", 0).unwrap();
    assert_eq!(t.write(&bob, 2, end, &[5u8; 4096]).unwrap_err(), "no space");
    t.server.fs.audit(true);
}

/// A root minted at quota 0 above a live root holds the lower root's charge and its own
/// directory, more than its quota, and its parent keeps that in reserve: no room is freed.
#[test]
fn minting_above_a_live_root_frees_no_room() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.create(&base, 1, "b", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(&base, 1).unwrap();
    mint(&mut t, &mut k, &base, "a/b", 24 * 1024).unwrap();
    let before = room(&t);
    mint(&mut t, &mut k, &base, "a", 0).unwrap();
    assert_eq!(room(&t), before);
    t.server.fs.audit(true);
}

/// A root minted above a live root becomes its nearest: the lower root's quota moves from the
/// old parent's reserve to the new root's, and back when the new root goes.
#[test]
fn a_root_minted_above_a_live_root_holds_its_quota_in_reserve() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.create(&base, 1, "b", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(&base, 1).unwrap();
    mint(&mut t, &mut k, &base, "a/b", 24 * 1024).unwrap();
    audit(&mut t);
    let before = room(&t);
    let (a, id) = mint(&mut t, &mut k, &base, "a", 64 * 1024).unwrap();
    audit(&mut t);
    let roots = t.server.fs.ledger.roots();
    let held_a = roots.iter().find(|r| r.1 == "a").unwrap();
    assert_eq!(held_a.4, 24 * 1024, "a keeps b's quota in reserve");
    // The volume root gave up a's pair and b's reserve, and carved a's quota.
    assert_eq!(room(&t) + 64 * 1024, before + 8192 + 24 * 1024);
    t.attach(&a, 0).unwrap();
    file(&mut t, &a, 1, "f").unwrap();
    let filled = fill(&mut t, &a, 1);
    assert!(filled < 40 * 1024, "a's room is its quota less b's: {filled}");
    t.clunk(&a, 1).unwrap();
    disconnect(&mut t, &mut k, &base, id);
    audit(&mut t);
}

/// littlefs rewrites a file from the first block written to its end before the commit frees
/// the old blocks: a write at the start of a large file is refused without room for its tail,
/// leaving the file as it was, and succeeds with it.
#[test]
fn a_rewrite_at_the_start_needs_room_for_the_tail() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    // The large file holds 13 blocks; rewriting all of them, and a commit, fits 128 KiB with
    // the directory's pair, but not with the filler's 5 blocks as well.
    let (a, _) = mint(&mut t, &mut k, &base, "a", 128 * 1024).unwrap();
    t.attach(&a, 0).unwrap();
    file(&mut t, &a, 1, "large").unwrap();
    t.write(&a, 1, 0, &[1u8; 48 * 1024]).unwrap();
    file(&mut t, &a, 2, "filler").unwrap();
    t.write(&a, 2, 0, &[2u8; 16 * 1024]).unwrap();
    assert_eq!(t.write(&a, 1, 0, b"new start").unwrap_err(), "no space");
    assert_eq!(t.read(&a, 1, 0, 9).unwrap(), [1u8; 9], "the refused write left the file");
    assert_eq!(t.write(&a, 1, 48 * 1024 - 9, b"a new end").unwrap(), 9, "the tail alone fits");
    t.remove(&a, 2).unwrap();
    assert_eq!(t.write(&a, 1, 0, b"new start").unwrap(), 9);
    assert_eq!(t.read(&a, 1, 0, 9).unwrap(), b"new start");
    audit(&mut t);
}

/// A rename or remove never ends a live root: moving it or a directory holding it, renaming
/// over it, or removing it, by any connection and by its own root fid, is refused.
#[test]
fn a_rename_or_remove_never_ends_a_live_root() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "x"]);
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.create(&base, 1, "b", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(&base, 1).unwrap();
    let (b, _) = mint(&mut t, &mut k, &base, "a/b", 0).unwrap();
    t.walk(&base, 0, 1, &["a"]).unwrap();
    assert_eq!(rename(&mut t, &base, 0, "a", 0, "c"), Err(ErrorCode::Refused));
    assert_eq!(rename(&mut t, &base, 1, "b", 0, "b"), Err(ErrorCode::Refused));
    assert_eq!(rename(&mut t, &base, 0, "x", 1, "b"), Err(ErrorCode::Refused));
    t.walk(&base, 0, 2, &["a", "b"]).unwrap();
    assert_eq!(t.remove(&base, 2).unwrap_err(), "permission denied");
    t.attach(&b, 0).unwrap();
    assert!(t.remove(&b, 0).is_err());
    t.walk(&base, 0, 2, &["a", "b"]).unwrap();
    // The root with quota 0 holds its own directory's pair.
    t.server.fs.audit(true);
}

/// A rename from one root's part of the tree to another's moves the bytes, and needs room in
/// the second.
#[test]
fn a_rename_between_two_roots_moves_the_bytes() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "b", "c"]);
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.create(&base, 1, "f", 0o644, mode::OWRITE).unwrap();
    t.write(&base, 1, 0, &[4u8; 24 * 1024]).unwrap();
    t.clunk(&base, 1).unwrap();
    mint(&mut t, &mut k, &base, "a", 64 * 1024).unwrap();
    mint(&mut t, &mut k, &base, "b", 16 * 1024).unwrap();
    mint(&mut t, &mut k, &base, "c", 64 * 1024).unwrap();
    for (fid, dir) in [(1, "a"), (2, "b"), (3, "c")] {
        t.walk(&base, 0, fid, &[dir]).unwrap();
    }
    assert_eq!(rename(&mut t, &base, 1, "f", 2, "f"), Err(ErrorCode::NoSpace));
    rename(&mut t, &base, 1, "f", 3, "f").unwrap();
    audit(&mut t);
}

/// A typed operation refused for want of room answers the table's `no_space`.
#[test]
fn a_copy_past_the_quota_is_no_space() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    t.walk(&base, 0, 1, &[]).unwrap();
    t.create(&base, 1, "f", 0o644, mode::OWRITE).unwrap();
    t.write(&base, 1, 0, &[5u8; 24 * 1024]).unwrap();
    let (a, _) = mint(&mut t, &mut k, &base, "a", 48 * 1024).unwrap();
    t.walk(&base, 0, 2, &["a"]).unwrap();
    let copy = |t: &mut T, name| {
        let message = Message::CopyFile(CopyFile { src_fid: 1, dst_dir: 2, dst_name: name });
        Typed(&mut t.server).handle(&base, message, &[]).map(|_| ())
    };
    assert_eq!(copy(&mut t, "one"), Ok(()));
    assert_eq!(copy(&mut t, "two"), Err(ErrorCode::NoSpace));
    let _ = a;
    audit(&mut t);
}

/// Every root filled to its quota by random creates, writes, truncations, renames and removes,
/// by connections at several roots: littlefs itself never runs out of room, and the ledger
/// always agrees with a fresh count.
#[test]
fn the_volume_never_runs_out_while_every_root_is_within_its_quota() {
    let (mut t, base, mut k) = volume_of(256 * 8);
    dirs(&mut t, &base, &["a", "b", "c"]);
    let mut who = vec![base.clone()];
    for (dir, quota) in [("a", 256 * 1024), ("b", 192 * 1024), ("c", 64 * 1024)] {
        who.push(mint(&mut t, &mut k, &base, dir, quota).unwrap().0);
    }
    for w in &who[1..] {
        t.attach(w, 0).unwrap();
    }
    let mut rng = 0x2545_f491_4f6c_dd1du64;
    let mut next = |n: u64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng % n
    };
    let mut refused = 0;
    for step in 0..1500 {
        let w = &who[next(who.len() as u64) as usize];
        let name = alloc::format!("f{}", next(12));
        let sub = alloc::format!("d{}", next(3));
        let r = match next(8) {
            0 => file(&mut t, w, 1, &name).map(|_| t.clunk(w, 1).unwrap()),
            1 => t.walk(w, 0, 1, &[]).and_then(|_| {
                let made = t.create(w, 1, &sub, DMDIR | 0o755, mode::OREAD);
                t.clunk(w, 1).unwrap();
                made
            }),
            2 | 3 | 4 => t.walk(w, 0, 1, &[&name]).and_then(|_| {
                let at = next(64 * 1024);
                let data = vec![step as u8; next(24 * 1024) as usize + 1];
                let written = t.open(w, 1, mode::OWRITE).and_then(|_| t.write(w, 1, at, &data)).map(|_| ());
                t.clunk(w, 1).unwrap();
                written
            }),
            5 => t.walk(w, 0, 1, &[&name]).and_then(|_| {
                let cut = t.open(w, 1, mode::OWRITE | mode::OTRUNC);
                t.clunk(w, 1).unwrap();
                cut
            }),
            6 => {
                let to = alloc::format!("f{}", next(12));
                rename(&mut t, w, 0, &name, 0, &to).map_err(|e| alloc::format!("{e:?}"))
            }
            _ => t.walk(w, 0, 1, &[&name]).and_then(|_| t.remove(w, 1)),
        };
        if r.is_err_and(|e| e == "no space" || e == "NoSpace") {
            refused += 1;
        }
        assert_eq!(t.server.fs.out_of_room, 0, "littlefs ran out of room at step {step}");
        if step % 50 == 0 {
            audit(&mut t);
        }
    }
    audit(&mut t);
    assert!(refused > 100, "the roots were filled: {refused} refusals");
}

/// Creates empty files with long names in `who`'s root until one is refused; how many.
fn crowd(t: &mut T, who: &Caller, most: usize) -> usize {
    for i in 0..most {
        let name = alloc::format!("a-long-name-to-crowd-the-directory-{i:04}");
        if file(t, who, 1, &name).is_err() {
            return i;
        }
        t.clunk(who, 1).unwrap();
    }
    most
}

/// The pairs the directory at `path` spans.
fn pairs(t: &mut T, path: &str) -> u32 { t.server.fs.with(|fs| fs.read_dir(path, |_| {})).unwrap() }

/// A root exactly at its quota still creates while its directory's entries fit one block:
/// littlefs is given no room for another pair, so it compacts instead of splitting, and the
/// count never passes the quota. Past one block the create is refused and changes nothing.
#[test]
fn a_root_at_its_quota_creates_while_its_entries_fit_one_pair() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    // The directory's own pair, and not a byte more.
    let (a, _) = mint(&mut t, &mut k, &base, "a", 8192).unwrap();
    t.attach(&a, 0).unwrap();
    let made = crowd(&mut t, &a, 500);
    assert!(made > 20, "{made} entries in one pair");
    assert_eq!(pairs(&mut t, "a"), 1, "never split");
    assert_eq!(
        t.walk(&a, 0, 1, &[&alloc::format!("a-long-name-to-crowd-the-directory-{made:04}")]).unwrap_err(),
        "file does not exist"
    );
    audit(&mut t);
}

/// With room, the same directory splits, and the split is charged to its root.
#[test]
fn with_room_the_directory_splits_and_the_split_is_charged() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    let (a, _) = mint(&mut t, &mut k, &base, "a", 64 * 1024).unwrap();
    t.attach(&a, 0).unwrap();
    assert_eq!(crowd(&mut t, &a, 100), 100);
    assert!(pairs(&mut t, "a") >= 2, "split");
    let held = t.server.fs.ledger.roots().iter().find(|r| r.1 == "a").unwrap().3;
    assert_eq!(held, u64::from(pairs(&mut t, "a")) * 8192, "every pair charged");
    audit(&mut t);
}

/// A `mkdir` with no room for its pair is `no space` and changes nothing.
#[test]
fn a_mkdir_without_room_for_its_pair_changes_nothing() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    let (a, _) = mint(&mut t, &mut k, &base, "a", 8192 + 4096).unwrap();
    t.attach(&a, 0).unwrap();
    t.walk(&a, 0, 1, &[]).unwrap();
    assert_eq!(t.create(&a, 1, "sub", DMDIR | 0o755, mode::OREAD).unwrap_err(), "no space");
    t.clunk(&a, 1).unwrap();
    assert_eq!(t.walk(&a, 0, 1, &["sub"]).unwrap_err(), "file does not exist");
    assert_eq!(t.server.fs.out_of_room, 0, "refused before littlefs was asked");
    audit(&mut t);
}
