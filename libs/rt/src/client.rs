//! A minimal synchronous 9P client over one connection (one endpoint handle), for native
//! programs, the client library and the panic handler. The words are [`crate::server::ninep`]'s.
//!
//! What it does and does not do:
//! - A [`Connection`] is shared: it holds the endpoint, a tag counter and a timeout, and no buffer and no
//!   fids, so several threads call on one connection at once. Which fids are in use is its caller's to track.
//! - Each request lends the caller's [`Lend`], one per thread, reused call after call; its size bounds each
//!   read and write ([`Lend::iounit`]).
//! - version, attach, walk, open, create, read, write, stat, remove, clunk; and `ninep_common`'s
//!   `new_connection` and `disconnect`, for launchers.
//! - A walk is one `Twalk`: at most `MAXWELEM` (16) components after cleaning; a longer path is refused
//!   (`BadPath`) rather than split, so a failed walk never leaves a fid behind.
//! - The server is not trusted: a reply must decode, carry the request's tag and be the matching R-message,
//!   and every count it returns is checked against what was asked. A 9P reply carries no handles, so any that
//!   arrive are closed.
//! - An `Rerror`'s text is not kept, only its name in the one table ([`ErrorName`]; servers/wire.md, "Error
//!   names"): `Rerror(NotFound)` for `file does not exist` and for a walk that stopped short. A typed refusal
//!   of `new_connection` or `disconnect` is `Remote`, which says only that the server refused.

use core::sync::atomic::{AtomicU16, Ordering};

use redoubt_sys::{Error, Handle};
use redoubt_wire::ninep::{Body, ErrorName, IOHDRSZ, Message, NOFID, NOTAG, Names, Qid, Stat, VERSION};
use redoubt_wire::proto::ninep_common;

use crate::handle::Endpoint;
use crate::ipc::{Buffer, CallOutcome, Words};
use crate::path;
use crate::server::ninep::WORDS_9P;

/// Why a 9P request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    /// The call itself failed.
    Sys(Error),
    /// The reply did not decode.
    Wire(redoubt_wire::Error),
    /// The request did not encode (too large for the lend, or the msize): nothing was sent.
    Encode(redoubt_wire::Error),
    /// The lend's pages could not be mapped: nothing was sent.
    Pages(Error),
    /// The server answered `Rerror`, by its text's name; or walked only part of the path, which
    /// is `NotFound`: the name is not there.
    Rerror(ErrorName),
    /// The server refused a typed request (`new_connection`, `disconnect`), whatever its reason.
    Remote,
    /// The server's reply does not answer the request (wrong words, handles, tag, type or count).
    Unexpected,
    /// A path that does not clean ([`path::clean`]), or has more than `MAXWELEM` components:
    /// nothing was sent.
    BadPath,
}

impl ClientError {
    /// Whether the request was refused here, so the server never saw it.
    pub fn unsent(&self) -> bool {
        matches!(self, ClientError::Encode(_) | ClientError::Pages(_) | ClientError::BadPath)
    }
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
        Ok(Lend { buf: Some(Buffer::new(npages).map_err(ClientError::Pages)?), npages })
    }

    /// The most data one read or write carries in this lend.
    pub fn iounit(&self) -> usize {
        (self.npages * redoubt_sys::PAGE_SIZE).min(redoubt_wire::MSIZE).saturating_sub(IOHDRSZ)
    }

    /// What the last reply wrote; nothing if its call consumed the pages (it then failed).
    pub fn bytes(&self) -> &[u8] { self.buf.as_deref().unwrap_or(&[]) }

    /// The pages, to write a request into: mapped afresh if the last call consumed them.
    pub fn pages(&mut self) -> Result<&mut [u8], ClientError> { Ok(self.buffer()?) }

    /// Calls `to`, lending these pages. The outcome's buffer is back in the lend, or consumed;
    /// its status and reply are the caller's, as [`Endpoint::call`] gives them.
    pub fn call(&mut self, to: &Endpoint, words: &Words, handles: &[Handle], timeout: u64) -> CallOutcome {
        let mut outcome = to.call(words, handles, self.buf.take(), timeout);
        self.buf = outcome.buffer.take();
        outcome
    }

    fn buffer(&mut self) -> Result<&mut Buffer, ClientError> {
        let buf = match self.buf.take() {
            Some(buf) => buf,
            None => Buffer::new(self.npages).map_err(ClientError::Pages)?,
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
    pub fn new(endpoint: Endpoint) -> Connection { Connection::within(endpoint, redoubt_sys::FOREVER) }

    /// A connection on `endpoint` whose every call waits at most `timeout` µs for its reply.
    pub fn within(endpoint: Endpoint, timeout: u64) -> Connection {
        Connection { endpoint, tag: AtomicU16::new(0), timeout }
    }

    pub fn endpoint(&self) -> &Endpoint { &self.endpoint }

    /// Gives the endpoint back.
    pub fn into_endpoint(self) -> Endpoint { self.endpoint }

    /// Calls with `lend`'s pages.
    fn call(&self, lend: &mut Lend, words: &Words) -> Result<crate::ipc::Reply, ClientError> {
        lend.buffer()?;
        Ok(lend.call(&self.endpoint, words, &[], self.timeout).into_result()?.0)
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
        Message { tag, body }.encode(lend.buffer()?).map_err(ClientError::Encode)?;
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
            Body::Rerror { ename } => Err(ClientError::Rerror(ErrorName::of(ename))),
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
            Body::Rwalk { .. } => Err(ClientError::Rerror(ErrorName::NotFound)),
            _ => Err(ClientError::Unexpected),
        }
    }

    pub fn open(&self, lend: &mut Lend, fid: u32, mode: u8) -> Result<Qid, ClientError> {
        match self.rpc(lend, Body::Topen { fid, mode })? {
            Body::Ropen { qid, .. } => Ok(qid),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `Tcreate`: creates `name` in the directory `fid` and opens it with `mode`; `fid` becomes
    /// the new file. Which names are refused is the server's to say.
    pub fn create(
        &self,
        lend: &mut Lend,
        fid: u32,
        name: &str,
        perm: u32,
        mode: u8,
    ) -> Result<Qid, ClientError> {
        match self.rpc(lend, Body::Tcreate { fid, name, perm, mode })? {
            Body::Rcreate { qid, .. } => Ok(qid),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `Tstat`: the file's directory entry, in `lend` until its next call.
    pub fn stat<'l>(&self, lend: &'l mut Lend, fid: u32) -> Result<Stat<'l>, ClientError> {
        match self.rpc(lend, Body::Tstat { fid })? {
            Body::Rstat { stat } => Ok(stat),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `Tremove`: removes the file and clunks `fid`, whether or not the removal succeeds
    /// (intro(5)).
    pub fn remove(&self, lend: &mut Lend, fid: u32) -> Result<(), ClientError> {
        match self.rpc(lend, Body::Tremove { fid })? {
            Body::Rremove => Ok(()),
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
        let words = request.encode(lend.buffer()?).map_err(ClientError::Encode)?;
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
    /// other clients' ids. Waits at most `timeout` µs, not the connection's own: a launcher reaping
    /// a child bounds each release, however the connection's requests are timed.
    pub fn disconnect(&self, id: u64, timeout: u64) -> Result<(), ClientError> {
        let words = ninep_common::Message::Disconnect(ninep_common::Disconnect { id })
            .encode(&mut [])
            .map_err(ClientError::Encode)?;
        let (reply, _) = self.endpoint.call(&words, &[], None, timeout).into_result()?;
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
fn close_all(handles: &[Option<Handle>]) {
    for handle in handles.iter().flatten() {
        let _ = crate::handle::close(*handle);
    }
}
