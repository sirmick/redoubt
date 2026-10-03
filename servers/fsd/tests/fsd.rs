//! `fsd`, the whole program, as a fake process on the runtime's fake kernel: its range is a
//! fake `blkd` serving sectors from memory over the real protocol, its startup block is written
//! as a launcher would, and clients reach it through the client library, unchanged, as API1's
//! tests reached in-test servers.
//!
//! The rules that need no kernel are in `src/server_tests.rs` and `src/typed_tests.rs`.

use std::sync::{Arc, Mutex};

use redoubt_client::file::Connection;
use redoubt_client::{Error, Lend, fsd};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Words};
use redoubt_rt::server::ninep::{DMDIR, mode};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    self, ErrorCode as BlkdError, InfoReply, Message as BlkdMessage, ReadReply,
};
use redoubt_rt::wire::proto::fsd::ErrorCode;

#[path = "../src/bin/fsd.rs"]
mod program;

const SECTOR: usize = 512;

/// The sectors behind the fake `blkd`, shared with the test.
#[derive(Default)]
struct Disk {
    bytes: Vec<u8>,
    failing: bool,
    /// `info` says so, and every write is refused, as `blkd` does for a read-only range.
    read_only: bool,
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

/// A range in memory behind `blkd`'s protocol; a failing disk answers `failed`.
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
        if disk.failing {
            return Err(BlkdError::Failed);
        }
        let span = |sector: u64, len: usize, disk: &Disk| {
            let start = sector as usize * SECTOR;
            (start + len <= disk.bytes.len()).then_some(start..start + len).ok_or(BlkdError::OutOfRange)
        };
        let reply = match request {
            BlkdMessage::Info(_) => blkd::Reply::Info(InfoReply {
                sectors: (disk.bytes.len() / SECTOR) as u64,
                sector_size: SECTOR as u32,
                read_only: u32::from(disk.read_only),
            }),
            BlkdMessage::Read(r) => {
                let span = span(r.sector, r.count as usize * SECTOR, &disk)?;
                self.out = disk.bytes[span].to_vec();
                drop(disk);
                return Ok(Answer::new(blkd::Reply::Read(ReadReply { data: &self.out })));
            }
            BlkdMessage::Write(_) if disk.read_only => return Err(BlkdError::NotPermitted),
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

/// A running `fsd` on a fake `blkd`, and `init`, which holds the founding connection.
struct Volume {
    disk: Shared,
    fsd: usize,
    receive: Handle,
    init: usize,
    founding: Handle,
    blkd: (usize, Handle),
    thread: std::thread::JoinHandle<u32>,
}

impl Volume {
    /// `fsd` started with `args` on a range holding `bytes`.
    fn start(bytes: Vec<u8>, args: &[&str]) -> Volume { Volume::start_with(bytes, args, false) }

    /// `fsd` on a read-only range holding `bytes`.
    fn start_read_only(bytes: Vec<u8>) -> Volume { Volume::start_with(bytes, &["buckets=4"], true) }

    fn start_with(bytes: Vec<u8>, args: &[&str], read_only: bool) -> Volume {
        let f = fake();
        let disk: Shared = Arc::new(Mutex::new(Disk { bytes, failing: false, read_only }));
        let blkd = f.process(0, &[]);
        let blkd_receive = f.endpoint(blkd);
        let shared = disk.clone();
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
        let fsd = f.process(0, &[]);
        let receive = f.endpoint(fsd);
        let volume = f.grant(blkd, blkd_receive, fsd, 1);
        let mut builder = StartupBuilder::new(receive.index().max(volume.index()));
        builder.handle("fsd", receive).handle("volume", volume);
        for arg in args {
            builder.arg(arg);
        }
        let block = builder.finish().expect("the block");
        let thread = f.run(fsd, move || program::serve(&Startup::parse(&block).expect("the block parses")));
        let init = f.process(0, &[]);
        let founding = f.grant(fsd, receive, init, 1);
        Volume { disk, fsd, receive, init, founding, blkd: (blkd, blkd_receive), thread }
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

    /// Stops `fsd` once it is serving (a `Tversion` answered: mounting is over), then `blkd`.
    fn stop(self) -> u32 {
        let f = fake();
        let nine = redoubt_rt::client::Connection::new(Endpoint::from_handle(self.founding));
        f.as_process(self.init, || nine.version(&mut Lend::new(1).unwrap()).expect("fsd is serving"));
        f.destroy(self.fsd, self.receive);
        let code = self.thread.join().unwrap();
        f.destroy(self.blkd.0, self.blkd.1);
        code
    }
}

/// The client library, unchanged, against the real `fsd`: files over 9P and every typed
/// operation, each answer decoded from the wire.
#[test]
fn the_client_library_works_against_fsd() {
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
        fsd::set_attr(&mut lend, &file, 16, b"blue").unwrap();
        assert_eq!(fsd::get_attr(&mut lend, &file, 16).unwrap(), b"blue");
        assert_eq!(
            fsd::set_attr(&mut lend, &file, 2, b"forged"),
            Err(Error::Server(ErrorCode::Refused.code()))
        );
        assert_eq!(
            fsd::set_attr(&mut lend, &file, 16, &[0; 1023]),
            Err(Error::Server(ErrorCode::TooLarge.code()))
        );
        assert_eq!(fsd::copy_file(&mut lend, &file, &dir, "copy").unwrap(), 12);
        fsd::rename(&mut lend, &root, "notes", &dir, "moved").unwrap();
        assert_eq!(
            fsd::rename(&mut lend, &root, "d", &dir, "self"),
            Err(Error::Server(ErrorCode::Refused.code()))
        );
        // The handle on the renamed file now names nothing at its path.
        assert_eq!(file.read_at(&mut lend, 0, &mut out), Err(Error::Rerror));
        assert_eq!(fsd::get_attr(&mut lend, &file, 16), Err(Error::Server(ErrorCode::Removed.code())));
        let moved = c.open(&mut lend, "d/moved", mode::OREAD).unwrap();
        assert_eq!(fsd::get_attr(&mut lend, &moved, 16).unwrap(), b"blue");
        let copy = c.open(&mut lend, "d/copy", mode::OREAD).unwrap();
        assert_eq!(copy.read_at(&mut lend, 0, &mut out).unwrap(), 12);
        c.remove(&mut lend, "d/copy").unwrap();
        assert_eq!(copy.read_at(&mut lend, 0, &mut out), Err(Error::Rerror));
        copy.close(&mut lend).unwrap();
    });
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// Files survive the server: a second `fsd` on the same range finds them.
#[test]
fn files_survive_a_restart() {
    let volume = Volume::blank(512, &["buckets=4"]);
    let (session, conn, _) = volume.session(1001, &[]);
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        c.create(&mut lend, "", "kept", 0o644, mode::OWRITE).unwrap().write_at(&mut lend, 0, b"yes").unwrap();
    });
    let bytes = volume.disk.lock().unwrap().bytes.clone();
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
    let volume = Volume::start(bytes, &["buckets=4"]);
    let (session, conn, _) = volume.session(1001, &[]);
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let mut out = [0u8; 8];
        assert_eq!(
            c.open(&mut lend, "kept", mode::OREAD).unwrap().read_at(&mut lend, 0, &mut out).unwrap(),
            3
        );
        assert_eq!(&out[..3], b"yes");
    });
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// Arguments are parsed strictly: anything `fsd` does not understand stops it before it serves.
#[test]
fn arguments_it_does_not_understand_stop_it_before_serving() {
    for args in [
        &[][..],
        &["buckets=4", "labels="],
        &["buckets=4", "labels=7,7"],
        &["buckets=4", "labels=07"],
        &["buckets=4", "labels=x"],
        &["buckets=4", "labels=1", "labels=2"],
        &["buckets=4", "labels=1,2,3,4,5,6,7,8,9"],
        &["buckets=4", "readonly"],
        &["buckets=4", "block=512"],
    ] {
        let volume = Volume::blank(512, args);
        let thread = volume.thread;
        assert_eq!(thread.join().unwrap(), program::BAD_ARGS, "{args:?}");
    }
    let ok = Volume::blank(512, &["buckets=4", "labels=0,7"]);
    assert_eq!(ok.stop(), redoubt_rt::exit::OK);
}

/// A range of fewer than four blocks is no volume.
#[test]
fn a_range_too_small_is_no_volume() {
    let volume = Volume::blank(4 * 8 - 1, &["buckets=4"]);
    assert_eq!(volume.thread.join().unwrap(), program::NO_VOLUME);
}

/// Noise on the range: `fsd` stays up and answers every attach `corrupt`, rather than exiting
/// into a restart loop.
#[test]
fn a_range_of_noise_is_served_as_corrupt() {
    let noise: Vec<u8> = (0..512 * SECTOR).map(|i| (i * 7 + i / 513) as u8).collect();
    let volume = Volume::start(noise.clone(), &["buckets=4"]);
    let f = fake();
    for _ in 0..3 {
        let refused = f.as_process(volume.init, || {
            Connection::attach(Endpoint::from_handle(volume.founding), &mut Lend::new(1).unwrap()).err()
        });
        assert_eq!(refused, Some(Error::Rerror));
    }
    assert!(volume.disk.lock().unwrap().bytes == noise, "never formatted");
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// `blkd` failing under a mounted volume: `corrupt` from then on.
#[test]
fn a_failing_range_answers_corrupt() {
    let volume = Volume::blank(512, &["buckets=4"]);
    let (session, conn, _) = volume.session(1001, &[]);
    let disk = volume.disk.clone();
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let file = c.create(&mut lend, "", "f", 0o644, mode::ORDWR).unwrap();
        disk.lock().unwrap().failing = true;
        assert_eq!(fsd::set_attr(&mut lend, &file, 16, b"x"), Err(Error::Server(ErrorCode::Corrupt.code())));
        disk.lock().unwrap().failing = false;
        assert_eq!(fsd::get_attr(&mut lend, &file, 16), Err(Error::Server(ErrorCode::Corrupt.code())));
    });
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
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

/// A range `blkd` says is read-only: `fsd` refuses every change before `blkd` sees it, keeps
/// serving reads to every client, and changes not one byte. A blank one is never formatted.
#[test]
fn a_read_only_range_is_served_read_only() {
    let volume = Volume::blank(512, &["buckets=4"]);
    let (session, conn, _) = volume.session(1001, &[]);
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        c.create(&mut lend, "", "kept", 0o644, mode::OWRITE).unwrap().write_at(&mut lend, 0, b"yes").unwrap();
    });
    let bytes = volume.disk.lock().unwrap().bytes.clone();
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);

    let volume = Volume::start_read_only(bytes.clone());
    let (session, conn, _) = volume.session(1001, &[]);
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        assert_eq!(c.open(&mut lend, "kept", mode::OWRITE).err(), Some(Error::Rerror));
        assert_eq!(c.create(&mut lend, "", "new", 0o644, mode::OWRITE).err(), Some(Error::Rerror));
        assert_eq!(c.remove(&mut lend, "kept"), Err(Error::Rerror));
        let file = c.open(&mut lend, "kept", mode::OREAD).unwrap();
        assert_eq!(fsd::set_attr(&mut lend, &file, 16, b"x"), Err(Error::Server(ErrorCode::Refused.code())));
        let mut out = [0u8; 8];
        assert_eq!(file.read_at(&mut lend, 0, &mut out).unwrap(), 3, "nothing refused poisoned the volume");
    });
    assert!(volume.disk.lock().unwrap().bytes == bytes);
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);

    let blank = Volume::start_read_only(vec![0; 512 * SECTOR]);
    let refused = fake().as_process(blank.init, || {
        Connection::attach(Endpoint::from_handle(blank.founding), &mut Lend::new(1).unwrap()).err()
    });
    assert_eq!(refused, Some(Error::Rerror));
    assert!(blank.disk.lock().unwrap().bytes.iter().all(|b| *b == 0), "never formatted");
    assert_eq!(blank.stop(), redoubt_rt::exit::OK);
}
