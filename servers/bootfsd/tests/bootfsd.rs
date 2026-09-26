//! `bootfsd`, the whole program, as a fake process: it gets a startup block written with
//! `StartupBuilder`, exactly as `init` would, and runs its entry function against the runtime's
//! fake kernel. Then `init` fills `/boot` over the real IPC path and clients — one honest, one
//! hostile — read it back over 9P.
//!
//! The rules that need no kernel are in `src/server_tests.rs`; the 9P conformance vectors are in
//! `tests/vectors.rs`.

#[path = "../../../libs/rt/tests/common/mod.rs"]
mod common;

use common::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::bootfs::{Add, ErrorCode, Message, Reply, Seal};

#[path = "../src/bin/bootfsd.rs"]
mod bootfsd;

/// The entries the milestone 1 manifest would mark public, and one name it must never mark.
const PUBLIC: [&str; 3] = ["keyd", "beamlet", "iex.beam"];
const MANIFEST: &str = "manifest.json";

/// Runs `main` as `pid` with a startup block naming `handles`.
fn launch(pid: usize, block: Vec<u8>, main: fn(&Startup) -> u32) -> std::thread::JoinHandle<u32> {
    fake().run(pid, move || {
        let startup = Startup::parse(&block).expect("the launcher's block parses");
        main(&startup)
    })
}

/// The block `init` writes for `bootfsd`: the endpoint it receives on, and the `public` list.
fn block(receive: Handle, public: &[&str]) -> Vec<u8> {
    let mut builder = StartupBuilder::new(receive.index());
    builder.handle("bootfsd", receive);
    for name in public {
        builder.arg(name);
    }
    builder.finish().expect("the block")
}

/// A typed call on `bootfsd`'s 9P endpoint: `add` (buffer-shaped, so it travels in a lend) or
/// `seal` (inline, so it must travel with no buffer at all: servers/wire.md's two shapes).
fn setup(conn: Handle, message: Message<'_>) -> Result<(), ErrorCode> {
    let mut buf = Buffer::new(16).expect("a lend");
    let (opcode, inline) = match message {
        Message::Add(_) => (16, false),
        Message::Seal(_) => (17, true),
    };
    let words = message.encode(&mut buf).expect("the request encodes");
    let lend = (!inline).then_some(buf);
    let (reply, returned) =
        Endpoint::from_handle(conn).call(&words, &[], lend, FOREVER).into_result().expect("the call");
    if reply.words == MALFORMED {
        return Err(ErrorCode::Malformed);
    }
    let body: &[u8] = returned.as_deref().unwrap_or(&[]);
    match Reply::decode(opcode, &reply.words, body, 0) {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(code)) => Err(code),
        Err(e) => panic!("the reply does not decode: {e:?}"),
    }
}

fn add(conn: Handle, name: &str, offset: u64, data: &[u8]) -> Result<(), ErrorCode> {
    setup(conn, Message::Add(Add { name, offset, data }))
}

/// A whole `/boot`, filled and sealed by `init` over IPC, with one long entry that needs two
/// messages. Returns the bytes each entry should read back as.
fn fill(conn: Handle) -> Vec<(&'static str, Vec<u8>)> {
    let long: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    let entries: Vec<(&str, Vec<u8>)> =
        vec![("keyd", b"\x7fELF keyd".to_vec()), ("beamlet", long), ("iex.beam", b"FOR1".to_vec())];
    for (name, data) in &entries {
        let mut offset = 0;
        for chunk in data.chunks(4096) {
            add(conn, name, offset, chunk).expect("add");
            offset += chunk.len() as u64;
        }
    }
    setup(conn, Message::Seal(Seal {})).expect("seal");
    entries
}

/// `init` fills `/boot`; a session walks it, reads every entry byte for byte, lists the
/// directory, and finds nothing else — the manifest's own name included.
#[test]
fn a_session_reads_the_public_entries_and_sees_nothing_else() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let init = f.process(0, &[]);
    let founding = f.grant(server, receive, init, 1);
    let server_thread = launch(server, block(receive, &PUBLIC), bootfsd::serve);

    let entries = f.as_process(init, || fill(founding));

    // A session gets a fresh connection, as a launcher always does (servers/init.md,
    // "Fresh connections per child").
    let session = f.process(1001, &[]);
    let (conn, id) = f
        .as_process(init, || Client::new(Endpoint::from_handle(founding), 4).unwrap().new_connection("", 0))
        .expect("a connection for the session");
    let conn = f.copy(init, conn.handle(), session);
    assert!(id != 0, "a connection id is random, never a counter");

    f.as_process(session, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 16).unwrap();
        assert_eq!(c.version().unwrap(), redoubt_rt::wire::MSIZE as u32);
        c.attach(0, "").unwrap();
        for (i, (name, data)) in entries.iter().enumerate() {
            let fid = 10 + i as u32;
            c.walk(0, fid, name).unwrap_or_else(|e| panic!("walk {name}: {e:?}"));
            c.open(fid, mode::OREAD).unwrap();
            let mut got = vec![0u8; data.len()];
            let mut at = 0;
            while at < data.len() {
                let n = c.read(fid, at as u64, &mut got[at..]).unwrap();
                assert!(n > 0, "{name} stopped short at {at}");
                at += n;
            }
            assert_eq!(&got, data, "{name} did not read back byte for byte");
            // Past the end: nothing, not an error.
            assert_eq!(c.read(fid, data.len() as u64, &mut got).unwrap(), 0);
            c.clunk(fid).unwrap();
        }
        // The attack case: the manifest's own name is refused exactly as a name the bundle
        // never held.
        let never = c.walk(0, 30, "no-such-entry").unwrap_err();
        assert_eq!(c.walk(0, 30, MANIFEST).unwrap_err(), never);
        assert_eq!(c.walk(0, 30, "kernel").unwrap_err(), never);
        assert_eq!(never, ClientError::Remote);
        // And the directory lists exactly the public list, in the manifest's order.
        assert_eq!(names(&mut c, 0), PUBLIC.map(String::from).to_vec());
    });

    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), redoubt_rt::exit::OK);
}

/// The names a directory read of `fid` lists.
fn names(c: &mut Client, fid: u32) -> Vec<String> {
    c.open(fid, mode::OREAD).unwrap();
    let mut out = Vec::new();
    let mut offset = 0u64;
    loop {
        let mut buf = vec![0u8; 8192];
        let n = c.read(fid, offset, &mut buf).unwrap();
        if n == 0 {
            return out;
        }
        offset += n as u64;
        let mut r = redoubt_rt::wire::codec::Reader::new(&buf[..n]);
        while !r.rest().is_empty() {
            let stat = redoubt_rt::wire::ninep::Stat::read_entry(&mut r).expect("a stat entry");
            out.push(stat.name.to_string());
        }
    }
}

/// A client cannot fill or refill `/boot`: `add` and `seal` are refused from every connection
/// `new_connection` minted, and refused from anyone at all once sealed.
#[test]
fn a_client_cannot_publish_into_boot() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let init = f.process(0, &[]);
    let founding = f.grant(server, receive, init, 1);
    let server_thread = launch(server, block(receive, &PUBLIC), bootfsd::serve);

    f.as_process(init, || {
        add(founding, "keyd", 0, b"good").expect("init may add");
    });
    let session = f.process(1001, &[]);
    let (conn, _) = f
        .as_process(init, || Client::new(Endpoint::from_handle(founding), 4).unwrap().new_connection("", 0))
        .unwrap();
    let conn = f.copy(init, conn.handle(), session);
    f.as_process(session, || {
        // Before the seal, and with a connection of its own: still refused.
        assert_eq!(add(conn, "keyd", 4, b"evil"), Err(ErrorCode::Refused));
        assert_eq!(add(conn, MANIFEST, 0, b"evil"), Err(ErrorCode::Refused));
        assert_eq!(setup(conn, Message::Seal(Seal {})), Err(ErrorCode::Refused));
    });
    f.as_process(init, || {
        setup(founding, Message::Seal(Seal {})).expect("init may seal");
        // Sealed for good, even for init.
        assert_eq!(add(founding, "keyd", 4, b"more"), Err(ErrorCode::Refused));
        assert_eq!(setup(founding, Message::Seal(Seal {})), Err(ErrorCode::Refused));
    });
    f.as_process(session, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 16).unwrap();
        c.attach(0, "").unwrap();
        c.walk(0, 1, "keyd").unwrap();
        c.open(1, mode::OREAD).unwrap();
        let mut got = [0u8; 16];
        let n = c.read(1, 0, &mut got).unwrap();
        assert_eq!(&got[..n], b"good", "the client's bytes never reached /boot");
        // Nor can it write through 9P.
        assert_eq!(c.open(2, mode::OWRITE).unwrap_err(), ClientError::Remote);
        c.walk(0, 3, "keyd").unwrap();
        assert_eq!(c.open(3, mode::OWRITE).unwrap_err(), ClientError::Remote);
        assert_eq!(c.open(3, mode::ORDWR).unwrap_err(), ClientError::Remote);
        c.open(3, mode::OREAD).unwrap();
        assert_eq!(c.write(3, 0, b"evil").unwrap_err(), ClientError::Remote);
    });

    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), redoubt_rt::exit::OK);
}

/// A `public` list `bootfsd` cannot serve stops it starting, rather than serving a `/boot` that
/// is not what the manifest named.
#[test]
fn a_bad_public_list_stops_the_server() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let thread = launch(server, block(receive, &["keyd", "keyd"]), bootfsd::serve);
    assert_eq!(thread.join().unwrap(), bootfsd::BAD_PUBLIC_LIST);

    // And a block with no endpoint at all.
    let empty = StartupBuilder::new(0).finish().unwrap();
    let thread = launch(f.process(0, &[]), empty, bootfsd::serve);
    assert_eq!(thread.join().unwrap(), bootfsd::NO_ENDPOINT);
}
