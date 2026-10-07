//! `/dev/cons`: reads, writes, and the `consol` protocol's `size` and `resize`
//! (servers/consoled.md, "The `consol` protocol"), which every console server serves on the same
//! endpoint. A read with nothing to read waits in the server until a key arrives; nothing is
//! buffered here.

use redoubt_rt::client::Lend;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::wire::proto::consol::{self, Message, Reply, Resize, Size};
use redoubt_rt::wire::typed::MALFORMED;

use crate::error::{Error, Refusal};
use crate::file::File;
use crate::ns::Namespace;
use crate::typed;

/// The console, open for reading and writing.
pub struct Console {
    file: File,
}

impl Console {
    /// `/dev/cons` in `ns`.
    pub fn open(ns: &Namespace, lend: &mut Lend) -> Result<Console, Error> {
        let (conn, rest) = ns.lookup("/dev/cons").ok_or(Refusal::BadPath)?;
        Ok(Console { file: conn.open(lend, rest, mode::ORDWR)? })
    }

    /// What was typed, at most `out.len()` bytes: waits until there is some.
    pub fn read(&self, lend: &mut Lend, out: &mut [u8]) -> Result<usize, Error> {
        self.file.read_at(lend, 0, out)
    }

    pub fn write(&self, lend: &mut Lend, data: &[u8]) -> Result<usize, Error> {
        self.file.write_at(lend, 0, data)
    }

    /// The console's columns and rows, asked afresh: `None` from a server that does not serve
    /// `consol` (it refuses the call as malformed).
    pub fn size(&self, lend: &mut Lend) -> Result<Option<(u16, u16)>, Error> {
        match self.ask(lend, Message::Size(Size {})) {
            Err(Error::Server(MALFORMED)) => Ok(None),
            size => size.map(Some),
        }
    }

    /// The size when it next changes: waits, in the server, until it does.
    pub fn resize(&self, lend: &mut Lend) -> Result<(u16, u16), Error> {
        self.ask(lend, Message::Resize(Resize {}))
    }

    pub fn close(self, lend: &mut Lend) -> Result<(), Error> { self.file.close(lend) }

    /// The open file: its fid, for requests on the console's connection through a hub.
    pub fn file(&self) -> &File { &self.file }

    fn ask(&self, lend: &mut Lend, message: Message) -> Result<(u16, u16), Error> {
        let endpoint = self.file.connection().endpoint();
        typed::call::<consol::Protocol, _>(endpoint, lend, &message, &[], |reply, _| match reply {
            Reply::Size(r) => (r.cols, r.rows),
            Reply::Resize(r) => (r.cols, r.rows),
        })
    }
}
