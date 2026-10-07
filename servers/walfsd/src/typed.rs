//! `walfsd`'s typed operations: `littlefsd`'s protocol, served the same, so a client names no
//! format (servers/walfsd.md, "Serving"; the table is libs/wire/tables/littlefsd.md). They name the
//! caller's fids, which live in the skeleton, so they are served with the whole [`NineServer`] in
//! hand: [`NineServer::fid_node`] finds a fid exactly as a 9P request does, and a fid of any other
//! connection is `not_found`.

use redoubt_rt::abi::Handle;
use redoubt_rt::ipc::{Caller, Words};
use redoubt_rt::path;
use redoubt_rt::server::ninep::{NineServer, QTDIR};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer};
use redoubt_rt::server::{Access, check};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::littlefsd::{
    CopyFile, CopyFileReply, ErrorCode, GetAttr, GetAttrReply, Message, Rename, RenameReply, Reply, SetAttr,
    SetAttrReply,
};
use walfs::{Error as FsError, OpenOptions};

use crate::server::{Failure, Node, OWN_ATTRS, Walfsd, code, file_bytes};
use crate::volume::{BLOCK, Range};

/// How much `copy_file` moves at a time: one block, so each write is one transaction.
const CHUNK: usize = walfs::BLOCK;

/// The `littlefsd` protocol, named once for [`redoubt_rt::server::typed`].
pub struct Walfsds;

impl Protocol for Walfsds {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// The server a typed call is answered by: the skeleton, for the caller's fids, and the files.
pub struct Typed<'a, R: Range>(pub &'a mut NineServer<Walfsd<R>>);

impl<R: Range> Typed<'_, R> {
    /// The node `fid` rests on, the caller's own, after the label check for `access`: the same
    /// check 9P makes, against the volume's labels.
    fn node(&self, caller: &Caller, fid: u32, access: Access) -> Result<(Node, bool), ErrorCode> {
        let (node, qid) = self.0.fid_node(caller, fid).map_err(|_| ErrorCode::NotFound)?;
        check(caller.labels.as_slice(), self.0.fs.volume_labels(), access)
            .map_err(|_| ErrorCode::NotPermitted)?;
        Ok((node, qid.kind & QTDIR != 0))
    }

    /// The node of a directory fid.
    fn dir(&self, caller: &Caller, fid: u32) -> Result<Node, ErrorCode> {
        let (node, dir) = self.node(caller, fid, Access::Write)?;
        if dir { Ok(node) } else { Err(ErrorCode::NotDir) }
    }

    /// One transaction (servers/walfsd.md, "Atomicity"), over an existing file or empty directory
    /// too.
    fn rename(&mut self, caller: &Caller, r: &Rename<'_>) -> Result<(), ErrorCode> {
        let (old_dir, new_dir) = (self.dir(caller, r.old_dir)?, self.dir(caller, r.new_dir)?);
        if !path::valid_name(r.old_name) || !path::valid_name(r.new_name) {
            return Err(ErrorCode::BadName);
        }
        let fs = &mut self.0.fs;
        fs.writable().map_err(code)?;
        let from = fs.found_child(&old_dir, r.old_name).map_err(code)?;
        let to = fs.found_child(&new_dir, r.new_name).map_err(code)?;
        // A rename onto itself changes nothing, if there is anything to rename.
        if from == to {
            return fs.with(|fs| fs.stat(&from)).map(|_| ()).map_err(code);
        }
        // Moving a live root or a directory holding one, or renaming over a live root's
        // directory, would end its connections (servers/walfsd.md, "Quotas").
        if fs.ledger.holds_live(&from) || fs.ledger.holds_live(&to) {
            return Err(ErrorCode::NotPermitted);
        }
        // What moves leaves the root holding `old_dir` for the one holding `new_dir`, which
        // also gets back what the rename replaces, and may gain a directory block.
        let moved = fs.holds(&from).map_err(code)?;
        let replaced = match fs.holds(&to) {
            Ok(held) => held,
            Err(Failure::Fs(FsError::NoEntry)) => 0,
            Err(e) => return Err(code(e)),
        };
        let src = fs.ledger.holder(old_dir.path());
        let crossing =
            if src == fs.ledger.holder(new_dir.path()) { 0 } else { moved.saturating_sub(replaced) };
        let dst = fs.room(new_dir.path(), crossing + u64::from(BLOCK)).map_err(code)?;
        fs.changing(dst, new_dir.path(), |fs| {
            // walfs refuses a directory moved into itself (`Invalid`, so `not_permitted`).
            fs.with(|fs| fs.rename(&from, &to))?;
            fs.ledger.change(src, 0, moved);
            fs.ledger.change(dst, moved, replaced);
            Ok(())
        })
        .map_err(code)
    }

    /// A new file, charged its share and its blocks in full; a copy that does not finish is
    /// removed, not left short.
    fn copy_file(&mut self, caller: &Caller, c: &CopyFile<'_>) -> Result<u64, ErrorCode> {
        let (src, src_dir) = self.node(caller, c.src_fid, Access::Read)?;
        let dst_dir = self.dir(caller, c.dst_dir)?;
        // A directory is not copied: the server does no tree copy.
        if src_dir {
            return Err(ErrorCode::NotSupported);
        }
        if !path::valid_name(c.dst_name) {
            return Err(ErrorCode::BadName);
        }
        let fs = &mut self.0.fs;
        fs.writable().map_err(code)?;
        let size = fs.find(&src).map_err(code)?.size;
        let to = fs.found_child(&dst_dir, c.dst_name).map_err(code)?;
        let need = fs.share() + file_bytes(size) + u64::from(BLOCK);
        let root = fs.room(dst_dir.path(), need).map_err(code)?;
        let copy = |walfsd: &mut Walfsd<R>| {
            walfsd.create_file(&to)?;
            walfsd.ledger.change(root, walfsd.share(), 0);
            let copied = walfsd.with(|fs| {
                let from = fs.open(src.path(), OpenOptions { read: true, ..OpenOptions::default() })?;
                let into = match fs.open(&to, OpenOptions { write: true, ..OpenOptions::default() }) {
                    Ok(into) => into,
                    Err(e) => {
                        let _ = fs.close(from);
                        return Err(e);
                    }
                };
                let mut chunk = [0u8; CHUNK];
                let mut total = 0u64;
                let moved = loop {
                    match fs.read(from, &mut chunk) {
                        Ok(0) => break Ok(total),
                        Ok(n) => match fs.write(into, &chunk[..n]) {
                            Ok(w) if w == n => total += n as u64,
                            Ok(_) => break Err(FsError::NoSpace),
                            Err(e) => break Err(e),
                        },
                        Err(e) => break Err(e),
                    }
                };
                let _ = fs.close(from);
                let closed = fs.close(into);
                moved.and_then(|total| closed.map(|()| total))
            });
            // What the copy left is charged, counted afresh: the whole file, or nothing once a
            // failed copy is removed.
            let left = walfsd.with(|fs| fs.stat(&to)).map(|m| m.size).unwrap_or(0);
            if copied.is_err() && walfsd.with(|fs| fs.remove(&to)).is_ok() {
                walfsd.ledger.change(root, 0, walfsd.share());
            } else {
                walfsd.ledger.change(root, file_bytes(left), 0);
            }
            copied
        };
        fs.changing(root, dst_dir.path(), copy).map_err(code)
    }

    /// The inode's attribute area, one transaction of its own (servers/walfsd.md, "User
    /// attributes"); no quota, since the area is the inode's.
    fn set_attr(&mut self, caller: &Caller, s: &SetAttr<'_>) -> Result<(), ErrorCode> {
        let (node, _) = self.node(caller, s.fid, Access::Write)?;
        // The attributes below `OWN_ATTRS` are the server's own.
        if s.attr < OWN_ATTRS {
            return Err(ErrorCode::NotPermitted);
        }
        if s.value.len() > walfs::ATTR_MAX {
            return Err(ErrorCode::TooLarge);
        }
        let fs = &mut self.0.fs;
        fs.writable().map_err(code)?;
        fs.find(&node).map_err(code)?;
        fs.changed();
        fs.with(|fs| fs.set_attr(node.path(), s.attr, s.value)).map_err(code)
    }

    fn get_attr(&mut self, caller: &Caller, g: &GetAttr) -> Result<&[u8], ErrorCode> {
        let (node, _) = self.node(caller, g.fid, Access::Read)?;
        let fs = &mut self.0.fs;
        fs.find(&node).map_err(code)?;
        // Type 0 ends an area, and is no attribute.
        if g.attr == 0 {
            return Err(ErrorCode::NotFound);
        }
        let value = fs.with(|fs| fs.get_attr(node.path(), g.attr)).map_err(code)?;
        Ok(fs.keep_attr(value))
    }
}

impl<R: Range> TypedServer<Walfsds> for Typed<'_, R> {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        _handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let reply = match request {
            Message::Rename(r) => self.rename(caller, &r).map(|()| Reply::Rename(RenameReply {}))?,
            Message::CopyFile(c) => {
                self.copy_file(caller, &c).map(|count| Reply::CopyFile(CopyFileReply { count }))?
            }
            Message::SetAttr(s) => self.set_attr(caller, &s).map(|()| Reply::SetAttr(SetAttrReply {}))?,
            Message::GetAttr(g) => Reply::GetAttr(GetAttrReply { value: self.get_attr(caller, &g)? }),
        };
        Ok(Answer::new(reply))
    }
}

#[cfg(test)]
#[path = "typed_tests.rs"]
pub(crate) mod tests;
