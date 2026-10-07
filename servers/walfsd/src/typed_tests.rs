//! The typed operations against the files behind the skeleton, on fids a client made over 9P.

use alloc::vec::Vec;

use redoubt_rt::server::ninep::{DMDIR, mode};

use super::*;
use crate::server::tests::{Memory, SECTORS, T, caller, with_notes};

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

pub(crate) fn rename(
    t: &mut T,
    who: &Caller,
    old_dir: u32,
    old_name: &str,
    new_dir: u32,
    new_name: &str,
) -> Result<(), ErrorCode> {
    call(t, who, Message::Rename(Rename { old_dir, old_name, new_dir, new_name })).map(|_| ())
}

pub(crate) fn copy(
    t: &mut T,
    who: &Caller,
    src_fid: u32,
    dst_dir: u32,
    dst_name: &str,
) -> Result<u64, ErrorCode> {
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
fn volume(labels: &[u64], who: &Caller) -> T {
    let mut t = T::on(&Memory::blank(SECTORS), labels);
    with_notes(&mut t, who, b"hello, world");
    t.walk(who, 0, 1, &[]).unwrap();
    t.create(who, 1, "d", DMDIR | 0o755, mode::OREAD).unwrap();
    t.clunk(who, 1).unwrap();
    t
}

#[test]
fn rename_moves_within_the_volume_and_keeps_the_inode() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    let ino = t.walk(&who, 0, 1, &["notes"]).unwrap();
    t.walk(&who, 0, 2, &["d"]).unwrap();
    rename(&mut t, &who, 0, "notes", 2, "moved").unwrap();
    assert_eq!(t.walk(&who, 0, 3, &["notes"]).unwrap_err(), "file does not exist");
    assert_eq!(t.walk(&who, 0, 3, &["d", "moved"]).unwrap()[1], ino[0], "the file, not a copy");
    assert_eq!(t.get(&who, &["d", "moved"]).unwrap(), b"hello, world");
    // A fid does not follow a rename: it rests on the path it was walked by.
    assert_eq!(t.stat(&who, 1).unwrap_err(), "removed");
    assert_eq!(rename(&mut t, &who, 0, "notes", 0, "x"), Err(ErrorCode::NotFound));
    assert_eq!(rename(&mut t, &who, 3, "a", 0, "b"), Err(ErrorCode::NotDir));
    assert_eq!(rename(&mut t, &who, 0, "d", 0, ".."), Err(ErrorCode::BadName));
    // Into itself, refused by walfs.
    assert_eq!(rename(&mut t, &who, 0, "d", 2, "self"), Err(ErrorCode::NotPermitted));
}

/// A rename over a file ends that file for every fid on it, as a remove does, in one
/// transaction.
#[test]
fn a_rename_over_a_file_removes_it() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.put(&who, "other", b"replaced");
    t.walk(&who, 0, 1, &["other"]).unwrap();
    rename(&mut t, &who, 0, "notes", 0, "other").unwrap();
    assert_eq!(t.stat(&who, 1).unwrap_err(), "removed");
    assert_eq!(t.get(&who, &["other"]).unwrap(), b"hello, world");
}

#[test]
fn copy_file_copies_and_counts_the_bytes() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    let big: Vec<u8> = (0..20_000u32).map(|i| i as u8).collect();
    t.put(&who, "big", &big);
    t.walk(&who, 0, 1, &["big"]).unwrap();
    t.walk(&who, 0, 2, &["d"]).unwrap();
    assert_eq!(copy(&mut t, &who, 1, 2, "copy"), Ok(20_000));
    t.walk(&who, 0, 3, &["d", "copy"]).unwrap();
    t.open(&who, 3, mode::OREAD).unwrap();
    let mut got = Vec::new();
    while got.len() < big.len() {
        got.extend(t.read(&who, 3, got.len() as u64, 8192).unwrap());
    }
    assert_eq!(got, big);
    assert_eq!(copy(&mut t, &who, 1, 2, "copy"), Err(ErrorCode::Exists));
    assert_eq!(copy(&mut t, &who, 2, 0, "dir"), Err(ErrorCode::NotSupported), "a directory is not copied");
    t.server.fs.audit(false);
}

/// Attributes live in the inode's area: the user's types 16 to 255, up to walfs's 254 bytes each;
/// types 0 to 15 are refused as on `littlefsd`, and a value past the area is `no_space`.
#[test]
fn attributes_set_and_get_with_the_reserved_types_refused() {
    let who = caller(1, &[]);
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    assert_eq!(get(&mut t, &who, 1, 16), Err(ErrorCode::NotFound));
    set(&mut t, &who, 1, 16, b"tag").unwrap();
    assert_eq!(get(&mut t, &who, 1, 16).unwrap(), b"tag");
    set(&mut t, &who, 1, 16, b"other").unwrap();
    assert_eq!(get(&mut t, &who, 1, 16).unwrap(), b"other");
    for own in [0, 2, 15] {
        assert_eq!(set(&mut t, &who, 1, own, b"x"), Err(ErrorCode::NotPermitted));
    }
    assert_eq!(get(&mut t, &who, 1, 0), Err(ErrorCode::NotFound));
    assert_eq!(set(&mut t, &who, 1, 17, &[1; 255]), Err(ErrorCode::TooLarge));
    set(&mut t, &who, 1, 17, &[1; 200]).unwrap();
    assert_eq!(set(&mut t, &who, 1, 18, &[1; 100]), Err(ErrorCode::NoSpace), "the area is 256 bytes");
    // A directory has its area too.
    t.walk(&who, 0, 2, &["d"]).unwrap();
    set(&mut t, &who, 2, 200, b"dir").unwrap();
    assert_eq!(get(&mut t, &who, 2, 200).unwrap(), b"dir");
}

#[test]
fn a_strangers_fid_is_not_found() {
    let (who, stranger) = (caller(1, &[]), caller(2, &[]));
    let mut t = volume(&[], &who);
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    assert_eq!(get(&mut t, &stranger, 1, 16), Err(ErrorCode::NotFound));
    assert_eq!(rename(&mut t, &stranger, 0, "notes", 0, "mine"), Err(ErrorCode::NotFound));
}

/// The volume's labels are checked as 9P checks them: a reader above them cannot change
/// anything, and one without them reaches nothing.
#[test]
fn typed_operations_check_the_volumes_labels() {
    let (owner, above) = (caller(1, &[7]), caller(2, &[7, 9]));
    let mut t = volume(&[7], &owner);
    t.attach(&above, 0).unwrap();
    t.walk(&above, 0, 1, &["notes"]).unwrap();
    assert_eq!(get(&mut t, &above, 1, 16), Err(ErrorCode::NotFound));
    assert_eq!(set(&mut t, &above, 1, 16, b"x"), Err(ErrorCode::NotPermitted));
    assert_eq!(rename(&mut t, &above, 0, "notes", 0, "x"), Err(ErrorCode::NotPermitted));
    assert_eq!(copy(&mut t, &above, 1, 0, "x"), Err(ErrorCode::NotPermitted));
}

#[test]
fn a_read_only_volume_refuses_every_typed_change() {
    let who = caller(1, &[]);
    let disk = Memory::blank(SECTORS);
    {
        let mut t = T::on(&disk, &[]);
        with_notes(&mut t, &who, b"kept");
    }
    let before = disk.0.borrow().writes;
    let mut t = T::on(&disk.clone().read_only(), &[]);
    t.attach(&who, 0).unwrap();
    t.walk(&who, 0, 1, &["notes"]).unwrap();
    assert_eq!(rename(&mut t, &who, 0, "notes", 0, "x"), Err(ErrorCode::ReadOnly));
    assert_eq!(copy(&mut t, &who, 1, 0, "x"), Err(ErrorCode::ReadOnly));
    assert_eq!(set(&mut t, &who, 1, 16, b"x"), Err(ErrorCode::ReadOnly));
    assert_eq!(disk.0.borrow().writes, before);
}
