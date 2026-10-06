//! `erofsd`, the whole program, as a fake process on the runtime's fake kernel: its range is a
//! fake `blkd` serving sectors from memory over the real protocol, its startup block is written
//! as a launcher would, and clients reach it through the client library, unchanged.
//!
//! The rules that need no kernel are in `src/server_tests.rs`.

use std::sync::{Arc, Mutex};

use erofs::{Entry, pack};
use redoubt_client::file::Connection;
use redoubt_client::{Error, Lend};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Words};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    self, ErrorCode as BlkdError, InfoReply, Message as BlkdMessage, ReadReply,
};

#[path = "../src/bin/erofsd.rs"]
mod program;

const SECTOR: usize = 512;

/// The sectors behind the fake `blkd`, shared with the test, and what was asked of them.
#[derive(Default)]
struct Disk {
    bytes: Vec<u8>,
    failing: bool,
    /// `read` calls, and the most sectors one asked for.
    reads: usize,
    most: u32,
    writes: usize,
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

/// A read-only range in memory behind `blkd`'s protocol, at most 64 sectors a read as `blkd`'s;
/// a failing disk answers `failed`.
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
        match request {
            BlkdMessage::Info(_) => Ok(Answer::new(blkd::Reply::Info(InfoReply {
                sectors: (disk.bytes.len() / SECTOR) as u64,
                sector_size: SECTOR as u32,
                read_only: 1,
            }))),
            BlkdMessage::Read(r) if r.count == 0 || r.count > 64 => Err(BlkdError::TooMany),
            BlkdMessage::Read(r) => {
                let start = r.sector as usize * SECTOR;
                let end = start + r.count as usize * SECTOR;
                if end > disk.bytes.len() {
                    return Err(BlkdError::OutOfRange);
                }
                disk.reads += 1;
                disk.most = disk.most.max(r.count);
                self.out = disk.bytes[start..end].to_vec();
                drop(disk);
                Ok(Answer::new(blkd::Reply::Read(ReadReply { data: &self.out })))
            }
            BlkdMessage::Write(_) | BlkdMessage::Flush(_) => {
                disk.writes += 1;
                Err(BlkdError::NotPermitted)
            }
        }
    }
}

/// A running `erofsd` on a fake `blkd`, and `init`, which holds the founding connection.
struct Volume {
    disk: Shared,
    erofsd: usize,
    receive: Handle,
    init: usize,
    founding: Handle,
    blkd: (usize, Handle),
    thread: std::thread::JoinHandle<u32>,
}

const ENDPOINT: &str = "erofsd:system";

impl Volume {
    /// `erofsd` receiving on `erofsd:system` with exactly `args`, on a range holding `bytes`;
    /// with `volume` false, no range handle at all.
    fn start(mut bytes: Vec<u8>, args: &[&str], volume: bool) -> Volume {
        bytes.resize(bytes.len().next_multiple_of(SECTOR), 0);
        let f = fake();
        let disk: Shared = Arc::new(Mutex::new(Disk { bytes, ..Disk::default() }));
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
        let erofsd = f.process(0, &[]);
        let receive = f.endpoint(erofsd);
        let range = f.grant(blkd, blkd_receive, erofsd, 1);
        let mut builder = StartupBuilder::new(receive.index().max(range.index()));
        builder.handle(ENDPOINT, receive);
        if volume {
            builder.handle("volume", range);
        }
        for arg in args {
            builder.arg(arg);
        }
        let block = builder.finish().expect("the block");
        let thread =
            f.run(erofsd, move || program::serve(&Startup::parse(&block).expect("the block parses")));
        let init = f.process(0, &[]);
        let founding = f.grant(erofsd, receive, init, 1);
        Volume { disk, erofsd, receive, init, founding, blkd: (blkd, blkd_receive), thread }
    }

    fn serving(bytes: Vec<u8>) -> Volume {
        Volume::start(bytes, &["endpoint=erofsd:system", "buckets=4"], true)
    }

    /// A fresh connection for a session of `account`, minted by `init` at `root`.
    fn session(&self, account: u64, root: &str) -> (usize, Handle) {
        let f = fake();
        let (conn, _) = f.as_process(self.init, || {
            let mut lend = Lend::new(4).unwrap();
            Connection::attach(Endpoint::from_handle(self.founding), &mut lend)
                .unwrap()
                .new_connection(&mut lend, root, 0)
                .expect("a connection for the session")
        });
        let session = f.process(account, &[]);
        (session, f.copy(self.init, conn.handle(), session))
    }

    /// Stops `erofsd` once it is serving, then `blkd`.
    fn stop(self) -> u32 {
        let f = fake();
        let nine = redoubt_rt::client::Connection::new(Endpoint::from_handle(self.founding));
        f.as_process(self.init, || nine.version(&mut Lend::new(1).unwrap()).expect("erofsd is serving"));
        f.destroy(self.erofsd, self.receive);
        let code = self.thread.join().unwrap();
        f.destroy(self.blkd.0, self.blkd.1);
        code
    }
}

fn counting(len: usize) -> Vec<u8> { (0..len).map(|i| (i % 251) as u8).collect() }

fn system() -> Vec<u8> {
    let big = counting(100_000);
    let tail = counting(5000);
    let tree = [
        Entry::Dir("lib"),
        Entry::File("lib/Elixir.Redoubt.Shell.beam", &big),
        Entry::File("lib/tail", &tail),
        Entry::File("motd", b"hello\n"),
    ];
    pack(&tree, |_| [0; 32]).unwrap()
}

/// The client library, unchanged, against the real `erofsd` on a `blkd` range: a module read
/// whole in reads as large as the lend allows, each one `blkd` call of at most 64 sectors from
/// any offset; a listing; every write refused before `blkd` sees it.
#[test]
fn the_client_library_reads_the_volume_through_blkd() {
    let volume = Volume::serving(system());
    let (session, conn) = volume.session(1001, "");
    let disk = volume.disk.clone();
    fake().as_process(session, || {
        let mut lend = Lend::new(16).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let file = c.open(&mut lend, "lib/Elixir.Redoubt.Shell.beam", mode::OREAD).unwrap();
        assert_eq!(file.stat(&mut lend).unwrap().length, 100_000);
        let want = counting(100_000);
        for start in [0, 1000, 4095, 4097] {
            let reads = disk.lock().unwrap().reads;
            let (mut out, mut offset, mut nine) = (vec![0u8; 100_000], start, 0);
            while offset < 100_000 {
                let n = file.read_at(&mut lend, offset as u64, &mut out[offset..]).unwrap();
                assert!(n > 0);
                (offset, nine) = (offset + n, nine + 1);
            }
            assert_eq!(out[start..], want[start..], "from {start}");
            let calls = disk.lock().unwrap().reads - reads;
            // Each 9P read is one call for every 32 KiB it touches, from wherever it starts.
            let per_read = (lend.iounit() + 511).div_ceil(32 * 1024);
            assert!(calls <= nine * per_read, "{calls} calls for {nine} reads from {start}");
        }
        assert!(disk.lock().unwrap().most <= 64);
        let tail = c.open(&mut lend, "lib/tail", mode::OREAD).unwrap();
        let mut out = vec![0u8; 8192];
        assert_eq!(tail.read_at(&mut lend, 0, &mut out).unwrap(), 5000);
        assert_eq!(out[..5000], counting(5000));
        let root = c.open(&mut lend, "", mode::OREAD).unwrap();
        let (entries, _) = root.read_dir(&mut lend, 0).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["lib", "motd"]);
        assert!(matches!(c.open(&mut lend, "motd", mode::OWRITE), Err(Error::Rerror(_))));
        assert!(matches!(c.create(&mut lend, "", "new", 0o644, mode::OWRITE), Err(Error::Rerror(_))));
        assert!(matches!(c.remove(&mut lend, "motd"), Err(Error::Rerror(_))));
    });
    assert_eq!(disk.lock().unwrap().writes, 0, "erofsd sends no write");
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// A minted connection is rooted where `init` chose.
#[test]
fn a_session_rooted_below_the_root_sees_only_there() {
    let volume = Volume::serving(system());
    let (session, conn) = volume.session(1001, "lib");
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        assert!(c.open(&mut lend, "tail", mode::OREAD).is_ok());
        assert!(c.open(&mut lend, "motd", mode::OREAD).is_err());
        assert!(c.open(&mut lend, "../motd", mode::OREAD).is_err());
    });
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// Noise on the range, or a range that fails: `erofsd` stays up and answers every attach
/// `corrupt`, rather than exiting into a restart loop.
#[test]
fn a_volume_of_noise_or_a_failing_range_is_served_as_corrupt() {
    let noise: Vec<u8> = (0..64 * SECTOR).map(|i| (i * 7 + i / 513) as u8).collect();
    let volume = Volume::serving(noise);
    let f = fake();
    let init = volume.init;
    let attach = |founding| {
        f.as_process(init, || {
            let mut lend = Lend::new(1).unwrap();
            Connection::attach(Endpoint::from_handle(founding), &mut lend).map(|_| ())
        })
    };
    assert!(matches!(attach(volume.founding), Err(Error::Rerror(_))));
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);

    let volume = Volume::serving(system());
    let (session, conn) = volume.session(1001, "");
    let disk = volume.disk.clone();
    fake().as_process(session, || {
        let mut lend = Lend::new(4).unwrap();
        let c = Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap();
        let file = c.open(&mut lend, "motd", mode::OREAD).unwrap();
        disk.lock().unwrap().failing = true;
        let mut out = [0u8; 16];
        assert!(file.read_at(&mut lend, 0, &mut out).is_err());
        disk.lock().unwrap().failing = false;
        assert!(file.read_at(&mut lend, 0, &mut out).is_err(), "corrupt until erofsd starts again");
    });
    assert_eq!(volume.stop(), redoubt_rt::exit::OK);
}

/// Arguments it does not understand, or no range, stop it before it serves.
#[test]
fn arguments_it_does_not_understand_or_no_range_stop_it_before_serving() {
    for args in [
        &[][..],
        &["endpoint=erofsd:system"],
        &["buckets=4"],
        &["endpoint=erofsd:other", "buckets=4"],
        &["endpoint=erofsd:system", "buckets=4", "labels=07"],
        &["endpoint=erofsd:system", "buckets=4", "readonly"],
        &["endpoint=erofsd:system", "buckets=999"],
    ] {
        let volume = Volume::start(system(), args, true);
        assert_eq!(volume.thread.join().unwrap(), program::BAD_ARGS, "{args:?}");
    }
    let volume = Volume::start(system(), &["endpoint=erofsd:system", "buckets=4"], false);
    assert_eq!(volume.thread.join().unwrap(), program::NO_VOLUME);
}
