//! A minimal synchronous 9P client over one connection (one endpoint handle), for native
//! programs and the panic handler. The words are [`crate::server::ninep`]'s.
//!
//! What it does and does not do:
//! - One request at a time, in one lent buffer of the client's own; the buffer bounds each read and write
//!   ([`Client::iounit`]).
//! - Only what native programs use today: version, attach, walk, open, read, write, clunk.
//! - A walk is one `Twalk`: at most `MAXWELEM` (16) components after cleaning; a longer path is refused
//!   (`BadPath`) rather than split, so a failed walk never leaves a fid behind.
//! - The server is not trusted: a reply must decode, carry the request's tag and be the matching R-message,
//!   and every count it returns is checked against what was asked. A 9P reply carries no handles, so any that
//!   arrive are closed.
//! - An `Rerror`'s text is not kept (`Remote` says only that the server refused).

use redoubt_sys::Error;
use redoubt_wire::ninep::{Body, IOHDRSZ, Message, NOFID, NOTAG, Names, Qid, VERSION};

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
        for handle in reply.handles.as_slice() {
            let _ = crate::handle::close(*handle);
        }
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

    pub fn clunk(&mut self, fid: u32) -> Result<(), ClientError> {
        match self.rpc(Body::Tclunk { fid })? {
            Body::Rclunk => Ok(()),
            _ => Err(ClientError::Unexpected),
        }
    }
}
