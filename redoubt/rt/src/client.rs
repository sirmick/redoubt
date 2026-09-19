//! A minimal synchronous 9P client over one connection (one endpoint handle), for native
//! programs and the panic handler. The words are [`crate::server::ninep`]'s.
//!
//! What it does and does not do:
//! - One request at a time, in one lent buffer of the client's own; the buffer bounds each read and write
//!   ([`Client::iounit`]).
//! - Only what native programs use today: version, attach, walk, open, read, write, clunk; and
//!   `ninep_common`'s `new_connection` and `disconnect`, for launchers.
//! - A walk is one `Twalk`: at most `MAXWELEM` (16) components after cleaning; a longer path is refused
//!   (`BadPath`) rather than split, so a failed walk never leaves a fid behind.
//! - The server is not trusted: a reply must decode, carry the request's tag and be the matching R-message,
//!   and every count it returns is checked against what was asked. A 9P reply carries no handles, so any that
//!   arrive are closed.
//! - An `Rerror`'s text is not kept (`Remote` says only that the server refused).

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

/// One 9P connection.
pub struct Client {
    endpoint: Endpoint,
    buf: Buffer,
    tag: u16,
    /// Relative µs each request may take; `FOREVER` by default.
    pub timeout: u64,
}

impl Client {
    /// A client on `endpoint` with a lend of `npages` pages (at most `MAX_LEND_PAGES`, the msize).
    pub fn new(endpoint: Endpoint, npages: usize) -> Result<Client, ClientError> {
        let buf = Buffer::new(npages)?;
        Ok(Client { endpoint, buf, tag: 0, timeout: redoubt_sys::FOREVER })
    }

    /// Gives the endpoint back.
    pub fn into_endpoint(self) -> Endpoint { self.endpoint }

    /// The most data one read or write carries with this client's buffer.
    pub fn iounit(&self) -> usize { self.buf.len().min(redoubt_wire::MSIZE).saturating_sub(IOHDRSZ) }

    /// Sends `body` and returns the reply's body, which must be `body`'s R-message.
    fn rpc(&mut self, body: Body<'_>) -> Result<Body<'_>, ClientError> {
        // Tags are for matching replies; one request at a time needs only a fresh one each time.
        self.tag = self.tag.wrapping_add(1) % NOTAG;
        let tag = if matches!(body, Body::Tversion { .. }) { NOTAG } else { self.tag };
        let want = body.kind() + 1;
        Message { tag, body }.encode(&mut self.buf)?;
        let reply = self.endpoint.call(&WORDS_9P, &[], Some(&mut self.buf), self.timeout)?;
        // No 9P reply carries handles: close any a hostile server sent, before anything else.
        close_all(reply.handles.as_slice());
        if reply.words != WORDS_9P || !reply.handles.as_slice().is_empty() {
            return Err(ClientError::Unexpected);
        }
        let reply = Message::decode(&self.buf)?;
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
    pub fn version(&mut self) -> Result<u32, ClientError> {
        let msize = redoubt_wire::MSIZE as u32;
        match self.rpc(Body::Tversion { msize, version: VERSION })? {
            Body::Rversion { msize: m, version } if m <= msize && version == VERSION => Ok(m),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `Tattach` without authentication: `fid` becomes the connection's root.
    pub fn attach(&mut self, fid: u32, aname: &str) -> Result<Qid, ClientError> {
        match self.rpc(Body::Tattach { fid, afid: NOFID, uname: "", aname })? {
            Body::Rattach { qid } => Ok(qid),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Walks `newfid` from `fid` along `path`, cleaned first, so `..` never climbs above `fid`.
    /// Succeeds only if the whole path was walked; on failure `newfid` is not in use (intro(5)).
    pub fn walk(&mut self, fid: u32, newfid: u32, path: &str) -> Result<Qid, ClientError> {
        let names = path::clean(path).map_err(|_| ClientError::BadPath)?;
        let wnames = Names::new(&names).map_err(|_| ClientError::BadPath)?;
        match self.rpc(Body::Twalk { fid, newfid, wnames })? {
            Body::Rwalk { qids } if qids.as_slice().len() == names.len() => {
                Ok(qids.as_slice().last().copied().unwrap_or_default())
            }
            Body::Rwalk { .. } => Err(ClientError::Remote),
            _ => Err(ClientError::Unexpected),
        }
    }

    pub fn open(&mut self, fid: u32, mode: u8) -> Result<Qid, ClientError> {
        match self.rpc(Body::Topen { fid, mode })? {
            Body::Ropen { qid, .. } => Ok(qid),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Reads at most `out.len()` bytes (and at most [`Client::iounit`]) at `offset`.
    pub fn read(&mut self, fid: u32, offset: u64, out: &mut [u8]) -> Result<usize, ClientError> {
        let count = out.len().min(self.iounit());
        // `count` is at most the msize, so it fits.
        match self.rpc(Body::Tread { fid, offset, count: count as u32 })? {
            Body::Rread { data } if data.len() <= count => {
                out[..data.len()].copy_from_slice(data);
                Ok(data.len())
            }
            _ => Err(ClientError::Unexpected),
        }
    }

    /// Writes at most [`Client::iounit`] bytes of `data` at `offset`; returns how many the
    /// server took.
    pub fn write(&mut self, fid: u32, offset: u64, data: &[u8]) -> Result<usize, ClientError> {
        let data = &data[..data.len().min(self.iounit())];
        match self.rpc(Body::Twrite { fid, offset, data })? {
            Body::Rwrite { count } if count as usize <= data.len() => Ok(count as usize),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// `new_connection`: a fresh connection to this server rooted at `root` (relative to this
    /// connection's root; it never climbs above it), with `quota` bytes carved from this
    /// connection's quota (0 shares it). Returns the connection and its id, which only this
    /// connection may `disconnect`. What a launcher gives each child (INIT.md, launching gives
    /// fresh connections).
    pub fn new_connection(&mut self, root: &str, quota: u64) -> Result<(Endpoint, u64), ClientError> {
        let request = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
        let words = request.encode(&mut self.buf)?;
        let reply = self.endpoint.call(&words, &[], Some(&mut self.buf), self.timeout)?;
        let handles = reply.handles.as_slice();
        match ninep_common::Reply::decode(2, &reply.words, &self.buf, handles.len()) {
            Ok(Ok(ninep_common::Reply::NewConnection(r))) => match handles {
                [Some(conn)] => Ok((Endpoint::from_handle(*conn), r.id)),
                _ => Err(ClientError::Unexpected),
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
    /// [`Client::new_connection`], and every connection minted under it.
    pub fn disconnect(&mut self, id: u64) -> Result<(), ClientError> {
        let words = ninep_common::Message::Disconnect(ninep_common::Disconnect { id }).encode(&mut [])?;
        let reply = self.endpoint.call(&words, &[], None, self.timeout)?;
        close_all(reply.handles.as_slice());
        match ninep_common::Reply::decode(3, &reply.words, &[], reply.handles.as_slice().len()) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) | Err(redoubt_wire::Error::BadStatus) => Err(ClientError::Remote),
            Err(_) => Err(ClientError::Unexpected),
        }
    }

    pub fn clunk(&mut self, fid: u32) -> Result<(), ClientError> {
        match self.rpc(Body::Tclunk { fid })? {
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
