//! The server side of `tests/ninep-newconn-discard.toml`: a 9P server on the runtime's own
//! skeleton (`NineServer::serve`, the real serve path), holding the boot endpoint's receive right.
//! `ninep-discard-client` only ever asks for `new_connection`, with its handle table full, so
//! each reply's capability is dropped by the kernel on the way. The skeleton must roll back each
//! such connection and its admission charge (servers/serving.md, "Replies and rollback").
//!
//! The verdict is this server's, read from its own tables after each call, never the client's:
//! the first call that leaves a connection standing is noted, with how many before it were rolled
//! back and what the client's bucket holds; the next, the client giving that connection back, ends
//! the case, naming what the bucket holds then.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event};
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, NineError, NineServer, QTDIR, Qid, Read};
use redoubt_rt::server::{Limits, Resource};
use test_programs::{Logger, checker, log};

redoubt_rt::panic_handler!();

/// Few connections per bucket, so that the client's discarded replies outnumber them.
const LIMITS: Limits = Limits { buckets: 2, in_flight: 0, files: 2, state: 4 };

/// A server with one empty directory: a connection needs a root, nothing more.
struct Empty;

const ROOT: Qid = Qid { kind: QTDIR, version: 0, path: 0 };

impl FileServer for Empty {
    type Node = ();

    fn attach(&mut self, _: &Caller, _: &str) -> Result<((), Qid), NineError> { Ok(((), ROOT)) }

    fn labels(&self, _: &()) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &(), _: &str) -> Result<((), Qid), NineError> {
        Err(NineError::NOT_FOUND)
    }

    fn open(&mut self, _: &Caller, _: &(), _: u8) -> Result<Qid, NineError> { Ok(ROOT) }

    fn read(&mut self, _: &Caller, _: &(), _: u64, _: &mut [u8]) -> Result<Read, NineError> {
        Ok(Read::Done(0))
    }

    fn write(&mut self, _: &Caller, _: &(), _: u64, _: &[u8]) -> Result<usize, NineError> {
        Err(NineError::NOT_SUPPORTED)
    }

    fn stat(&mut self, _: &Caller, _: &()) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: ROOT, mode: DMDIR | 0o555, mtime: 0, length: 0, name: String::from("/") })
    }

    fn dir_entry(&mut self, _: &Caller, _: &(), _: u64) -> Result<Option<((), FileStat)>, NineError> {
        Ok(None)
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let endpoint = Endpoint::from_handle(Handle::new(1).expect("slot 1"));
    let random = redoubt_rt::handle::random_u64().expect("a random word");
    let mut server = NineServer::new(Empty, LIMITS, random).expect("limits that fit");
    log!(logger, "[ninep-server] serving, {} connections per bucket", LIMITS.state);
    let mut rolled_back = 0;
    let mut standing = None;
    loop {
        let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { continue };
        let (key, _) = server.charge_of(&request.caller);
        let _ = server.serve(request);
        let held = server.admission().held(key, Resource::State);
        match standing {
            None if server.connections() == 0 => rolled_back += 1,
            None => standing = Some(held),
            Some(stood) => {
                log!(
                    logger,
                    "[ninep-server] a connection stood after {} rolled back; its bucket held {}, then {}",
                    rolled_back,
                    stood,
                    held
                );
                checker::done();
                test_programs::park()
            }
        }
    }
}
