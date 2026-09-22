//! The 9P2000 conformance vectors (`redoubt/wire/vectors/9p.txt`, WP-W1) run against a real
//! server, which is what that file says they are for: "for servers' conformance tests too".
//!
//! Every line is put at the front of a lend and handed to the server exactly as a client's 9P
//! call would be. What is checked, for every server that includes this module:
//!
//! - **Nothing panics**, on any line.
//! - **Every answer is a well-formed 9P message.** A reply that does not decode would be a server the client
//!   cannot parse, which is the same as no server at all.
//! - **Every answer carries the request's tag**, so a client matches replies to requests.
//! - **Every line the codec refuses is an `Rerror`** — never a reply pretending the request was understood,
//!   and never a read or a write.
//! - **Every R-message is an `Rerror`**: replies travel from servers, so one arriving at a server is not a
//!   request (`intro(5)`), and a server that answered one would be taking orders from something pretending to
//!   be a server.
//! - **Nothing a hostile client sends mints a connection**, and a `Tversion` afterwards leaves the connection
//!   with no fids at all, so the run holds nothing when it ends.
//!
//! Each server's own test then checks what only it knows: that the bytes it serves are right.

#![allow(dead_code)]

use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{Answer, FileServer, NineServer};
use redoubt_rt::wire::MSIZE;
use redoubt_rt::wire::ninep::{Body, Message};

/// What the run saw.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Lines the codec accepts (`ok`).
    pub well_formed: usize,
    /// Lines the codec refuses (`bad`).
    pub malformed: usize,
    /// Answers that were an `Rerror`.
    pub refused: usize,
    /// Answers that were an R-message of the request's kind.
    pub answered: usize,
    /// Requests the file server asked to hold instead of answering (`Read::Wait`). A server
    /// that never waits must see none; `consoled` may, and its own test says when.
    pub waiting: usize,
}

/// The vectors, as (kind, bytes): `true` for an `ok` line, `false` for a `bad` one.
pub fn lines() -> Vec<(bool, Vec<u8>)> {
    // The corpus lives in `libs/wire/vectors/`, and this module is included by servers whose
    // manifest directories are at different depths, so walk up from this crate to the workspace
    // root rather than assuming a fixed relative path.
    let path = {
        let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            let candidate = dir.join("libs/wire/vectors/9p.txt");
            if candidate.is_file() {
                break candidate;
            }
            if !dir.pop() {
                panic!("libs/wire/vectors/9p.txt not found above {}", env!("CARGO_MANIFEST_DIR"));
            }
        }
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let kind = fields.next().expect("a kind");
        let hex = fields.next().unwrap_or_else(|| panic!("no bytes in {line:?}"));
        assert!(hex.len().is_multiple_of(2), "odd hex in {line:?}");
        let bytes =
            (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        match kind {
            "ok" => out.push((true, bytes)),
            "bad" => out.push((false, bytes)),
            other => panic!("unknown vector kind {other:?}"),
        }
    }
    assert!(out.len() > 30, "the vector file looks truncated: {} lines", out.len());
    out
}

/// Runs every vector against `server` as `who`.
pub fn run<S: FileServer>(server: &mut NineServer<S>, who: &Caller) -> Counts {
    let mut counts = Counts::default();
    let conns = server.connections();
    for (well_formed, bytes) in lines() {
        let mut lend = vec![0u8; MSIZE];
        lend[..bytes.len()].copy_from_slice(&bytes);
        let request = Message::decode(&bytes);
        assert_eq!(request.is_ok(), well_formed, "the vector file and the codec disagree about {bytes:02x?}");
        let answer = server.answer_in_place(who, &mut lend);
        if answer == Answer::Waiting {
            // Held for later: nothing was written, so the request is still there to serve.
            assert_eq!(&lend[..bytes.len()], &bytes[..], "a held request was changed");
            counts.waiting += 1;
            continue;
        }
        assert_eq!(answer, Answer::Replied, "every vector is answered or held: {bytes:02x?}");
        let reply = Message::decode(&lend)
            .unwrap_or_else(|e| panic!("the answer to {bytes:02x?} does not decode: {e:?}"));
        let is_error = matches!(reply.body, Body::Rerror { .. });
        match request {
            Ok(request) => {
                counts.well_formed += 1;
                assert_eq!(reply.tag, request.tag, "the answer to {bytes:02x?} lost its tag");
                // R-messages travel from servers, so one sent to a server is not a request.
                if request.body.kind() % 2 == 1 {
                    assert!(is_error, "an R-message was answered as a request: {bytes:02x?}");
                }
                if !is_error {
                    assert_eq!(
                        reply.body.kind(),
                        request.body.kind() + 1,
                        "the answer to {bytes:02x?} is not its R-message"
                    );
                }
            }
            Err(_) => {
                counts.malformed += 1;
                assert!(is_error, "a malformed request was not refused: {bytes:02x?}");
            }
        }
        if is_error {
            counts.refused += 1;
        } else {
            counts.answered += 1;
        }
    }
    assert_eq!(server.connections(), conns, "a vector minted a connection");
    // A `Tversion` starts a new session, which clunks every fid of the connection (intro(5)).
    let mut lend = vec![0u8; MSIZE];
    let version = Body::Tversion { msize: MSIZE as u32, version: "9P2000" };
    Message { tag: 0xffff, body: version }.encode(&mut lend).unwrap();
    assert_eq!(server.answer_in_place(who, &mut lend), Answer::Replied);
    assert_eq!(server.fids(who), 0, "a vector left a fid behind a new session");
    counts
}
