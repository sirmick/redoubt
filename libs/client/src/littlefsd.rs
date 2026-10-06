//! `littlefsd`'s typed operations on open files (servers/littlefsd.md, "Typed operations"). They name the
//! caller's fids, which live in [`File`]s here rather than in the caller, so they cannot be bound
//! through [`crate::typed`] alone. Every check is `littlefsd`'s, but one: two files in one operation must
//! be on one connection, since a fid means nothing on another.

use alloc::vec::Vec;

use redoubt_rt::client::Lend;
use redoubt_rt::wire::proto::littlefsd::{CopyFile, GetAttr, Message, Protocol, Rename, Reply, SetAttr};

use crate::error::{Error, Refusal};
use crate::file::File;
use crate::typed;

/// Renames `old_dir`'s entry `old_name` to `new_dir`'s `new_name`, atomically, within one volume.
pub fn rename(
    lend: &mut Lend,
    old_dir: &File,
    old_name: &str,
    new_dir: &File,
    new_name: &str,
) -> Result<(), Error> {
    one_connection(old_dir, new_dir)?;
    let rename = Rename { old_dir: old_dir.fid(), old_name, new_dir: new_dir.fid(), new_name };
    typed::call::<Protocol, _>(
        old_dir.connection().endpoint(),
        lend,
        &Message::Rename(rename),
        &[],
        |_, _| (),
    )
}

/// Copies `src` into `dst_dir` as `dst_name`; returns the bytes copied.
pub fn copy_file(lend: &mut Lend, src: &File, dst_dir: &File, dst_name: &str) -> Result<u64, Error> {
    one_connection(src, dst_dir)?;
    let copy = Message::CopyFile(CopyFile { src_fid: src.fid(), dst_dir: dst_dir.fid(), dst_name });
    typed::call::<Protocol, _>(src.connection().endpoint(), lend, &copy, &[], |reply, _| match reply {
        Reply::CopyFile(r) => Ok(r.count),
        _ => Err(Error::Unexpected),
    })?
}

/// Sets the user attribute `attr` of `file` to `value`.
pub fn set_attr(lend: &mut Lend, file: &File, attr: u8, value: &[u8]) -> Result<(), Error> {
    let set = Message::SetAttr(SetAttr { fid: file.fid(), attr, value });
    typed::call::<Protocol, _>(file.connection().endpoint(), lend, &set, &[], |_, _| ())
}

/// The user attribute `attr` of `file`.
pub fn get_attr(lend: &mut Lend, file: &File, attr: u8) -> Result<Vec<u8>, Error> {
    let get = Message::GetAttr(GetAttr { fid: file.fid(), attr });
    typed::call::<Protocol, _>(file.connection().endpoint(), lend, &get, &[], |reply, _| match reply {
        Reply::GetAttr(r) => Ok(r.value.to_vec()),
        _ => Err(Error::Unexpected),
    })?
}

fn one_connection(a: &File, b: &File) -> Result<(), Error> {
    if a.connection().same(b.connection()) { Ok(()) } else { Err(Refusal::OtherConnection.into()) }
}
