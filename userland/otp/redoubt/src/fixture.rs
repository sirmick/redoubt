//! Running the platform on a host, on the fake kernel (`redoubt-fake-kernel`): a console server
//! for it to reach, a home volume, a session process to run it in, and the host's way to find its
//! modules. Host only (the `fake` feature); nothing here runs on the machine.
//!
//! **The home volume is the real `littlefsd`**, the program itself, on a fake `blkd` that serves
//! sectors from memory over `blkd`'s protocol, as `littlefsd`'s own tests run it: its files, its
//! label rule and its refusals are the machine's.
//!
//! **The console server is a fixture, not `consoled`.** `consoled` drives an ns16550, and the
//! fake kernel's device is a page of plain memory: nothing clears "data ready" when a byte is
//! read, or tells a writer when the transmitter has taken one, so the real driver cannot be fed
//! a person's typing through it. This server keeps `consoled`'s protocol exactly (one file,
//! `/dev/cons`, served over 9P, a read with nothing to read parked until input comes, and its
//! admission) with a byte stream for its device: the host's terminal, or a pipe in a test. What
//! runs over it, the platform and the client library, is what runs on Redoubt; `consoled`'s own
//! device handling is its host tests' and the machine's.

use std::collections::VecDeque;
use std::io::{Read as _, Write};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Request, Words};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{
    Around, FileServer, FileStat, NineError, NineServer, Qid, Read, WORDS_9P, mode, refuse, refuse_malformed,
};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    self, ErrorCode as BlkdError, FlushReply, InfoReply, Message as BlkdMessage, ReadReply, WriteReply,
};

use crate::{Modules, Unloaded};

/// The badge of the input thread's wake-ups, as `consoled`'s interrupt thread's are.
const INPUT: u64 = 1;

/// A console server running as a fake process.
pub struct ConsoleServer {
    pub pid: usize,
    /// Its receive endpoint, which a session is granted a connection to.
    pub endpoint: Handle,
    pub thread: JoinHandle<u32>,
    /// When each write reached it (`time_now`, µs), answered or refused: how a test sees a
    /// client's retries spaced.
    pub writes: Arc<Mutex<Vec<u64>>>,
}

/// Starts the console server, with `input` as what is typed and `output` as the screen.
pub fn console(input: Box<dyn std::io::Read + Send>, output: Box<dyn Write + Send>) -> ConsoleServer {
    console_labelled(input, output, &[])
}

/// As [`console`], run with `labels`: the console of a session of that label set, which a
/// labelled process may write.
pub fn console_labelled(
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
    labels: &[u64],
) -> ConsoleServer {
    console_built(input, output, labels, 0)
}

/// As [`console`], answering the first `busy` writes `busy`, as a server over its share does,
/// and taking the rest.
pub fn console_busy(
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
    busy: u32,
) -> ConsoleServer {
    console_built(input, output, &[], busy)
}

fn console_built(
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
    labels: &[u64],
    busy: u32,
) -> ConsoleServer {
    let f = fake();
    let pid = f.process(0, labels);
    let endpoint = f.endpoint(pid);
    let labels = labels.to_vec();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let stamps = Arc::clone(&writes);
    let thread = f.run(pid, move || serve(endpoint, pid, input, output, labels, busy, stamps));
    ConsoleServer { pid, endpoint, thread, writes }
}

/// A new session process, with a connection to `console` bound at `/dev/cons`, and its startup
/// block, as a launcher writes it.
pub fn session(console: &ConsoleServer) -> (usize, Vec<u8>) { session_with(console, &[], &[]) }

/// A new process with `/dev/cons`, a connection to each volume, and `args`, and its startup block:
/// a volume named by a path is bound at that prefix of its namespace, as a session's are; one
/// named otherwise is handed as a handle of that name, as `init` hands one.
pub fn session_with(console: &ConsoleServer, volumes: &[(&str, &Volume)], args: &[&str]) -> (usize, Vec<u8>) {
    session_built(console, volumes, args, &[], |_| Vec::new())
}

/// As [`session_with`], for a process of `labels`, also handed what `extra` makes for it once it
/// exists (a budget, an endpoint of its own, a grant), each under its name.
pub fn session_built(
    console: &ConsoleServer,
    volumes: &[(&str, &Volume)],
    args: &[&str],
    labels: &[u64],
    extra: impl FnOnce(usize) -> Vec<(&'static str, Handle)>,
) -> (usize, Vec<u8>) {
    let f = fake();
    let pid = f.process(1001, labels);
    let cons = f.grant(console.pid, console.endpoint, pid, 0x20 + pid as u64);
    let mut held: Vec<(&str, Handle)> = volumes
        .iter()
        .map(|(name, volume)| (*name, f.grant(volume.littlefsd, volume.endpoint, pid, 0x40 + pid as u64)))
        .collect();
    held.extend(extra(pid));
    let highest = held.iter().map(|(_, h)| h.index()).fold(cons.index(), u32::max);
    let mut builder = StartupBuilder::new(highest);
    builder.namespace("/dev/cons", cons);
    for (name, handle) in held {
        if name.starts_with('/') {
            builder.namespace(name, handle);
        } else {
            builder.handle(name, handle);
        }
    }
    for arg in args {
        builder.arg(arg);
    }
    (pid, builder.finish().expect("a startup block"))
}

/// Modules from host directories, the first that has a file of the name, as beamlet's `-pa`.
pub struct Dirs(pub Vec<PathBuf>);

impl Modules for Dirs {
    fn load(&mut self, file: &str) -> Result<Vec<u8>, Unloaded> {
        if file.is_empty() || file.contains(['/', '\\', '\0']) || file.starts_with('.') {
            return Err(Unloaded::Absent);
        }
        let found = self.0.iter().map(|dir| dir.join(file)).find(|p| p.is_file());
        found.and_then(|p| std::fs::read(p).ok()).ok_or(Unloaded::Absent)
    }
}

/// The file every connection attaches at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cons;

/// What was typed and nobody has read yet, and whether the typing has ended: the device's
/// buffer, filled by the input thread.
#[derive(Default)]
struct Input {
    bytes: VecDeque<u8>,
    ended: bool,
}

/// The console: its input, shared with the input thread, the screen, its file's labels, how many
/// writes it still answers `busy`, and when each write reached it.
struct Stream {
    input: Arc<Mutex<Input>>,
    output: Box<dyn Write + Send>,
    labels: Vec<u64>,
    busy: u32,
    writes: Arc<Mutex<Vec<u64>>>,
}

impl Stream {
    /// Whether a parked read can be answered: there is input, or its end.
    fn has_input(&self) -> bool {
        let input = self.input.lock().expect("the input");
        !input.bytes.is_empty() || input.ended
    }
}

fn qid() -> Qid { Qid { kind: 0, version: 0, path: 0 } }

impl FileServer for Stream {
    type Node = Cons;

    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Cons, Qid), NineError> { Ok((Cons, qid())) }

    fn labels(&self, _: &Cons) -> &[u64] { &self.labels }

    fn walk(&mut self, _: &Caller, _: &Cons, _name: &str) -> Result<(Cons, Qid), NineError> {
        Err(NineError::NOT_DIR)
    }

    fn open(&mut self, _: &Caller, _: &Cons, open_mode: u8) -> Result<Qid, NineError> {
        if open_mode & mode::OTRUNC != 0 || !matches!(open_mode & 3, mode::OREAD | mode::OWRITE | mode::ORDWR)
        {
            return Err(NineError::BAD_MODE);
        }
        Ok(qid())
    }

    /// Input, or a request to hold the call; nothing at all once the input has ended, which a
    /// reader takes as its end.
    fn read(&mut self, _: &Caller, _: &Cons, _offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        if out.is_empty() {
            return Ok(Read::Done(0));
        }
        let mut input = self.input.lock().expect("the input");
        if input.bytes.is_empty() {
            return Ok(if input.ended { Read::Done(0) } else { Read::Wait });
        }
        let n = out.len().min(input.bytes.len());
        for (slot, byte) in out[..n].iter_mut().zip(input.bytes.drain(..n)) {
            *slot = byte;
        }
        Ok(Read::Done(n))
    }

    /// Output; `busy` while the fixture is told to be, as a server over its share answers.
    fn write(&mut self, _: &Caller, _: &Cons, _offset: u64, data: &[u8]) -> Result<usize, NineError> {
        self.writes.lock().expect("the writes").push(redoubt_rt::handle::time_now().unwrap_or(0));
        if self.busy > 0 {
            self.busy -= 1;
            return Err(NineError::BUSY);
        }
        let written = self.output.write_all(data).and_then(|()| self.output.flush());
        Ok(if written.is_ok() { data.len() } else { 0 })
    }

    fn stat(&mut self, _: &Caller, _: &Cons) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: qid(), mode: 0o666, mtime: 0, length: 0, name: String::from("cons") })
    }

    fn dir_entry(&mut self, _: &Caller, _: &Cons, _: u64) -> Result<Option<(Cons, FileStat)>, NineError> {
        Ok(None)
    }
}

/// A file server of one writable file, `out`, under its root, with a share of one page a badge
/// (`consoled`'s page limits): what a write to it brought is kept, and the first `busy` writes
/// are answered `busy`, as a server over its share answers. Bound in a session's namespace, it
/// is a server for the files' own retries.
pub struct SinkServer {
    pub pid: usize,
    pub endpoint: Handle,
    pub thread: JoinHandle<u32>,
    /// When each write reached it (`time_now`, µs), answered or refused.
    pub writes: Arc<Mutex<Vec<u64>>>,
    /// What the writes it took brought, in order.
    pub taken: Arc<Mutex<Vec<u8>>>,
    /// While set, a read of `out` waits (the request stays pending at the server, holding the
    /// share's one request), so a test can make the next request on the connection `busy`.
    pub hold_reads: Arc<AtomicBool>,
    /// When each clunk was served (`time_now`, µs): a fid the server let go of.
    pub clunks: Arc<Mutex<Vec<u64>>>,
}

/// Starts a [`SinkServer`] answering the first `busy` writes `busy`.
pub fn sink(busy: u32) -> SinkServer {
    let f = fake();
    let pid = f.process(0, &[]);
    let endpoint = f.endpoint(pid);
    let writes = Arc::new(Mutex::new(Vec::new()));
    let taken = Arc::new(Mutex::new(Vec::new()));
    let hold_reads = Arc::new(AtomicBool::new(false));
    let clunks = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink {
        busy,
        writes: Arc::clone(&writes),
        taken: Arc::clone(&taken),
        hold_reads: Arc::clone(&hold_reads),
        clunks: Arc::clone(&clunks),
    };
    let thread = f.run(pid, move || {
        // `consoled`'s page share, and a share of one request: a second request outstanding on
        // the connection is `busy`.
        let limits = Limits { buckets: 4, in_flight: 2, files: 8, state: 4, requests: 2, pages: 2 };
        let random = redoubt_rt::handle::random_u64().unwrap_or(1);
        let Ok(mut server) = NineServer::new(sink, limits, random) else { return 1 };
        server.run(&Endpoint::from_handle(endpoint), |_, request| refuse_malformed(request))
    });
    SinkServer { pid, endpoint, thread, writes, taken, hold_reads, clunks }
}

/// The sink's nodes: its root directory, and `out`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Node {
    Root,
    Out,
}

struct Sink {
    busy: u32,
    writes: Arc<Mutex<Vec<u64>>>,
    taken: Arc<Mutex<Vec<u8>>>,
    hold_reads: Arc<AtomicBool>,
    clunks: Arc<Mutex<Vec<u64>>>,
}

fn sink_qid(node: Node) -> Qid {
    match node {
        Node::Root => Qid { kind: 0x80, version: 0, path: 0 },
        Node::Out => Qid { kind: 0, version: 0, path: 1 },
    }
}

impl FileServer for Sink {
    type Node = Node;

    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Node, Qid), NineError> {
        Ok((Node::Root, sink_qid(Node::Root)))
    }

    fn labels(&self, _: &Node) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        match (dir, name) {
            (Node::Root, "out") => Ok((Node::Out, sink_qid(Node::Out))),
            (Node::Root, _) => Err(NineError::NOT_FOUND),
            (Node::Out, _) => Err(NineError::NOT_DIR),
        }
    }

    fn open(&mut self, _: &Caller, node: &Node, _open_mode: u8) -> Result<Qid, NineError> {
        Ok(sink_qid(*node))
    }

    /// Nothing to read: the sink keeps what it takes for the test, not for a reader. While the
    /// test holds reads, a read waits instead.
    fn read(&mut self, _: &Caller, node: &Node, _offset: u64, _out: &mut [u8]) -> Result<Read, NineError> {
        if *node == Node::Out && self.hold_reads.load(Ordering::SeqCst) {
            return Ok(Read::Wait);
        }
        Ok(Read::Done(0))
    }

    fn clunk(&mut self, _: &Node) {
        self.clunks.lock().expect("the clunks").push(redoubt_rt::handle::time_now().unwrap_or(0));
    }

    /// Output; `busy` while the fixture is told to be, as a server over its share answers.
    fn write(&mut self, _: &Caller, node: &Node, _offset: u64, data: &[u8]) -> Result<usize, NineError> {
        if *node != Node::Out {
            return Err(NineError::NOT_DIR);
        }
        self.writes.lock().expect("the writes").push(redoubt_rt::handle::time_now().unwrap_or(0));
        if self.busy > 0 {
            self.busy -= 1;
            return Err(NineError::BUSY);
        }
        self.taken.lock().expect("the bytes").extend_from_slice(data);
        Ok(data.len())
    }

    fn stat(&mut self, _: &Caller, node: &Node) -> Result<FileStat, NineError> {
        let (mode, name) = match node {
            Node::Root => (0o040_755, "/"),
            Node::Out => (0o644, "out"),
        };
        Ok(FileStat { qid: sink_qid(*node), mode, mtime: 0, length: 0, name: String::from(name) })
    }

    fn dir_entry(
        &mut self,
        _: &Caller,
        dir: &Node,
        index: u64,
    ) -> Result<Option<(Node, FileStat)>, NineError> {
        if *dir != Node::Root || index != 0 {
            return Ok(None);
        }
        let stat = FileStat {
            qid: sink_qid(Node::Out),
            mode: 0o644,
            mtime: 0,
            length: 0,
            name: String::from("out"),
        };
        Ok(Some((Node::Out, stat)))
    }
}

/// Serves `/dev/cons` on `endpoint` until the endpoint is destroyed, as `consoled` does: calls
/// and multiplexed sessions on the skeleton's loop, with an input thread in place of its interrupt
/// thread.
fn serve(
    endpoint: Handle,
    pid: usize,
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
    labels: Vec<u64>,
    busy: u32,
    writes: Arc<Mutex<Vec<u64>>>,
) -> u32 {
    let endpoint = Endpoint::from_handle(endpoint);
    // `consoled`'s shares: a page a badge for writes, and requests to spare for a parked read.
    let limits = Limits { buckets: 4, in_flight: 2, files: 4, state: 4, requests: 80, pages: 2 };
    let random = redoubt_rt::handle::random_u64().unwrap_or(1);
    let typed = Arc::new(Mutex::new(Input::default()));
    let stream = Stream { input: Arc::clone(&typed), output, labels, busy, writes };
    let Ok(mut server) = NineServer::new(stream, limits, random) else { return 1 };
    // A console read waits on a person, so it has no deadline.
    server.requests_wait(FOREVER);
    let Ok(wake) = endpoint.mint(NonZeroU64::new(INPUT).expect("non-zero"), None) else { return 2 };
    std::thread::spawn(move || fake().as_process(pid, move || feed(input, &typed, &wake)));
    server.run_around(&endpoint, Readers(Parked::new(FOREVER)))
}

/// The input thread: reads the device into the console's input, waking the server after each
/// read, then marks the end.
fn feed(mut input: Box<dyn std::io::Read + Send>, typed: &Mutex<Input>, wake: &Endpoint) {
    let mut buf = [0u8; 16];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        typed.lock().expect("the input").bytes.extend(&buf[..n]);
        if wake.send(&[0; 4], &[], None, FOREVER).is_err() {
            return;
        }
    }
    typed.lock().expect("the input").ended = true;
    let _ = wake.send(&[0; 4], &[], None, FOREVER);
}

fn serve_or_park(
    server: &mut NineServer<Stream>,
    parked: &mut Parked<()>,
    request: Request,
    now: u64,
) -> Result<(), Error> {
    let held = server.serve_parking(request, |_, request| refuse_malformed(request).map(|()| None))?;
    let Some(request) = held else { return Ok(()) };
    let charge = server.charge_of(&request.caller);
    match parked.park(server.admission_mut(), request, charge, (), now) {
        Ok(()) => Ok(()),
        Err(NotParked(request)) => refuse(request, NineError::TOO_MANY),
    }
}

/// The parked console reads beside the skeleton's loop, as `consoled`'s: an input thread's
/// wake-up is a send the skeleton drops, and the turn after it answers what the input satisfies.
struct Readers(Parked<()>);

impl Around<Stream> for Readers {
    fn call(&mut self, server: &mut NineServer<Stream>, request: Request, now: u64) {
        let _ = serve_or_park(server, &mut self.0, request, now);
    }

    /// Answers the parked calls the input satisfies, then the multiplexed reads waiting for it.
    fn turn(&mut self, server: &mut NineServer<Stream>, now: u64) {
        while server.fs.has_input() {
            let Some(call) = self.0.resume_first(server.admission_mut(), |_| true) else { break };
            let Ok((request, ())) = call else { continue };
            let _ = serve_or_park(server, &mut self.0, request, now);
        }
        if server.fs.has_input() {
            server.wake(now);
        }
    }

    fn abandoned(&mut self, server: &mut NineServer<Stream>, id: NonZeroU64) {
        self.0.abandoned(server.admission_mut(), id, &WORDS_9P);
    }
}

/// Parses a startup block as the fake process it was written for would.
pub fn startup(block: &[u8]) -> Startup<'_> { Startup::parse(block).expect("a startup block") }

// ---- the home volume ----

#[path = "../../../../servers/littlefsd/src/bin/littlefsd.rs"]
#[allow(dead_code)]
mod littlefsd;

/// Bytes of a sector of the fake `blkd`'s range.
const SECTOR: usize = 512;

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

/// A range of sectors in memory behind `blkd`'s protocol.
struct Sectors {
    bytes: Vec<u8>,
    out: Vec<u8>,
}

impl TypedServer<Blkd> for Sectors {
    fn handle<'s>(
        &'s mut self,
        _: &Caller,
        request: BlkdMessage<'_>,
        _: &[Handle],
    ) -> Result<Answer<blkd::Reply<'s>>, BlkdError> {
        let len = self.bytes.len();
        let span = |sector: u64, n: usize| {
            let start = sector as usize * SECTOR;
            (start + n <= len).then_some(start..start + n).ok_or(BlkdError::OutOfRange)
        };
        let reply = match request {
            BlkdMessage::Info(_) => blkd::Reply::Info(InfoReply {
                sectors: (len / SECTOR) as u64,
                sector_size: SECTOR as u32,
                read_only: 0,
            }),
            BlkdMessage::Read(r) => {
                self.out = self.bytes[span(r.sector, r.count as usize * SECTOR)?].to_vec();
                return Ok(Answer::new(blkd::Reply::Read(ReadReply { data: &self.out })));
            }
            BlkdMessage::Write(w) => {
                let span = span(w.sector, w.data.len())?;
                self.bytes[span].copy_from_slice(w.data);
                blkd::Reply::Write(WriteReply {})
            }
            BlkdMessage::Flush(_) => blkd::Reply::Flush(FlushReply {}),
        };
        Ok(Answer::new(reply))
    }
}

/// A `littlefsd` serving a blank volume of its own, labelled with its `labels=` argument if any.
pub struct Volume {
    pub littlefsd: usize,
    /// Its receive endpoint, which a session is granted a connection to.
    pub endpoint: Handle,
    pub thread: JoinHandle<u32>,
    blkd: (usize, Handle),
}

/// Starts `littlefsd` on a blank range of `sectors`, with `args` beside its endpoint's.
pub fn volume(sectors: usize, args: &[&str]) -> Volume {
    let f = fake();
    let blkd = f.process(0, &[]);
    let blkd_receive = f.endpoint(blkd);
    f.run(blkd, move || {
        let endpoint = Endpoint::from_handle(blkd_receive);
        let mut range = Sectors { bytes: vec![0; sectors * SECTOR], out: Vec::new() };
        loop {
            match endpoint.receive(FOREVER, 0) {
                Ok(Event::Call(request)) => {
                    let _ = serve_call::<Blkd, _>(&mut range, request);
                }
                Ok(_) => {}
                Err(_) => return 0,
            }
        }
    });
    let pid = f.process(0, &[]);
    let receive = f.endpoint(pid);
    let range = f.grant(blkd, blkd_receive, pid, 1);
    let mut builder = StartupBuilder::new(receive.index().max(range.index()));
    builder.handle("littlefsd:data", receive).handle("volume", range).arg("endpoint=littlefsd:data");
    for arg in args {
        builder.arg(arg);
    }
    let block = builder.finish().expect("littlefsd's block");
    let thread = f.run(pid, move || littlefsd::serve(&Startup::parse(&block).expect("littlefsd's block")));
    Volume { littlefsd: pid, endpoint: receive, thread, blkd: (blkd, blkd_receive) }
}

impl Volume {
    /// Stops `littlefsd`, then its `blkd`, and returns `littlefsd`'s exit code.
    pub fn stop(self) -> u32 {
        let f = fake();
        f.destroy(self.littlefsd, self.endpoint);
        let code = self.thread.join().unwrap_or(1);
        f.destroy(self.blkd.0, self.blkd.1);
        code
    }
}
