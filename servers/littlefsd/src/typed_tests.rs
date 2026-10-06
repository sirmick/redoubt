//! The typed operations against the files behind the skeleton, on fids a client made over 9P.

use alloc::vec::Vec;

use redoubt_rt::server::ninep::{DMDIR, mode};

use super::*;
use crate::server::tests::{Memory, SECTORS, T, caller, many, with_notes};

/// One typed request as `who`; the reply as owned values, or the error.
fn call(t: &mut T, who: &Caller, request: Message<'_>) -> Result<Option<Vec<u8>>, ErrorCode> {
    let mut typed = Typed(&mut t.server);
    let answer = typed.handle(who, request, &[])?;
    Ok(match answer.reply {
        Reply::CopyFile(r) => Some(r.count.to_le_bytes().to_vec()),
        Reply::GetAttr(r) => Some(r.value.to_vec()),
        Reply::Rename(_) | Reply::SetAttr(_) => None,
    })
}

fn rename(
    t: &mut T,
    who: &Caller,
    old_dir: u32,
    old_name: &str,
    new_dir: u32,
    new_name: &str,
) -> Result<(), ErrorCode> {
    call(t, who, Message::Rename(Rename { old_dir, old_name, new_dir, new_name })).map(|_| ())
}

fn copy(t: &mut T, who: &Caller, src_fid: u32, dst_dir: u32, dst_name: &str) -> Result<u64, ErrorCode> {
    let count = call(t, who, Message::CopyFile(CopyFile { src_fid, dst_dir, dst_name }))?.unwrap();
    Ok(u64::from_le_bytes(count.try_into().unwrap()))
}

fn set(t: &mut T, who: &Caller, fid: u32, attr: u8, value: &[u8]) -> Result<(), ErrorCode> {
    call(t, who, Message::SetAttr(SetAttr { fid, attr, value })).map(|_| ())
}

fn get(t: &mut T, who: &Caller, fid: u32, attr: u8) -> Result<Vec<u8>, ErrorCode> {
    call(t, who, Message::GetAttr(GetAttr { fid, attr })).map(Option::unwrap)
}

/// A volume holding `notes` and an empty directory `d`; fid 0 is the root.
fn volume(labels: &[u64], who: &Caller) -> T { volume_with(&Memory::blank(SECTORS), labels, who) }

/// [`volume`] on `disk`, unlabelled.
fn volume_on(disk: &Memory, who: &Caller) -> T { volume_with(disk, &[], who) }

fn volume_with(disk: &Memory, labels: &[u64], who: &Caller) -> T {
    let mut t = T::on(disk, labels);
    with_notes(&mut t, who, b"hello, world");
    t.walk(who, 0, 1, &[]).unwrap();
    t.create(who, 1, "d", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(who, 1).unwrap();
    t
}

#[test]
fn rename_moves_within_the_volume_and_keeps_the_files_id() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    let id = t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.walk(&who, 0, 2, &["d"]).unwrap();
    rename(&mut t, &who, 0, "notes", 2, "moved").unwrap();
    assert_eq!(t.walk(&who, 0, 3, &["notes"]).unwrap_err(), "file does not exist");
    assert_eq!(t.walk(&who, 0, 3, &["d", "moved"]).unwrap()[1], id[0], "the file, not a copy");
    t.open(&who, 3, mode::OREAD).unwrap();
    assert_eq!(t.read(&who, 3, 0, 100).unwrap(), b"hello, world");
    assert_eq!(rename(&mut t, &who, 0, "notes", 0, "x"), Err(ErrorCode::NotFound));
    assert_eq!(rename(&mut t, &who, 3, "a", 0, "b"), Err(ErrorCode::NotDir));
    assert_eq!(rename(&mut t, &who, 0, "d", 0, ".."), Err(ErrorCode::Refused));
}

#[test]
fn a_directory_is_not_renamed_into_itself() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &["d"]).unwrap();
    t.create(&who, 1, "inner", DMDIR | 0o755, mode::OREAD).unwrap();
    t.walk(&who, 0, 2, &["d"]).unwrap();
    assert_eq!(rename(&mut t, &who, 0, "d", 2, "self"), Err(ErrorCode::Refused));
    t.walk(&who, 0, 3, &["d", "inner"]).unwrap();
    assert_eq!(rename(&mut t, &who, 0, "d", 3, "deeper"), Err(ErrorCode::Refused));
    t.walk(&who, 0, 4, &["d", "inner"]).unwrap();
}

/// A rename over a file ends that file for every fid on it, as a remove does.
#[test]
fn a_rename_over_a_file_removes_it() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &[]).unwrap();
    t.create(&who, 1, "other", 0o644, mode::OWRITE).unwrap();
    rename(&mut t, &who, 0, "notes", 0, "other").unwrap();
    assert_eq!(t.write(&who, 1, 0, b"x").unwrap_err(), "removed");
}

#[test]
fn copy_file_copies_and_counts_the_bytes() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    let big: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
    t.walk(&who, 0, 1, &[]).unwrap();
    t.create(&who, 1, "big", 0o644, mode::OWRITE).unwrap();
    for (i, chunk) in big.chunks(4000).enumerate() {
        t.write(&who, 1, (i * 4000) as u64, chunk).unwrap();
    }
    t.walk(&who, 0, 2, &["d"]).unwrap();
    assert_eq!(copy(&mut t, &who, 1, 2, "copy"), Ok(20_000));
    let ids = t.walk(&who, 0, 3, &["d", "copy"]).unwrap();
    assert_ne!(ids[1], t.walk(&who, 0, 4, &["big"]).unwrap()[0], "a copy is a new file");
    t.open(&who, 3, mode::OREAD).unwrap();
    let mut back = Vec::new();
    while back.len() < big.len() {
        back.extend(t.read(&who, 3, back.len() as u64, 8000).unwrap());
    }
    assert!(back == big);
    assert_eq!(copy(&mut t, &who, 1, 2, "copy"), Err(ErrorCode::Exists));
    assert_eq!(copy(&mut t, &who, 2, 0, "dir"), Err(ErrorCode::Refused));
    assert_eq!(copy(&mut t, &who, 1, 1, "x"), Err(ErrorCode::NotDir));
}

#[test]
fn attributes_set_and_get_with_littlefsds_own_types_refused() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    set(&mut t, &who, 1, 16, b"user").unwrap();
    assert_eq!(get(&mut t, &who, 1, 16).unwrap(), b"user");
    set(&mut t, &who, 0, 255, b"on the root").unwrap();
    assert_eq!(get(&mut t, &who, 0, 255).unwrap(), b"on the root");
    for own in 0..OWN_ATTRS {
        assert_eq!(set(&mut t, &who, 1, own, b"forged"), Err(ErrorCode::Refused), "type {own}");
    }
    assert_eq!(set(&mut t, &who, 1, 17, &[0; ATTR_MAX + 1]), Err(ErrorCode::TooLarge));
    set(&mut t, &who, 1, 17, &[1; ATTR_MAX]).unwrap();
    assert_eq!(get(&mut t, &who, 1, 17).unwrap().len(), ATTR_MAX);
    assert_eq!(get(&mut t, &who, 1, 99), Err(ErrorCode::NotFound));
    // The file's own id is untouched by all of it.
    assert_eq!(t.walk(&who, 0, 2, &["notes"]).unwrap(), t.walk(&who, 0, 3, &["notes"]).unwrap());
}

/// A fid is the caller's: another connection's fid numbers name nothing here, so a stranger's
/// fid is `not_found`; one operation's fids from two connections are the same refusal.
#[test]
fn a_strangers_fid_is_not_found() {
    let (alice, bob) = (caller(1, &[]), caller(2, &[]));
    let mut t = volume(&[], &alice);
    t.walk(&alice, 0, 1, &["notes"]).unwrap();
    t.walk(&alice, 0, 2, &["d"]).unwrap();
    assert_eq!(get(&mut t, &bob, 1, 16), Err(ErrorCode::NotFound));
    assert_eq!(set(&mut t, &bob, 1, 16, b"x"), Err(ErrorCode::NotFound));
    assert_eq!(copy(&mut t, &bob, 1, 2, "stolen"), Err(ErrorCode::NotFound));
    assert_eq!(rename(&mut t, &bob, 0, "notes", 2, "stolen"), Err(ErrorCode::NotFound));
    // Bob's own root with Alice's directory fid: the second fid is resolved on Bob's
    // connection, where it does not exist.
    t.attach(&bob, 0).unwrap();
    assert_eq!(rename(&mut t, &bob, 0, "notes", 2, "moved"), Err(ErrorCode::NotFound));
    assert_eq!(copy(&mut t, &bob, 7, 0, "x"), Err(ErrorCode::NotFound));
    t.walk(&alice, 0, 3, &["notes"]).unwrap();
}

/// The typed operations make 9P's label checks against the volume's labels.
#[test]
fn typed_operations_check_the_volumes_labels() {
    let (owner, above) = (caller(1, &[7]), caller(2, &[7, 9]));
    let mut t = volume(&[7], &owner);
    t.attach(&above, 0).unwrap();
    t.walk(&above, 0, 1, &["notes"]).unwrap();
    assert_eq!(get(&mut t, &above, 1, 16), Err(ErrorCode::NotFound), "a read is allowed");
    assert_eq!(set(&mut t, &above, 1, 16, b"x"), Err(ErrorCode::Refused));
    assert_eq!(rename(&mut t, &above, 0, "notes", 0, "x"), Err(ErrorCode::Refused));
    assert_eq!(copy(&mut t, &above, 1, 0, "x"), Err(ErrorCode::Refused));
    t.walk(&owner, 0, 1, &["notes"]).unwrap();
    set(&mut t, &owner, 1, 16, b"ok").unwrap();
    assert_eq!(get(&mut t, &above, 1, 16).unwrap(), b"ok");
}

/// A removed file's fids get `removed` from the typed operations too; a broken volume
/// answers `corrupt`.
#[test]
fn removed_and_corrupt_are_the_tables_answers() {
    let who = caller(1, &[]);
    let disk = Memory::blank(SECTORS);
    let mut t = T::on(&disk, &[]);
    with_notes(&mut t, &who, b"x");
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.walk(&who, 0, 2, &["notes"]).unwrap();
    t.remove(&who, 2).unwrap();
    assert_eq!(get(&mut t, &who, 1, 16), Err(ErrorCode::Removed));
    assert_eq!(set(&mut t, &who, 1, 16, b"x"), Err(ErrorCode::Removed));
    assert_eq!(copy(&mut t, &who, 1, 0, "c"), Err(ErrorCode::Removed));
    disk.fail();
    assert_eq!(set(&mut t, &who, 0, 16, b"x"), Err(ErrorCode::Corrupt));
    assert_eq!(get(&mut t, &who, 0, 16), Err(ErrorCode::Corrupt));
}

/// A copy the volume has no room for is refused, and leaves no short file behind.
#[test]
fn a_copy_that_does_not_fit_leaves_nothing() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &[]).unwrap();
    t.create(&who, 1, "big", 0o644, mode::OWRITE).unwrap();
    let chunk = [7u8; 8000];
    for i in 0..19 {
        t.write(&who, 1, i * 8000, &chunk).unwrap();
    }
    assert_eq!(copy(&mut t, &who, 1, 0, "copy"), Err(ErrorCode::NoSpace));
    assert_eq!(t.walk(&who, 0, 2, &["copy"]).unwrap_err(), "file does not exist");
}

/// On a read-only range the typed operations that change anything are refused, and refusing
/// them poisons nothing: a read still answers.
#[test]
fn a_read_only_volume_refuses_every_typed_change() {
    let who = caller(1, &[]);
    let disk = Memory::blank(SECTORS);
    {
        let mut t = volume_on(&disk, &who);
        t.walk(&who, 0, 1, &["notes"]).unwrap();
        set(&mut t, &who, 1, 16, b"before").unwrap();
    }
    let disk = disk.read_only();
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.walk(&who, 0, 2, &["d"]).unwrap();
    assert_eq!(set(&mut t, &who, 1, 16, b"after"), Err(ErrorCode::Refused));
    assert_eq!(rename(&mut t, &who, 0, "notes", 2, "moved"), Err(ErrorCode::Refused));
    assert_eq!(copy(&mut t, &who, 1, 2, "copy"), Err(ErrorCode::Refused));
    assert_eq!(get(&mut t, &who, 1, 16).unwrap(), b"before");
}

/// Fids do not follow renames: a fid on a renamed file, and one on a file below a renamed
/// directory, get `removed`; the file itself is reached again at its new path.
#[test]
fn fids_on_a_renamed_file_or_below_a_renamed_directory_are_removed() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &["d"]).unwrap();
    t.create(&who, 1, "inside", 0o644, mode::OWRITE).unwrap();
    t.walk(&who, 0, 2, &["notes"]).unwrap();
    t.walk(&who, 0, 3, &["d"]).unwrap();
    rename(&mut t, &who, 0, "notes", 0, "renamed").unwrap();
    rename(&mut t, &who, 0, "d", 0, "e").unwrap();
    assert_eq!(t.stat(&who, 2).unwrap_err(), "removed");
    assert_eq!(t.write(&who, 1, 0, b"x").unwrap_err(), "removed");
    assert_eq!(t.stat(&who, 3).unwrap_err(), "removed");
    assert_eq!(get(&mut t, &who, 2, 16), Err(ErrorCode::Removed));
    t.walk(&who, 0, 4, &["e", "inside"]).unwrap();
    t.walk(&who, 0, 5, &["renamed"]).unwrap();
}

/// Renames can build a tree deeper than any walk from one root reaches (300 here, the skeleton's
/// walks stopping at 64 below a fid's root): it is the writers' own tree, it works live, and
/// after a remount it is served, not refused as corrupt.
#[test]
fn a_tree_renames_made_deep_still_mounts() {
    let who = caller(1, &[]);
    let disk = Memory::blank(1024 * 8);
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &[]).unwrap();
    t.create(&who, 1, "chain", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(&who, 1).unwrap();
    for _ in 0..300 {
        t.walk(&who, 0, 1, &[]).unwrap();
        t.create(&who, 1, "next", DMDIR | 0o755, mode::OREAD).unwrap();
        t.clunk(&who, 1).unwrap();
        t.walk(&who, 0, 2, &["next"]).unwrap();
        rename(&mut t, &who, 0, "chain", 2, "chain").unwrap();
        t.clunk(&who, 2).unwrap();
        rename(&mut t, &who, 0, "next", 0, "chain").unwrap();
    }
    let mut t = T::on(&disk, &[]);
    t.attach(&who, 0).unwrap();
    assert_eq!(t.walk(&who, 0, 1, &["chain", "chain", "chain"]).map(|q| q.len()), Ok(3));
}

/// A create, a remove, a rename and a write between two reads of one listing: the second read
/// goes on from the entry where the first stopped, in the directory as it is now, with the
/// stats it has now. A directory removed between two reads is `removed`.
#[test]
fn a_change_between_reads_refills_the_window() {
    let who = caller(1, &[]);
    let mut t = volume_on(&Memory::blank(SECTORS * 4), &who);
    many(&mut t, &who, "e", 100, |i| alloc::format!("{i:03}"));
    t.walk(&who, 0, 2, &["e"]).unwrap();
    t.open(&who, 2, mode::OREAD).unwrap();
    let (first, offset) = t.entries(&who, 2, 0, 1000).unwrap();
    assert!(!first.is_empty() && first.len() < 64);
    t.walk(&who, 0, 3, &["e"]).unwrap();
    t.create(&who, 3, "new", 0o644, mode::OWRITE).unwrap();
    t.clunk(&who, 3).unwrap();
    t.walk(&who, 0, 3, &["e", "050"]).unwrap();
    t.remove(&who, 3).unwrap();
    t.walk(&who, 0, 3, &["e"]).unwrap();
    rename(&mut t, &who, 3, "005", 3, "renamed").unwrap();
    t.walk(&who, 0, 4, &["e", "070"]).unwrap();
    t.open(&who, 4, mode::OWRITE).unwrap();
    t.write(&who, 4, 0, b"longer now").unwrap();
    t.clunk(&who, 4).unwrap();
    let mut rest = Vec::new();
    let mut at = offset;
    loop {
        let (entries, n) = t.entries(&who, 2, at, 4096).unwrap();
        if n == 0 {
            break;
        }
        at += n;
        rest.extend(entries);
    }
    t.walk(&who, 0, 4, &["e"]).unwrap();
    t.open(&who, 4, mode::OREAD).unwrap();
    let now = t.listing(&who, 4).unwrap();
    assert_eq!(rest, now[first.len()..]);
    assert!(rest.iter().any(|e| e.0 == "070" && e.5 == 10));

    // A directory removed between two reads of its listing.
    t.walk(&who, 0, 5, &[]).unwrap();
    t.create(&who, 5, "gone", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(&who, 5).unwrap();
    for name in ["a", "b"] {
        t.walk(&who, 0, 6, &["gone"]).unwrap();
        t.create(&who, 6, name, 0o644, mode::OREAD).unwrap();
        t.clunk(&who, 6).unwrap();
    }
    t.walk(&who, 0, 5, &["gone"]).unwrap();
    t.open(&who, 5, mode::OREAD).unwrap();
    let (first, offset) = t.entries(&who, 5, 0, 60).unwrap();
    assert_eq!(first.len(), 1);
    for path in [&["gone", "a"][..], &["gone", "b"], &["gone"]] {
        t.walk(&who, 0, 6, path).unwrap();
        t.remove(&who, 6).unwrap();
    }
    assert_eq!(t.entries(&who, 5, offset, 4096).unwrap_err(), "removed");
}
