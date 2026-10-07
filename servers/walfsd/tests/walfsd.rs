//! `walfsd`, the whole program, as a fake process on the runtime's fake kernel: its range is a fake
//! `blkd` serving sectors from memory over the real protocol, its startup block is written as a
//! launcher would, and clients reach it through the client library, unchanged.
//!
//! The rules that need no kernel are in `src/server_tests.rs`, `src/typed_tests.rs` and
//! `src/quota_tests.rs`.

use std::sync::{Arc, Mutex};

use redoubt_client::file::Connection;
use redoubt_client::{Error, Lend, Name, littlefsd};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle, MAX_LABELS};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Words};
use redoubt_rt::server::ninep::{DMDIR, mode};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    self, ErrorCode as BlkdError, InfoReply, Message as BlkdMessage, ReadReply,
};
use redoubt_rt::wire::proto::littlefsd::ErrorCode;

#[path = "../src/bin/walfsd.rs"]
mod program;

const SECTOR: usize = 512;

/// The sectors behind the fake `blkd`, shared with the test.
#[derive(Default)]
struct Disk {
    bytes: Vec<u8>,
}

type Shared = Arc<Mutex<Disk>>;

/// `blkd`'s protocol, for the fake.
struct Blkd;

impl Protocol for Blkd {
    type Error = BlkdError;
    type Reply<'a> = blkd::Reply<'a>;
    type Request<'a> = BlkdMessage<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<BlkdMessage<'a>, WireError> {
        BlkdMessage::decode(words, buf, handles)
    }

    fn encode_reply(reply: &blkd::Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: BlkdError) -> Words { error.encode() }
}

/// A range in memory behind `blkd`'s protocol.
struct FakeBlkd {
    disk: Shared,
    out: Vec<u8>,
}

impl TypedServer<Blkd> for FakeBlkd {
    fn handle<'s>(
        &'s mut self,
        _: &Caller,
        request: BlkdMessage<'_>,
        _: &[Handle],
    ) -> Result<Answer<blkd::Reply<'s>>, BlkdError> {
        let mut disk = self.disk.lock().unwrap();
        let span = |sector: u64, len: usize, disk: &Disk| {
            let start = sector as usize * SECTOR;
            (start + len <= disk.bytes.len()).then_some(start..start + len).ok_or(BlkdError::OutOfRange)
        };
        let reply = match request {
            BlkdMessage::Info(_) => blkd::Reply::Info(InfoReply {
                sectors: (disk.bytes.len() / SECTOR) as u64,
                sector_size: SECTOR as u32,
                read_only: 0,
            }),
            BlkdMessage::Read(r) => {
                let span = span(r.sector, r.count as usize * SECTOR, &disk)?;
                self.out = disk.bytes[span].to_vec();
                drop(disk);
                return Ok(Answer::new(blkd::Reply::Read(ReadReply { data: &self.out })));
            }
            BlkdMessage::Write(w) => {
                let span = span(w.sector, w.data.len(), &disk)?;
                disk.bytes[span].copy_from_slice(w.data);
                blkd::Reply::Write(blkd::WriteReply {})
            }
            BlkdMessage::Flush(_) => blkd::Reply::Flush(blkd::FlushReply {}),
        };
        Ok(Answer::new(reply))
    }
}

/// A running `walfsd` on a fake `blkd`, and `init`, which holds the founding connection.
struct Volume {
    walfsd: usize,
    receive: Handle,
    init: usize,
    founding: Handle,
    blkd: (usize, Handle),
    thread: std::thread::JoinHandle<u32>,
}

/// The endpoint the tests' `walfsd` receives on, and the argument naming it.
const ENDPOINT: &str = "walfsd:data";
const ENDPOINT_ARG: &str = "endpoint=walfsd:data";

impl Volume {
    /// `walfsd` started with `args` on a range holding `bytes`.
    /// Its `endpoint=` argument first, then `args`.
    fn start(bytes: Vec<u8>, args: &[&str]) -> Volume {
        let args: Vec<&str> = [ENDPOINT_ARG].iter().chain(args).copied().collect();
        Volume::start_raw(bytes, &args)
    }

    /// `walfsd` receiving on `walfsd:data`, with exactly `args`.
    fn start_raw(bytes: Vec<u8>, args: &[&str]) -> Volume {
        let f = fake();
        let shared: Shared = Arc::new(Mutex::new(Disk { bytes }));
        let blkd = f.process(0, &[]);
        let blkd_receive = f.endpoint(blkd);
        f.run(blkd, move || {
            let endpoint = Endpoint::from_handle(blkd_receive);
            let mut server = FakeBlkd { disk: shared, out: Vec::new() };
            loop {
                match endpoint.receive(FOREVER, 0) {
                    Ok(Event::Call(request)) => {
                        let _ = serve_call::<Blkd, _>(&mut server, request);
                    }
                    Ok(_) => {}
                    Err(_) => return 0,
                }
            }
        });
        let walfsd = f.process(0, &[]);
        let receive = f.endpoint(walfsd);
        let volume = f.grant(blkd, blkd_receive, walfsd, 1);
        let mut builder = StartupBuilder::new(receive.index().max(volume.index()));
        builder.handle(ENDPOINT, receive).handle("volume", volume);
        for arg in args {
            builder.arg(arg);
        }
        let block = builder.finish().expect("the block");
        let thread =
            f.run(walfsd, move || program::serve(&Startup::parse(&block).expect("the block parses")));
        let init = f.process(0, &[]);
        let founding = f.grant(walfsd, receive, init, 1);
        Volume { walfsd, receive, init, founding, blkd: (blkd, blkd_receive), thread }
    }

    fn blank(sectors: usize, args: &[&str]) -> Volume { Volume::start(vec![0; sectors * SECTOR], args) }

    /// A fresh connection for a session of `account` labelled `labels`, minted by `init`.
    fn session(&self, account: u64, labels: &[u64]) -> (usize, Handle, u64) {
        let f = fake();
        let (conn, id) = f.as_process(self.init, || {
            let mut lend = Lend::new(4).unwrap();
            Connection::attach(Endpoint::from_handle(self.founding), &mut lend)
                .unwrap()
                .new_connection(&mut lend, "", 0)
                .expect("a connection for the session")
        });
        let session = f.process(account, labels);
        (session, f.copy(self.init, conn.handle(), session), id)
    }

    /// Stops `walfsd` once it is serving (a `Tversion` answered: mounting is over), then `blkd`.
    fn stop(self) -> u32 {
        let f = fake();
        let nine = redoubt_rt::client::Connection::new(Endpoint::from_handle(self.founding));
        f.as_process(self.init, || nine.version(&mut Lend::new(1).unwrap()).expect("walfsd is serving"));
        f.destroy(self.walfsd, self.receive);
        let code = self.thread.join().unwrap();
        f.destroy(self.blkd.0, self.blkd.1);
        code
    }
}

/// The client library, unchanged, against the real `walfsd`: files over 9P and every typed
/// operation of `littlefsd`'s protocol, each answer decoded from the wire.
#[test]
fn the_client_library_works_against_walfsd() {
    let volume = Volume::blank(1024, &["buckets=4"]);
    let (session, conn, _) = volume.session(1001, &[]);
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let file = c.create(&mut lend, "", "notes", 0o644, mode::ORDWR).unwrap();
        assert_eq!(file.write_at(&mut lend, 0, b"hello, world").unwrap(), 12);
        let mut out = [0u8; 32];
        assert_eq!(file.read_at(&mut lend, 0, &mut out).unwrap(), 12);
        assert_eq!(&out[..12], b"hello, world");
        assert_eq!(file.stat(&mut lend).unwrap().length, 12);
        c.create(&mut lend, "", "d", DMDIR | 0o755, mode::OREAD).unwrap().close(&mut lend).unwrap();

        let root = c.open(&mut lend, "", mode::OREAD).unwrap();
        let dir = c.open(&mut lend, "d", mode::OREAD).unwrap();
        littlefsd::set_attr(&mut lend, &file, 16, b"blue").unwrap();
        assert_eq!(littlefsd::get_attr(&mut lend, &file, 16).unwrap(), b"blue");
        assert_eq!(
            littlefsd::set_attr(&mut lend, &file, 2, b"forged"),
            Err(Error::Server(ErrorCode::NotPermitted.code()))
        );
        assert_eq!(
            littlefsd::set_attr(&mut lend, &file, 16, &[0; 1023]),
            Err(Error::Server(ErrorCode::TooLarge.code()))
        );
        assert_eq!(littlefsd::copy_file(&mut lend, &file, &dir, "copy").unwrap(), 12);
        littlefsd::rename(&mut lend, &root, "notes", &dir, "moved").unwrap();
        assert_eq!(
            littlefsd::rename(&mut lend, &root, "d", &dir, "self"),
            Err(Error::Server(ErrorCode::NotPermitted.code()))
        );
        // The handle on the renamed file now names nothing at its path.
        assert_eq!(file.read_at(&mut lend, 0, &mut out), Err(Error::Rerror(Name::Removed)));
        assert_eq!(littlefsd::get_attr(&mut lend, &file, 16), Err(Error::Server(ErrorCode::Removed.code())));
        let moved = c.open(&mut lend, "d/moved", mode::OREAD).unwrap();
        assert_eq!(littlefsd::get_attr(&mut lend, &moved, 16).unwrap(), b"blue");
        let copy = c.open(&mut lend, "d/copy", mode::OREAD).unwrap();
        assert_eq!(copy.read_at(&mut lend, 0, &mut out).unwrap(), 12);
        c.remove(&mut lend, "d/copy").unwrap();
        assert_eq!(copy.read_at(&mut lend, 0, &mut out), Err(Error::Rerror(Name::Removed)));
        copy.close(&mut lend).unwrap();
    });
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// Arguments are parsed strictly: anything `walfsd` does not understand stops it before it serves.
#[test]
fn arguments_it_does_not_understand_stop_it_before_serving() {
    // One label more than a set holds.
    let ids: Vec<String> = (1..=MAX_LABELS + 1).map(|id| id.to_string()).collect();
    let too_many = format!("labels={}", ids.join(","));
    for args in [
        &[][..],
        &["buckets=4", "labels="],
        &["buckets=4", "labels=7,7"],
        &["buckets=4", "labels=07"],
        &["buckets=4", "labels=x"],
        &["buckets=4", "labels=1", "labels=2"],
        &["buckets=4", too_many.as_str()],
        &["buckets=4", "readonly"],
        &["buckets=4", "block=512"],
    ] {
        let volume = Volume::blank(512, args);
        let thread = volume.thread;
        assert_eq!(thread.join().unwrap(), program::BAD_ARGS, "{args:?}");
    }
    let ok = Volume::blank(512, &["buckets=4", "labels=0,7"]);
    assert_eq!(ok.stop(), redoubt_rt::exit::OK);
    // `endpoint=` is required, given once, a name, and names a handle the block holds.
    for args in [
        &["buckets=4"][..],
        &["endpoint=", "buckets=4"],
        &["endpoint=walfsd:other", "buckets=4"],
        &["endpoint=Walfsd", "buckets=4"],
        &[ENDPOINT_ARG, ENDPOINT_ARG, "buckets=4"],
    ] {
        let volume = Volume::start_raw(vec![0; 512 * SECTOR], args);
        assert_eq!(volume.thread.join().unwrap(), program::BAD_ARGS, "{args:?}");
    }
}

/// A range smaller than walfs's smallest volume is no volume.
#[test]
fn a_range_too_small_is_no_volume() {
    let volume = Volume::blank(40 * 8 - 1, &["buckets=4"]);
    assert_eq!(volume.thread.join().unwrap(), program::NO_VOLUME);
}

/// Admission (servers/serving.md R26): a client's fids are bounded by its share of `buckets`,
/// and `disconnect` gives back every fid of the connection.
#[test]
fn fids_are_bounded_and_disconnect_frees_them() {
    let volume = Volume::blank(512, &["buckets=4"]);
    let f = fake();
    let (session, conn, id) = volume.session(1001, &[]);
    let held = f.as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        c.create(&mut lend, "", "f", 0o644, mode::OWRITE).unwrap().close(&mut lend).unwrap();
        let mut held = Vec::new();
        while let Ok(file) = c.open(&mut lend, "f", mode::OREAD) {
            held.push(file);
            assert!(held.len() < 64, "fids are bounded");
        }
        let n = held.len();
        std::mem::forget(held);
        n
    });
    assert!(held > 0);
    let (other, conn2, _) = volume.session(1001, &[]);
    let opens = |who: usize, conn: Handle| {
        f.as_process(who, || {
            let mut lend = Lend::new(4).unwrap();
            let Ok(c) = Connection::attach(Endpoint::from_handle(conn), &mut lend) else { return 0 };
            let mut n = 0;
            while let Ok(file) = c.open(&mut lend, "f", mode::OREAD) {
                std::mem::forget(file);
                n += 1;
            }
            n
        })
    };
    assert!(opens(other, conn2) > 0, "another connection has a share of its own");
    let (filler, conn_filler, _) = volume.session(1001, &[]);
    opens(filler, conn_filler);
    let (late, conn_late, _) = volume.session(1001, &[]);
    assert_eq!(opens(late, conn_late), 0, "the client's bucket is full");
    f.as_process(volume.init, || {
        let mut lend = Lend::new(1).unwrap();
        Connection::attach(Endpoint::from_handle(volume.founding), &mut lend)
            .unwrap()
            .disconnect(id, FOREVER)
            .unwrap()
    });
    let (third, conn3, _) = volume.session(1001, &[]);
    assert!(opens(third, conn3) > 0, "disconnect gave the fids back");
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}
