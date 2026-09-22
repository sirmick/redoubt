//! `redoubt-rt`'s own run of the 9P2000 conformance vectors (`libs/wire/vectors/9p.txt`).
//!
//! The vector runner (`common/vectors.rs`) is a shared module the servers include; this test
//! exists so it is also compiled and exercised where it lives, against a minimal server, rather
//! than only from a server package that may not have landed yet. A server that never waits must
//! see no `Read::Wait`, which is the one count that is a property of *this* server.

mod common;

#[path = "common/vectors.rs"]
mod vectors;

use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, NineServer, Qid, Read, mode};
use redoubt_rt::server::Limits;

/// A server that serves one small file and otherwise refuses: enough for every vector line, and
/// it never asks to hold a call.
struct OneFile;

impl FileServer for OneFile {
    type Node = u8;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(u8, Qid), NineError> {
        Ok((0, Qid { kind: 0, version: 0, path: 0 }))
    }

    fn labels(&self, _: &u8) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &u8, name: &str) -> Result<(u8, Qid), NineError> {
        if name == "file" {
            Ok((1, Qid { kind: 0, version: 0, path: 1 }))
        } else {
            Err(NineError::NOT_FOUND)
        }
    }

    fn open(&mut self, _: &Caller, _: &u8, mode: u8) -> Result<Qid, NineError> {
        if mode & mode::OWRITE != 0 {
            return Err(NineError::PERMISSION);
        }
        Ok(Qid { kind: 0, version: 0, path: 1 })
    }

    fn read(&mut self, _: &Caller, node: &u8, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        const DATA: &[u8] = b"redoubt\n";
        if *node != 1 {
            return Err(NineError::NOT_FOUND);
        }
        let from = (offset as usize).min(DATA.len());
        let n = out.len().min(DATA.len() - from);
        out[..n].copy_from_slice(&DATA[from..from + n]);
        Ok(Read::Done(n))
    }

    fn write(&mut self, _: &Caller, _: &u8, _: u64, _: &[u8]) -> Result<usize, NineError> {
        Err(NineError::PERMISSION)
    }

    fn stat(&mut self, _: &Caller, node: &u8) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: Qid { kind: 0, version: 0, path: u64::from(*node) }, ..FileStat::default() })
    }

    fn dir_entry(&mut self, _: &Caller, _: &u8, _: u64) -> Result<Option<(u8, FileStat)>, NineError> {
        Ok(None)
    }
}

#[test]
fn the_9p_conformance_vectors_hold_for_a_minimal_server() {
    let limits = Limits { buckets: 8, in_flight: 0, files: 64, state: 8 };
    let mut server = NineServer::new(OneFile, limits, 0).expect("a server");
    let who = Caller { badge: 1, account: 1001, labels: redoubt_rt::abi::Labels::from_slice(&[]).unwrap() };
    let counts = vectors::run(&mut server, &who);

    // The corpus is non-empty and split, so a silent no-op run cannot pass.
    assert!(counts.well_formed > 0, "the vectors have ok lines: {counts:?}");
    assert!(counts.malformed > 0, "the vectors have bad lines: {counts:?}");
    // Every line was answered or held, and the totals account for all of them.
    assert_eq!(
        counts.refused + counts.answered + counts.waiting,
        counts.well_formed + counts.malformed,
        "every vector was answered, refused or held: {counts:?}"
    );
    assert!(counts.refused > 0, "hostile lines are refused: {counts:?}");
    // This server never asks to hold a call, so no vector line parks.
    assert_eq!(counts.waiting, 0, "a server that never waits parks nothing: {counts:?}");
}