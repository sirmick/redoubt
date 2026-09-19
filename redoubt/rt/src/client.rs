//! A minimal synchronous 9P client over one connection (one endpoint handle), for native
//! programs and the panic handler. One request at a time, in one lent buffer; the protocol's
//! words are [`crate::server::ninep`]'s.
//!
//! The server is not trusted: a reply must decode, carry the request's tag and be the matching
//! R-message, and every count it returns is checked against what was asked.

use redoubt_sys::Error;
use redoubt_wire::ninep::{Body, IOHDRSZ, MAXWELEM, Message, NOFID, Names, Qid, Stat};

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
    /// The server answered `Rerror`; its text is in [`Client::last_error`].
    Remote,
    /// The server's reply does not answer the request (wrong words, tag, type or count).
    Unexpected,
    /// A path that does not clean ([`path::clean`]).
    BadPath,
}

impl From<Error> for ClientError {
    fn from(e: Error) -> Self { ClientError::Sys(e) }
}

impl From<redoubt_wire::Error> for ClientError {
    fn from(e: redoubt_wire::Error) -> Self { ClientError::Wire(e) }
}

/// Bytes of an `Rerror` text kept for [`Client::last_error`]; fixed, so an error can be read
/// without allocating (the panic handler uses this client).
const ERROR_TEXT: usize = 64;

/// One 9P connection.
pub struct Client {
    endpoint: Endpoint,
    buf: Buffer,
    tag: u16,
    /// Relative µs each request may take; `FOREVER` by default.
    pub timeout: u64,
    error: [u8; ERROR_TEXT],
    error_len: usize,
}

impl Client {
    /// A client on `endpoint` with a lend of `npages` pages (at most `MAX_LEND_PAGES`, the msize).
    pub fn new(endpoint: Endpoint, npages: usize) -> Result<Client, ClientError> {
        let buf = Buffer::new(npages)?;
        Ok(Client {
            endpoint,
            buf,
            tag: 0,
            timeout: redoubt_sys::FOREVER,
            error: [0; ERROR_TEXT],
            error_len: 0,
        })
    }

    /// Gives the endpoint back.
    pub fn into_endpoint(self) -> Endpoint { self.endpoint }

    /// The text of the last `Rerror`, cut to 64 bytes at a character boundary.
    pub fn last_error(&self) -> &str {
        let bytes = self.error.get(..self.error_len).unwrap_or(&[]);
        core::str::from_utf8(bytes).unwrap_or("")
    }

    /// The most data one read or write carries with this client's buffer.
    pub fn iounit(&self) -> usize { self.buf.len().min(redoubt_wire::MSIZE).saturating_sub(IOHDRSZ) }

    /// Sends `body` and returns the reply's body, which must be `body`'s R-message.
    fn rpc(&mut self, body: Body<'_>) -> Result<Body<'_>, ClientError> {
        // Tags are for matching replies; one request at a time needs only a fresh one each time.
        self.tag = self.tag.wrapping_add(1) % redoubt_wire::ninep::NOTAG;
        let tag = if matches!(body, Body::Tversion { .. }) { redoubt_wire::ninep::NOTAG } else { self.tag };
        let want = body.kind() + 1;
        Message { tag, body }.encode(&mut self.buf)?;
        let reply = self.endpoint.call(&WORDS_9P, &[], Some(&mut self.buf), self.timeout)?;
        if reply.words != WORDS_9P || !reply.handles.as_slice().is_empty() {
            return Err(ClientError::Unexpected);
        }
        let reply = Message::decode(&self.buf)?;
        if reply.tag != tag {
            return Err(ClientError::Unexpected);
        }
        if let Body::Rerror { ename } = reply.body {
            let mut len = ename.len().min(ERROR_TEXT);
            while !ename.is_char_boundary(len) {
                len -= 1;
            }
            self.error[..len].copy_from_slice(&ename.as_bytes()[..len]);
            self.error_len = len;
            return Err(ClientError::Remote);
        }
        if reply.body.kind() != want {
            return Err(ClientError::Unexpected);
        }
        Ok(reply.body)
    }

    /// `Tversion`: returns the msize the server accepts.
    pub fn version(&mut self) -> Result<u32, ClientError> {
        let msize = redoubt_wire::MSIZE as u32;
        match self.rpc(Body::Tversion { msize, version: redoubt_wire::ninep::VERSION })? {
            Body::Rversion { msize: m, version } if m <= msize && version == redoubt_wire::ninep::VERSION => {
                Ok(m)
            }
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

    /// Walks `newfid` from `fid` along `path`, cleaned first, so `..` never climbs above
    /// `fid`. Succeeds only if the whole path was walked; on failure `newfid` is not in use.
    pub fn walk(&mut self, fid: u32, newfid: u32, path: &str) -> Result<Qid, ClientError> {
        let names = path::clean(path).map_err(|_| ClientError::BadPath)?;
        let mut from = fid;
        let mut qid = None;
        // A walk carries at most MAXWELEM names; a longer path is several, from `newfid` on.
        let mut chunks = names.chunks(MAXWELEM).peekable();
        if chunks.peek().is_none() {
            return self.walk_names(fid, newfid, &[]).map(|_| Qid::default());
        }
        for chunk in chunks {
            match self.walk_names(from, newfid, chunk) {
                Ok(last) => qid = Some(last),
                Err(e) => {
                    // Walked partway: `newfid` exists and must go (unless it is the caller's `fid`).
                    if from == newfid && newfid != fid {
                        let _ = self.clunk(newfid);
                    }
                    return Err(e);
                }
            }
            from = newfid;
        }
        qid.ok_or(ClientError::Unexpected)
    }

    /// One `Twalk`; the last qid, and only if every name was walked.
    fn walk_names(&mut self, fid: u32, newfid: u32, names: &[&str]) -> Result<Qid, ClientError> {
        let wnames = Names::new(names)?;
        match self.rpc(Body::Twalk { fid, newfid, wnames })? {
            Body::Rwalk { qids } if qids.as_slice().len() == names.len() => {
                Ok(qids.as_slice().last().copied().unwrap_or_default())
            }
            // A partial walk leaves `newfid` unused (intro(5)); for us it is a failure.
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

    /// `Tcreate` in the directory `fid`, which becomes the new file, opened with `mode`.
    pub fn create(&mut self, fid: u32, name: &str, perm: u32, mode: u8) -> Result<Qid, ClientError> {
        match self.rpc(Body::Tcreate { fid, name, perm, mode })? {
            Body::Rcreate { qid, .. } => Ok(qid),
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

    pub fn remove(&mut self, fid: u32) -> Result<(), ClientError> {
        match self.rpc(Body::Tremove { fid })? {
            Body::Rremove => Ok(()),
            _ => Err(ClientError::Unexpected),
        }
    }

    /// The file's stat, borrowed from the client's buffer until the next request.
    pub fn stat(&mut self, fid: u32) -> Result<Stat<'_>, ClientError> {
        match self.rpc(Body::Tstat { fid })? {
            Body::Rstat { stat } => Ok(stat),
            _ => Err(ClientError::Unexpected),
        }
    }
}
