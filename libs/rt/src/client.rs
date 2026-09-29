//! A minimal synchronous 9P client over one connection (one endpoint handle), for native
//! programs, the client library and the panic handler. The words are [`crate::server::ninep`]'s.
//!
//! What it does and does not do:
//! - A [`Connection`] is shared: it holds the endpoint, a tag counter and a timeout, and no buffer and no
//!   fids, so several threads call on one connection at once. Which fids are in use is its caller's to track.
//! - Each request lends the caller's [`Lend`], one per thread, reused call after call; its size bounds each
//!   read and write ([`Lend::iounit`]).
//! - Only what native programs use today: version, attach, walk, open, read, write, clunk; and
//!   `ninep_common`'s `new_connection` and `disconnect`, for launchers.
//! - A walk is one `Twalk`: at most `MAXWELEM` (16) components after cleaning; a longer path is refused
//!   (`BadPath`) rather than split, so a failed walk never leaves a fid behind.
//! - The server is not trusted: a reply must decode, carry the request's tag and be the matching R-message,
//!   and every count it returns is checked against what was asked. A 9P reply carries no handles, so any that
//!   arrive are closed.
//! - An `Rerror`'s text is not kept (`Remote` says only that the server refused).

use core::sync::atomic::{AtomicU16, Ordering};

use redoubt_sys::Error;
use redoubt_wire::ninep::{Body, IOHDRSZ, Message, NOFID, NOTAG, Names, Qid, VERSION};
use redoubt_wire::proto::ninep_common;

use crate::handle::Endpoint;
use crate::ipc::Buffer;
use crate::path;
use crate::server::ninep::WORDS_9P;

/// Why a 9P request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    /// The call itself failed.
    Sys(Error),
    /// The request did not encode (too large for the buffer), or the reply did not decode.
    Wire(redoubt_wire::Error),
    /// The server answered `Rerror`, or walked only part of the path.
    Remote,
    /// The server's reply does not answer the request (wrong words, handles, tag, type or count).
    Unexpected,
    /// A path that does not clean ([`path::clean`]), or has more than `MAXWELEM` components.
    BadPath,
}

impl From<Error> for ClientError {
    fn from(e: Error) -> Self { ClientError::Sys(e) }
}

impl From<redoubt_wire::Error> for ClientError {
    fn from(e: redoubt_wire::Error) -> Self { ClientError::Wire(e) }
}

/// One call's lent buffer, reused call after call by one thread. The call it is lent to takes it,
/// and gives it back or consumes it (the server took the call and it was then abandoned: R3), never
/// both (kernel/ipc.md R13). A consumed lend is empty, and the next call made with it maps fresh
/// pages: the old address is never used again.
pub struct Lend {
    buf: Option<Buffer>,
    npages: usize,
}

impl Lend {
    /// A lend of `npages` pages (at most `MAX_LEND_PAGES`, the msize).
    pub fn new(npages: usize) -> Result<Lend, ClientError> {
        Ok(Lend { buf: Some(Buffer::new(npages)?), npages })
    }

    /// The most data one read or write carries in this lend.
    pub fn iounit(&self) -> usize {
        (self.npages * redoubt_sys::PAGE_SIZE).min(redoubt_wire::MSIZE).saturating_sub(IOHDRSZ)
    }

    /// What the last reply wrote; nothing if its call consumed the pages (it then failed).
    fn bytes(&self) -> &[u8] { self.buf.as_deref().unwrap_or(&[]) }

    /// The pages, mapped afresh if the last call consumed them.
    fn buffer(&mut self) -> Result<&mut Buffer, ClientError> {
        let buf = match self.buf.take() {
            Some(buf) => buf,
            None => Buffer::new(self.npages)?,
        };
        Ok(self.buf.insert(buf))
    }
}

/// One 9P connection, shared by the threads that call on it.
pub struct Connection {
    endpoint: Endpoint,
    tag: AtomicU16,
    /// Relative µs each request may take; `FOREVER` by default.
    pub timeout: u64,
}

impl Connection {
    /// A connection on `endpoint`, with no timeout.
    pub fn new(endpoint: Endpoint) -> Connection {
        Connection { endpoint, tag: AtomicU16::new(0), timeout: redoubt_sys::FOREVER }
    }

    pub fn endpoint(&self) -> &Endpoint { &self.endpoint }

    /// Gives the endpoint back.
    pub fn into_endpoint(self) -> Endpoint { self.endpoint }

    /// Calls with `lend`'s pages, which come back into `lend` whatever the status, unless the
    /// call consumed them.
    fn call(&self, lend: &mut Lend, words: &crate::ipc::Words) -> Result<crate::ipc::Reply, ClientError> {
        lend.buffer()?;
        let mut outcome = self.endpoint.call(words, &[], lend.buf.take(), self.timeout);
        lend.buf = outcome.buffer.take();
        Ok(outcome.into_result()?.0)
    }

    /// Sends `body` and returns the reply's body, which must be `body`'s R-message.
    fn rpc<'l>(&self, lend: &'l mut Lend, body: Body<'_>) -> Result<Body<'l>, ClientError> {
        // Tags are for matching a reply to its request in one lend; a fresh one each time is enough.
        let tag = if matches!(body, Body::Tversion { .. }) {
            NOTAG
        } else {
            self.tag.fetch_add(1, Ordering::Relaxed) % NOTAG
        };
        let want = body.kind() + 1;
        Message { tag, body }.encode(lend.buffer()?)?;
        let reply = self.call(lend, &WORDS_9P)?;
        // No 9P reply carries handles: close any a hostile server sent, before anything else.
        close_all(reply.handles.as_slice());
        if reply.words != WORDS_9P || !reply.handles.as_slice().is_empty() {
            return Err(ClientError::Unexpected);
        }
        let reply = Message::decode(lend.bytes())?;
        if reply.tag != tag {
            return Err(ClientError::Unexpected);
        }
        match reply.body {
            Body::Rerror { .. } => Err(ClientError::Remote),
            body if body.kind() == want => Ok(body),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `Tversion`: returns the msize the server accepts.
    pub fn version(&self, lend: &mut Lend) -> Result<u32, ClientError> {
        let msize = redoubt_wire::MSIZE as u32;
        match self.rpc(lend, Body::Tversion { msize, version: VERSION })? {
            Body::Rversion { msize: m, version } if m <= msize && version == VERSION => Ok(m),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `Tattach` without authentication: `fid` becomes the connection's root.
    pub fn attach(&self, lend: &mut Lend, fid: u32, aname: &str) -> Result<Qid, ClientError> {
        match self.rpc(lend, Body::Tattach { fid, afid: NOFID, uname: "", aname })? {
            Body::Rattach { qid } => Ok(qid),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Walks `newfid` from `fid` along `path`, cleaned first, so `..` never climbs above `fid`.
    /// Succeeds only if the whole path was walked; on failure `newfid` is not in use (intro(5)).
    pub fn walk(&self, lend: &mut Lend, fid: u32, newfid: u32, path: &str) -> Result<Qid, ClientError> {
        let names = path::clean(path).map_err(|_| ClientError::BadPath)?;
        let wnames = Names::new(&names).map_err(|_| ClientError::BadPath)?;
        match self.rpc(lend, Body::Twalk { fid, newfid, wnames })? {
            Body::Rwalk { qids } if qids.as_slice().len() == names.len() => {
                Ok(qids.as_slice().last().copied().unwrap_or_default())
            }
            Body::Rwalk { .. } => Err(ClientError::Remote),
            _ => Err(ClientError::Unexpected),
        }
    }

    pub fn open(&self, lend: &mut Lend, fid: u32, mode: u8) -> Result<Qid, ClientError> {
        match self.rpc(lend, Body::Topen { fid, mode })? {
            Body::Ropen { qid, .. } => Ok(qid),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Reads at most `out.len()` bytes (and at most [`Lend::iounit`]) at `offset`.
    pub fn read(&self, lend: &mut Lend, fid: u32, offset: u64, out: &mut [u8]) -> Result<usize, ClientError> {
        let count = out.len().min(lend.iounit());
        // `count` is at most the msize, so it fits.
        match self.rpc(lend, Body::Tread { fid, offset, count: count as u32 })? {
            Body::Rread { data } if data.len() <= count => {
                out[..data.len()].copy_from_slice(data);
                Ok(data.len())
            }
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Writes at most [`Lend::iounit`] bytes of `data` at `offset`; returns how many the
    /// server took.
    pub fn write(&self, lend: &mut Lend, fid: u32, offset: u64, data: &[u8]) -> Result<usize, ClientError> {
        let data = &data[..data.len().min(lend.iounit())];
        match self.rpc(lend, Body::Twrite { fid, offset, data })? {
            Body::Rwrite { count } if count as usize <= data.len() => Ok(count as usize),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `new_connection`: a fresh connection to this server rooted at `root` (relative to this
    /// connection's root; it never climbs above it), with `quota` bytes carved from this
    /// connection's quota (0 shares it). Returns the connection and its id, which only this
    /// connection may `disconnect`. What a launcher gives each child (servers/init.md, "Fresh
    /// connections per child").
    /// A refusal is `Remote`, whatever its reason: which root exists, which cap was reached and
    /// which quota was refused are the server's business, not the caller's.
    pub fn new_connection(
        &self,
        lend: &mut Lend,
        root: &str,
        quota: u64,
    ) -> Result<(Endpoint, u64), ClientError> {
        let request = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
        let words = request.encode(lend.buffer()?)?;
        let reply = self.call(lend, &words)?;
        let handles = reply.handles.as_slice();
        match ninep_common::Reply::decode(2, &reply.words, lend.bytes(), handles.len()) {
            Ok(Ok(ninep_common::Reply::NewConnection(r))) => match handles {
                [Some(conn)] => Ok((Endpoint::from_handle(*conn), r.id)),
                _ => {
                    close_all(handles);
                    Err(ClientError::Unexpected)
                }
            },
            // An error reply carries no handles, and a malformed one may carry any: close them.
            result => {
                close_all(handles);
                match result {
                    Ok(Err(_)) | Err(redoubt_wire::Error::BadStatus) => Err(ClientError::Remote),
                    _ => Err(ClientError::Unexpected),
                }
            }
        }
    }

    /// `disconnect`: frees the connection with `id`, which this connection received from
    /// [`Connection::new_connection`], and every connection minted under it. A `Remote` error is all
    /// a caller learns: a server answers an id belonging to someone else exactly as it answers
    /// one that never existed (servers/wire.md, `ninep_common`), so a client cannot probe for
    /// other clients' ids.
    pub fn disconnect(&self, id: u64) -> Result<(), ClientError> {
        let words = ninep_common::Message::Disconnect(ninep_common::Disconnect { id }).encode(&mut [])?;
        let (reply, _) = self.endpoint.call(&words, &[], None, self.timeout).into_result()?;
        close_all(reply.handles.as_slice());
        match ninep_common::Reply::decode(3, &reply.words, &[], reply.handles.as_slice().len()) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) | Err(redoubt_wire::Error::BadStatus) => Err(ClientError::Remote),
            Err(_) => Err(ClientError::Unexpected),
        }
    }

    pub fn clunk(&self, lend: &mut Lend, fid: u32) -> Result<(), ClientError> {
        match self.rpc(lend, Body::Tclunk { fid })? {
            Body::Rclunk => Ok(()),
            _ => Err(ClientError::Unexpected),
        }
    }
}

/// Closes every handle a reply brought that the client will not keep; a slot that arrived
/// empty (revoked on its way) has nothing to close.
fn close_all(handles: &[Option<redoubt_sys::Handle>]) {
    for handle in handles.iter().flatten() {
        let _ = crate::handle::close(*handle);
    }
}
