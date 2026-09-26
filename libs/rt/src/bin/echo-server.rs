//! The echo server: serves one file, `/echo`, over 9P through the runtime's server skeleton.
//! Whatever is written to it reads back. It receives on the endpoint named `echo` in its
//! startup block, and exits 0 when that endpoint is destroyed.
//!
//! Built only on `redoubt-rt`. On the host, `tests/echo.rs` runs it against a fake kernel.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event};
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, NineError, NineServer, QTDIR, Qid, Read, mode};
use redoubt_rt::server::{Cost, Limits};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The most the file holds.
const MAX_DATA: usize = 64 * 1024;

/// Exit codes: 0 when the endpoint goes away.
pub const NO_ENDPOINT: u32 = 2;
pub const RECEIVE_FAILED: u32 = 3;
pub const BAD_LIMITS: u32 = 4;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (servers/serving.md R27). A server that cannot get one does not start.
pub const NO_RANDOM: u32 = 5;

/// What admission lets clients hold: sized so that every bucket at its cap fits [`BUDGET`]
/// (servers/serving.md, `admit`). The skeleton holds no calls open, so none are admitted in flight.
pub const LIMITS: Limits = Limits { buckets: 16, in_flight: 0, files: 32, state: 8 };
/// What one of each costs, in bytes: a fid is its table entry and a few steps; a connection its
/// record and a quota.
pub const COST: Cost = Cost { in_flight: 0, file: 256, state: 256 };
/// The bytes of this server's budget clients may use (its manifest gives the budget).
pub const BUDGET: u64 = 256 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Root,
    Echo,
}

#[derive(Default)]
pub struct EchoFs {
    data: Vec<u8>,
}

fn qid(node: Node) -> Qid {
    match node {
        Node::Root => Qid { kind: QTDIR, version: 0, path: 0 },
        Node::Echo => Qid { kind: 0, version: 0, path: 1 },
    }
}

impl FileServer for EchoFs {
    type Node = Node;

    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Node, Qid), NineError> {
        Ok((Node::Root, qid(Node::Root)))
    }

    /// Unlabelled: anyone may read; only unlabelled callers may write (no write down).
    fn labels(&self, _: &Node) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        match name {
            "echo" => Ok((Node::Echo, qid(Node::Echo))),
            _ => Err(NineError::NOT_FOUND),
        }
    }

    fn open(&mut self, _: &Caller, node: &Node, open_mode: u8) -> Result<Qid, NineError> {
        if open_mode & mode::OTRUNC != 0 {
            self.data.clear();
        }
        Ok(qid(*node))
    }

    /// Never waits: what has not been written yet reads as the end of the file.
    fn read(&mut self, _: &Caller, _: &Node, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        // Any offset is the client's: past the end reads nothing.
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(self.data.len());
        let n = out.len().min(self.data.len() - start);
        out[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, _: &Node, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let start = usize::try_from(offset).ok().filter(|o| *o <= MAX_DATA).ok_or(NineError::BAD_OFFSET)?;
        let n = data.len().min(MAX_DATA - start);
        if self.data.len() < start + n {
            self.data.resize(start + n, 0);
        }
        self.data[start..start + n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> { Ok(self.stat_of(*node)) }

    fn dir_entry(&mut self, _: &Caller, _: &Node, index: u64) -> Result<Option<(Node, FileStat)>, NineError> {
        Ok((index == 0).then(|| (Node::Echo, self.stat_of(Node::Echo))))
    }
}

impl EchoFs {
    fn stat_of(&self, node: Node) -> FileStat {
        let (mode, name) = match node {
            Node::Root => (DMDIR | 0o555, String::from("/")),
            Node::Echo => (0o666, String::from("echo")),
        };
        FileStat { qid: qid(node), mode, mtime: 0, length: self.data.len() as u64, name }
    }
}

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle("echo") else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(handle);
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(EchoFs::default(), LIMITS, random) else { return BAD_LIMITS };
    loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // A failed reply means the caller is gone; there is nobody to tell.
                let _ = server.serve(request);
            }
            // Nothing here is sent one-way: drop it, and close what it brought.
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            // No call is ever held open here, so no abandoned-call notice names one.
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
    }
}
