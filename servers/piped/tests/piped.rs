//! `piped`, the whole program, as a fake process: a session holding the one connection nobody
//! minted makes pipes and mints each stage a connection at one end, and stages in processes of
//! their own read and write those ends across "address spaces", their waiting calls parked.
//!
//! The 9P conformance vectors are in `tests/vectors.rs`.

use std::time::{Duration, Instant};

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::ninep::{COLLECT_WAIT, DMDIR, IN_WORDS, OPENED, collect_words, mode};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::ninep::ErrorName;
use redoubt_rt::wire::ninep::{Body, Message};

#[path = "../src/bin/piped.rs"]
#[allow(dead_code)]
mod piped;

use redoubt_piped::{PIPE_BYTES, ROOT_BADGE};

/// A `piped` up and running, and the session that started it.
struct Running {
    server: usize,
    receive: Handle,
    session: usize,
    /// The session's connection, the root badge's.
    root: Handle,
    thread: std::thread::JoinHandle<u32>,
}

fn start() -> Running {
    let f = fake();
    let server = f.process(0, &[3]);
    let receive = f.endpoint(server);
    let mut builder = StartupBuilder::new(receive.index());
    builder.handle(piped::ENDPOINT, receive).arg("buckets=2");
    let block = builder.finish().expect("the block");
    let thread = f.run(server, move || piped::serve(&Startup::parse(&block).expect("the block parses")));
    let session = f.process(1001, &[3]);
    let root = f.grant(server, receive, session, ROOT_BADGE);
    f.as_process(session, || {
        let c = Connection::new(Endpoint::from_handle(root));
        c.attach(&mut lend(), 0, "").unwrap();
    });
    Running { server, receive, session, root, thread }
}

fn lend() -> Lend { Lend::new(2).unwrap() }

/// Runs `body` as `pid` on a thread of its own, for a call that may wait.
fn on_thread<T: Send + 'static>(
    pid: usize,
    body: impl FnOnce() -> T + Send + 'static,
) -> std::thread::JoinHandle<T> {
    std::thread::spawn(move || fake().as_process(pid, body))
}

/// Waits for a state the server reaches on its own; 60 s is the guard, far beyond any step.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("timed out waiting for {what}");
}

/// A stage: a process of its own, with the session's account, as every budget the session carves
/// has (kernel/budgets.md R8).
struct Stage {
    pid: usize,
    conn: Handle,
    /// The id the session disconnects it by.
    id: u64,
}

impl Running {
    /// The session makes the pipe `name`.
    fn pipe(&self, name: &str) {
        fake().as_process(self.session, || {
            let c = Connection::new(Endpoint::from_handle(self.root));
            let mut l = lend();
            c.walk(&mut l, 0, 1, "").unwrap();
            c.create(&mut l, 1, name, DMDIR | 0o700, mode::OREAD).unwrap();
            c.clunk(&mut l, 1).unwrap();
        });
    }

    /// The session mints a connection at `path` and gives it to a new stage.
    fn stage(&self, path: &str) -> Stage {
        let f = fake();
        let (handle, id) = f.as_process(self.session, || {
            let c = Connection::new(Endpoint::from_handle(self.root));
            let (e, id) = c.new_connection(&mut lend(), path, 0).unwrap();
            (e.handle(), id)
        });
        let pid = f.process(1001, &[3]);
        let conn = f.copy(self.session, handle, pid);
        Stage { pid, conn, id }
    }

    /// The session lets a stage's connection go, as a launcher does at its exit notice.
    fn disconnect(&self, stage: &Stage) {
        fake().as_process(self.session, || {
            Connection::new(Endpoint::from_handle(self.root)).disconnect(stage.id, 1_000_000).unwrap();
        });
    }

    fn shut_down(self) -> u32 {
        fake().destroy(self.server, self.receive);
        self.thread.join().unwrap()
    }
}

impl Stage {
    /// Attaches its connection and opens what it is rooted at with `how`, as fid 0.
    fn open(&self, how: u8) -> Result<(), ClientError> {
        let conn = self.conn;
        fake().as_process(self.pid, || {
            let c = Connection::new(Endpoint::from_handle(conn));
            let mut l = lend();
            c.attach(&mut l, 0, "")?;
            c.open(&mut l, 0, how).map(|_| ())
        })
    }

    fn write(&self, data: &[u8]) -> Result<usize, ClientError> {
        let conn = self.conn;
        fake().as_process(self.pid, || {
            Connection::new(Endpoint::from_handle(conn)).write(&mut lend(), 0, 0, data)
        })
    }

    /// Reads on a thread of its own, since the read may wait.
    fn read(&self, n: usize) -> std::thread::JoinHandle<Result<Vec<u8>, ClientError>> {
        let conn = self.conn;
        on_thread(self.pid, move || {
            let mut out = vec![0; n];
            let got = Connection::new(Endpoint::from_handle(conn)).read(&mut lend(), 0, 0, &mut out)?;
            out.truncate(got);
            Ok(out)
        })
    }
}

#[test]
fn a_pipe_carries_one_stages_bytes_to_the_next_and_ends_when_its_writer_goes() {
    let r = start();
    let f = fake();
    r.pipe("p");
    let writer = r.stage("p/w");
    let reader = r.stage("p/r");
    writer.open(mode::OWRITE).unwrap();
    reader.open(mode::OREAD).unwrap();
    // Nothing written yet: the read waits, parked, and the server goes on serving.
    let reading = reader.read(16);
    wait_until("the read to be parked", || f.open_calls(r.server) == 1);
    assert_eq!(writer.write(b"hello").unwrap(), 5);
    assert_eq!(reading.join().unwrap().unwrap(), b"hello");
    // The writer ends: its launcher lets its connection go, and the reader reads the end.
    let reading = reader.read(16);
    wait_until("the read to be parked", || f.open_calls(r.server) == 1);
    r.disconnect(&writer);
    assert_eq!(reading.join().unwrap().unwrap(), b"");
    assert_eq!(r.shut_down(), redoubt_rt::exit::OK);
}

#[test]
fn a_full_pipe_holds_its_writer_until_the_reader_takes_some() {
    let r = start();
    let f = fake();
    r.pipe("p");
    let writer = r.stage("p/w");
    let reader = r.stage("p/r");
    writer.open(mode::OWRITE).unwrap();
    reader.open(mode::OREAD).unwrap();
    assert_eq!(writer.write(&[1; PIPE_BYTES]).unwrap(), PIPE_BYTES);
    let conn = writer.conn;
    let writing = on_thread(writer.pid, move || {
        Connection::new(Endpoint::from_handle(conn)).write(&mut lend(), 0, 0, b"more")
    });
    wait_until("the write to be parked", || f.open_calls(r.server) == 1);
    assert_eq!(reader.read(100).join().unwrap().unwrap(), vec![1; 100]);
    assert_eq!(writing.join().unwrap(), Ok(4));
    assert_eq!(r.shut_down(), redoubt_rt::exit::OK);
}

#[test]
fn a_writer_whose_reader_has_gone_is_refused_even_while_it_waits() {
    let r = start();
    let f = fake();
    r.pipe("p");
    let writer = r.stage("p/w");
    let reader = r.stage("p/r");
    writer.open(mode::OWRITE).unwrap();
    assert_eq!(writer.write(&[1; PIPE_BYTES]).unwrap(), PIPE_BYTES);
    let conn = writer.conn;
    let writing = on_thread(writer.pid, move || {
        Connection::new(Endpoint::from_handle(conn)).write(&mut lend(), 0, 0, b"more")
    });
    wait_until("the write to be parked", || f.open_calls(r.server) == 1);
    // The reader ends without reading: the waiting write, and every later one, is refused.
    r.disconnect(&reader);
    assert_eq!(writing.join().unwrap(), Err(ClientError::Rerror(ErrorName::State)));
    assert_eq!(writer.write(b"x"), Err(ClientError::Rerror(ErrorName::State)));
    assert_eq!(r.shut_down(), redoubt_rt::exit::OK);
}

/// What a stage holds reaches its own end and nothing else: not the other end, not another pipe,
/// not the root, by a walk, an open the wrong way, a new connection or an attach.
#[test]
fn a_stage_reaches_only_the_end_it_was_given() {
    let r = start();
    let f = fake();
    r.pipe("p");
    r.pipe("canary");
    let stage = r.stage("p/r");
    let conn = stage.conn;
    f.as_process(stage.pid, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut l = lend();
        // Whatever name it attaches with, it attaches at its own root: the read end.
        c.attach(&mut l, 0, "canary").unwrap();
        let qid = c.walk(&mut l, 0, 1, "..").unwrap();
        assert_eq!(qid.kind, 0, ".. from the end is the end itself");
        c.clunk(&mut l, 1).unwrap();
        assert!(c.walk(&mut l, 0, 1, "w").is_err(), "walked from a file");
        assert!(c.walk(&mut l, 0, 1, "../w").is_err(), "climbed above its root");
        assert_eq!(c.open(&mut l, 0, mode::OWRITE), Err(ClientError::Rerror(ErrorName::NotPermitted)));
        assert!(c.create(&mut l, 0, "x", DMDIR, mode::OREAD).is_err(), "made something");
        // `..` and `/` clean to its own root, which it may mint again: the same end, no more.
        for root in ["../w", "../../canary/r", "/canary/w", "w", "x"] {
            assert_eq!(c.new_connection(&mut l, root, 0).err(), Some(ClientError::Remote), "{root}");
        }
    });
    assert_eq!(r.shut_down(), redoubt_rt::exit::OK);
}

/// Only the session's own badge attaches, makes or removes; any other root badge reaches nothing.
#[test]
fn only_the_sessions_own_badge_makes_and_removes_pipes() {
    let r = start();
    let f = fake();
    r.pipe("p");
    let other = f.process(1001, &[3]);
    let handle = f.grant(r.server, r.receive, other, ROOT_BADGE + 1);
    f.as_process(other, || {
        let c = Connection::new(Endpoint::from_handle(handle));
        assert_eq!(c.attach(&mut lend(), 0, ""), Err(ClientError::Rerror(ErrorName::NotPermitted)));
        assert_eq!(c.new_connection(&mut lend(), "p/w", 0).err(), Some(ClientError::Remote));
    });
    f.as_process(r.session, || {
        let c = Connection::new(Endpoint::from_handle(r.root));
        let mut l = lend();
        c.walk(&mut l, 0, 1, "p").unwrap();
        c.remove(&mut l, 1).unwrap();
        assert_eq!(c.walk(&mut l, 0, 1, "p").err(), Some(ClientError::Rerror(ErrorName::NotFound)));
    });
    assert_eq!(r.shut_down(), redoubt_rt::exit::OK);
}

/// The session reads as the VM does, through its hub: a multiplexed read with nothing to read waits
/// in the skeleton, and is answered into the parked completion call when a stage writes.
#[test]
fn a_multiplexed_read_of_the_sessions_waits_for_a_stage_to_write() {
    let r = start();
    let f = fake();
    r.pipe("p");
    let writer = r.stage("p/w");
    writer.open(mode::OWRITE).unwrap();
    let root = r.root;
    let collect = move |hold| {
        let call = Endpoint::from_handle(root).call(
            &collect_words(hold),
            &[],
            Some(Buffer::new(1).unwrap()),
            FOREVER,
        );
        let (reply, lend) = call.into_result().unwrap();
        let lend = lend.unwrap();
        (reply.words, lend[..reply.words[1] as usize].to_vec())
    };
    f.as_process(r.session, || {
        let c = Connection::new(Endpoint::from_handle(root));
        let mut l = lend();
        c.walk(&mut l, 0, 1, "p/r").unwrap();
        c.open(&mut l, 1, mode::OREAD).unwrap();
        assert_eq!(collect(0).0, [0, 0, OPENED, 0], "the session opens");
        let mut bytes = [0u8; IN_WORDS];
        let n =
            Message { tag: 7, body: Body::Tread { fid: 1, offset: 0, count: 8 } }.encode(&mut bytes).unwrap();
        assert!(n <= IN_WORDS);
        let mut words = [0u64; 4];
        for (word, chunk) in words[1..].iter_mut().zip(bytes.chunks(8)) {
            *word = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        Endpoint::from_handle(root).send(&words, &[], None, FOREVER).unwrap();
    });
    let reading = on_thread(r.session, move || {
        let (words, bytes) = collect(COLLECT_WAIT);
        words[0] == 0
            && matches!(
                Message::decode(&bytes).map(|m| (m.tag, m.body)),
                Ok((7, Body::Rread { data: b"pipe" }))
            )
    });
    wait_until("the completion call to be parked", || f.open_calls(r.server) == 1);
    assert_eq!(writer.write(b"pipe").unwrap(), 4);
    assert!(reading.join().unwrap(), "the waiting read was answered with what the stage wrote");
    assert_eq!(r.shut_down(), redoubt_rt::exit::OK);
}
