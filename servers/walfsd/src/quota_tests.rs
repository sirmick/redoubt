//! Quotas per attach root (servers/walfsd.md, "Quotas"; R48), over the skeleton: connections are
//! minted with `new_connection` through `answer_common`, and every change is made over 9P.

use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_rt::abi::{Error, Handle, Handles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{DMDIR, FileServer, Minter, mode, ninep_common};
use redoubt_rt::wire::proto::littlefsd::ErrorCode;

use crate::server::file_bytes;
use crate::server::tests::{Memory, SECTORS, T, caller};
use crate::typed::tests::{copy, rename};

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

/// A volume of `sectors`, the founding caller, and a kernel to mint with.
fn volume_of(sectors: usize) -> (T, Caller, Kernel) {
    let t = T::on(&Memory::blank(sectors), &[]);
    (t, caller(1, &[]), Kernel { minted: Vec::new(), rng: 0x9e37_79b9_7f4a_7c15 })
}

fn volume() -> (T, Caller, Kernel) { volume_of(SECTORS) }

/// The ledger agrees with a fresh count of every live root, each within its quota.
fn audit(t: &mut T) { t.server.fs.audit(false) }

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

/// The quotas of the bench's `walfsd-quota`.
const QUOTA: u64 = 256 * 1024;

/// R48's attack: Bob fills his root to its quota and is refused, and Alice, on another root of
/// the same volume, still writes, as does the volume's root. The refusal is walfsd's, by the
/// quota it recorded at the mint, not anything Bob reports.
#[test]
fn a_write_past_one_roots_quota_is_refused_while_another_still_writes() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["alice", "bob"]);
    let (alice, _) = mint(&mut t, &mut k, &base, "alice", QUOTA).unwrap();
    let (bob, _) = mint(&mut t, &mut k, &base, "bob", QUOTA).unwrap();
    t.attach(&bob, 0).unwrap();
    file(&mut t, &bob, 1, "hog").unwrap();
    let filled = fill(&mut t, &bob, 1);
    let share = t.server.fs.share();
    assert!(filled + share <= QUOTA && filled + share + 3 * 4096 > QUOTA, "Bob wrote {filled}");
    assert_eq!(t.stat(&bob, 1).unwrap().1, filled, "the refused write changed nothing");
    t.attach(&alice, 0).unwrap();
    file(&mut t, &alice, 1, "save").unwrap();
    assert_eq!(t.write(&alice, 1, 0, &[1u8; 32 * 1024]), Ok(32 * 1024));
    file(&mut t, &base, 2, "log").unwrap();
    assert_eq!(t.write(&base, 2, 0, &[2u8; 32 * 1024]), Ok(32 * 1024));
    audit(&mut t);
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
    assert_eq!(t.write(&z, 1, 4096, b"more").unwrap_err(), "no space");
    t.remove(&z, 1).unwrap();
    t.server.fs.audit(true);
}

/// No promise the disk cannot keep, in blocks or in inodes: roots carving the whole volume
/// between them, each filling its quota with empty files and with data, never meet walfs's own
/// `no space`, and every one is refused by its quota first.
#[test]
fn the_volume_never_runs_out_while_every_root_is_within_its_quota() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "b", "c"]);
    let (_, _, room, held, reserve) = t.server.fs.ledger.roots()[0].clone();
    let each = (room - held - reserve) / 3;
    let roots: Vec<Caller> =
        ["a", "b", "c"].iter().map(|r| mint(&mut t, &mut k, &base, r, each).unwrap().0).collect();
    // `a` makes empty files, `b` one large file, `c` many small ones.
    t.attach(&roots[0], 0).unwrap();
    let mut made = 0;
    while file(&mut t, &roots[0], 1, &std::format!("e{made}")).is_ok() {
        t.clunk(&roots[0], 1).unwrap();
        made += 1;
    }
    t.attach(&roots[1], 0).unwrap();
    file(&mut t, &roots[1], 1, "big").unwrap();
    fill(&mut t, &roots[1], 1);
    t.attach(&roots[2], 0).unwrap();
    let mut small = 0;
    while file(&mut t, &roots[2], 1, &std::format!("s{small}")).is_ok() {
        let wrote = t.write(&roots[2], 1, 0, &[3u8; 5000]);
        t.clunk(&roots[2], 1).unwrap();
        if wrote.is_err() {
            break;
        }
        small += 1;
    }
    assert!(made > 0 && small > 0);
    audit(&mut t);
    // Every entry had its inode: none was refused by walfs.
    assert!(made + small <= fs_inodes(&mut t));
}

/// The inodes an entry may take: all but inode 0 and the root.
fn fs_inodes(t: &mut T) -> usize {
    let mut n = 0;
    t.server.fs.with(|fs| Ok(n = fs.inode_count() as usize - 2)).unwrap();
    n
}

/// A mint the parent's room cannot take is refused, and a disconnect gives the quota back.
#[test]
fn a_mint_the_room_cannot_take_is_refused_and_disconnect_gives_it_back() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "b"]);
    let (_, _, room, held, reserve) = t.server.fs.ledger.roots()[0].clone();
    let all = room - held - reserve;
    let (_, id) = mint(&mut t, &mut k, &base, "a", all).unwrap();
    assert_eq!(mint(&mut t, &mut k, &base, "b", 4096).unwrap_err(), REFUSED);
    disconnect(&mut t, &mut k, &base, id);
    mint(&mut t, &mut k, &base, "b", 4096).unwrap();
    audit(&mut t);
}

/// A root minted over files counts them: their shares and blocks.
#[test]
fn a_root_minted_over_files_counts_them() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.create(&base, 1, "f", 0o644, mode::OWRITE).unwrap();
    t.write(&base, 1, 0, &[1u8; 20_000]).unwrap();
    mint(&mut t, &mut k, &base, "a", QUOTA).unwrap();
    let a = t.server.fs.ledger.roots()[1].clone();
    let share = t.server.fs.share();
    assert_eq!(a.3, 4096 + share + file_bytes(20_000), "the directory's block, and the file");
    audit(&mut t);
}

/// A rename never takes a live root away, and one between two roots moves the bytes.
#[test]
fn a_rename_between_two_roots_moves_the_bytes_and_never_ends_a_live_root() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a", "b"]);
    t.put(&base, "f", &[1u8; 9000]);
    mint(&mut t, &mut k, &base, "a", QUOTA).unwrap();
    mint(&mut t, &mut k, &base, "b", QUOTA).unwrap();
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.walk(&base, 0, 2, &["b"]).unwrap();
    rename(&mut t, &base, 0, "f", 1, "f").unwrap();
    audit(&mut t);
    rename(&mut t, &base, 1, "f", 2, "g").unwrap();
    audit(&mut t);
    assert_eq!(rename(&mut t, &base, 0, "a", 2, "a"), Err(ErrorCode::NotPermitted), "a live root moved");
    t.walk(&base, 0, 3, &["b"]).unwrap();
    assert_eq!(t.remove(&base, 3).unwrap_err(), "permission denied", "a live root removed");
}

/// A copy is charged in full, and one past the quota is `no_space`, leaving nothing.
#[test]
fn a_copy_past_the_quota_is_no_space() {
    let (mut t, base, mut k) = volume();
    dirs(&mut t, &base, &["a"]);
    t.put(&base, "big", &vec![1u8; QUOTA as usize]);
    let (a, _) = mint(&mut t, &mut k, &base, "a", QUOTA).unwrap();
    t.attach(&a, 0).unwrap();
    t.walk(&base, 0, 1, &["big"]).unwrap();
    t.walk(&base, 0, 2, &["a"]).unwrap();
    assert_eq!(copy(&mut t, &base, 1, 2, "copy"), Err(ErrorCode::NoSpace));
    assert_eq!(t.walk(&a, 0, 3, &["copy"]).unwrap_err(), "file does not exist");
    audit(&mut t);
}

/// A mint at a node that is no longer its directory (removed and made again under the same name)
/// is `removed`, and records nothing: once the new directory is a live root, a stale mint there
/// still adds no second root, so its subtree is never taken from the volume root twice.
#[test]
fn a_mint_at_a_stale_root_records_nothing() {
    let (mut t, base, _) = volume();
    dirs(&mut t, &base, &["a"]);
    let stale = t.server.fs.node_at("a".into()).unwrap();
    t.walk(&base, 0, 1, &["a"]).unwrap();
    t.remove(&base, 1).unwrap();
    t.walk(&base, 0, 1, &[]).unwrap();
    t.create(&base, 1, "a", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(&base, 1).unwrap();
    let minted = t.server.fs.minted(&base, 77, 0, &stale, QUOTA);
    assert_eq!(minted, Err(crate::server::text::REMOVED));
    assert_eq!(t.server.fs.ledger.roots().len(), 1, "only the volume root is live");
    let fresh = t.server.fs.node_at("a".into()).unwrap();
    assert_eq!(t.server.fs.minted(&base, 78, 0, &fresh, QUOTA), Ok(()));
    assert_eq!(t.server.fs.minted(&base, 79, 0, &stale, QUOTA), Err(crate::server::text::REMOVED));
    assert_eq!(t.server.fs.ledger.roots().len(), 2, "one root at a, beside the volume's");
    audit(&mut t);
}

#[test]
fn file_bytes_counts_data_and_indirect_blocks() {
    let b = 4096;
    assert_eq!(file_bytes(0), 0);
    assert_eq!(file_bytes(1), b);
    assert_eq!(file_bytes(12 * b), 12 * b);
    assert_eq!(file_bytes(12 * b + 1), 14 * b, "a single-indirect block");
    assert_eq!(file_bytes((12 + 1024) * b), (12 + 1024 + 1) * b);
    assert_eq!(file_bytes((12 + 1024) * b + 1), (12 + 1024 + 1 + 1 + 1 + 1) * b, "a double and its first");
    assert_eq!(file_bytes(walfs::MAX_FILE_SIZE), (1_049_612 + 1 + 1 + 1024) * b);
}
