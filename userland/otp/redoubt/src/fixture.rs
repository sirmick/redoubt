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
//! `/dev/cons`, served over 9P, a read with nothing to read parked until input comes, `consol`'s
//! `size` and a `resize` parked until the test changes the size, and its admission) with a byte
//! stream for its device: the host's terminal, or a pipe in a test. What runs over it, the
//! platform and the client library, is what runs on Redoubt; `consoled`'s own device handling is
//! its host tests' and the machine's.
//!
//! **The `/net` is a script** ([`net`]): `ipd`'s tree served over the same skeleton, its
//! connects, refusals and bytes written by the test, so the VM's `gen_tcp` backend is tested
//! against the files it reads and writes without a stack.

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read as _, Write};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use redoubt_fake_kernel::{Launched, fake};
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Request, Words};
use redoubt_rt::server::ninep::{
    Around, FileServer, FileStat, NineError, NineServer, Qid, Read, WORDS_9P, mode, refuse, refuse_malformed,
};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::server::{Limits, consol as consol_server};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::blkd::{
    self, ErrorCode as BlkdError, FlushReply, InfoReply, Message as BlkdMessage, ReadReply, WriteReply,
};
use redoubt_rt::wire::proto::net_ctl;

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
    window: Arc<Mutex<Window>>,
}

/// The console's size, 80 by 24 at first, and how many times it has changed.
#[derive(Clone, Copy)]
struct Window {
    size: (u16, u16),
    changes: u64,
    /// The `resize` calls parked now.
    waiting: usize,
    /// The server's own wake-up endpoint, once it is serving.
    wake: Option<Handle>,
}

impl ConsoleServer {
    /// How many `resize` calls the server holds now: a change before one is parked is not one it
    /// waits for.
    pub fn resizes_waiting(&self) -> usize { self.window.lock().expect("the window").waiting }

    /// The window changes to `cols` by `rows`, as a terminal's does: every `resize` waiting is
    /// answered with it.
    pub fn resize(&self, cols: u16, rows: u16) {
        let wake = {
            let mut w = self.window.lock().expect("the window");
            w.size = (cols, rows);
            w.changes += 1;
            w.wake
        };
        if let Some(wake) = wake {
            fake().as_process(self.pid, || {
                let _ = Endpoint::from_handle(wake).send(&[0; 4], &[], None, FOREVER);
            });
        }
    }
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
    console_built(input, output, labels, 0, CONSOLED_IN_FLIGHT)
}

/// As [`console`], answering the first `busy` writes `busy`, as a server over its share does,
/// and taking the rest.
pub fn console_busy(
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
    busy: u32,
) -> ConsoleServer {
    console_built(input, output, &[], busy, CONSOLED_IN_FLIGHT)
}

/// As [`console`], with an SSH channel's parked calls, `sshd`'s 4 a bucket rather than
/// `consoled`'s 2: a session's completion call and a waiting `resize` both fit one connection's
/// share, so its VM learns of a change of size (servers/sshd.md, "Sessions over SSH").
pub fn console_channel(input: Box<dyn std::io::Read + Send>, output: Box<dyn Write + Send>) -> ConsoleServer {
    console_built(input, output, &[], 0, 4)
}

/// `consoled`'s parked calls a bucket: a lone connection's share is one, which a multiplexed
/// session's completion call takes.
const CONSOLED_IN_FLIGHT: u32 = 2;

fn console_built(
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
    labels: &[u64],
    busy: u32,
    in_flight: u32,
) -> ConsoleServer {
    let f = fake();
    let pid = f.process(0, labels);
    let endpoint = f.endpoint(pid);
    let writes = Arc::new(Mutex::new(Vec::new()));
    let window = Arc::new(Mutex::new(Window { size: (80, 24), changes: 0, waiting: 0, wake: None }));
    let stream = Stream {
        input: Arc::new(Mutex::new(Input::default())),
        output,
        labels: labels.to_vec(),
        busy,
        writes: Arc::clone(&writes),
        window: Arc::clone(&window),
        in_flight,
    };
    let thread = f.run(pid, move || serve(endpoint, pid, input, stream));
    ConsoleServer { pid, endpoint, thread, writes, window }
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
    built(Some(console), volumes, args, labels, extra)
}

/// A new process with no `/dev/cons`, as a lease's (docs/userland/agents.md, "Leases"), a
/// connection to each volume, and `args`, and its startup block.
pub fn lease(volumes: &[(&str, &Volume)], args: &[&str]) -> (usize, Vec<u8>) {
    built(None, volumes, args, &[], |_| Vec::new())
}

fn built(
    console: Option<&ConsoleServer>,
    volumes: &[(&str, &Volume)],
    args: &[&str],
    labels: &[u64],
    extra: impl FnOnce(usize) -> Vec<(&'static str, Handle)>,
) -> (usize, Vec<u8>) {
    let f = fake();
    let pid = f.process(1001, labels);
    let mut held: Vec<(&str, Handle)> = console
        .map(|console| ("/dev/cons", f.grant(console.pid, console.endpoint, pid, 0x20 + pid as u64)))
        .into_iter()
        .collect();
    held.extend(
        volumes.iter().map(|(name, volume)| {
            (*name, f.grant(volume.littlefsd, volume.endpoint, pid, 0x40 + pid as u64))
        }),
    );
    held.extend(extra(pid));
    let highest = held.iter().map(|(_, h)| h.index()).max().unwrap_or(0);
    let mut builder = StartupBuilder::new(highest);
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
    window: Arc<Mutex<Window>>,
    /// Its parked calls a bucket.
    in_flight: u32,
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
        // A held read waits as long as the test holds it: the server answers no `timeout` of
        // its own, so what a client does with a request never answered is the client's.
        server.requests_wait(FOREVER);
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
fn serve(endpoint: Handle, pid: usize, input: Box<dyn std::io::Read + Send>, stream: Stream) -> u32 {
    let endpoint = Endpoint::from_handle(endpoint);
    // `consoled`'s shares: a page a badge for writes, and requests to spare for a parked read.
    let limits =
        Limits { buckets: 4, in_flight: stream.in_flight, files: 4, state: 4, requests: 80, pages: 2 };
    let random = redoubt_rt::handle::random_u64().unwrap_or(1);
    let (typed, window) = (Arc::clone(&stream.input), Arc::clone(&stream.window));
    let Ok(mut server) = NineServer::new(stream, limits, random) else { return 1 };
    // A console read waits on a person, so it has no deadline.
    server.requests_wait(FOREVER);
    let Ok(wake) = endpoint.mint(NonZeroU64::new(INPUT).expect("non-zero"), None) else { return 2 };
    let Ok(resized) = endpoint.mint(NonZeroU64::new(INPUT).expect("non-zero"), None) else { return 2 };
    window.lock().expect("the window").wake = Some(resized.handle());
    std::thread::spawn(move || feed(pid, input, &typed, &wake));
    server.run_around(&endpoint, Readers(Parked::new(FOREVER)))
}

/// The input thread: reads the device into the console's input, waking the server after each
/// read, then marks the end. A thread of the fake only while it wakes the server: blocked in the
/// device's read it is the host's, so a tick of a held clock (`Fake::tick`), which waits for every
/// thread of the fake to reach a wait, does not wait on a keyboard nobody types on.
fn feed(pid: usize, mut input: Box<dyn std::io::Read + Send>, typed: &Mutex<Input>, wake: &Endpoint) {
    let f = fake();
    let mut buf = [0u8; 16];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        typed.lock().expect("the input").bytes.extend(&buf[..n]);
        if f.as_process(pid, || wake.send(&[0; 4], &[], None, FOREVER)).is_err() {
            return;
        }
    }
    typed.lock().expect("the input").ended = true;
    let _ = f.as_process(pid, || wake.send(&[0; 4], &[], None, FOREVER));
}

/// What a parked call waits for: input, or a change of size since the count it saw.
#[derive(Clone, Copy, PartialEq)]
enum Waiting {
    Read,
    Resize(u64),
}

fn serve_or_park(
    server: &mut NineServer<Stream>,
    parked: &mut Parked<Waiting>,
    request: Request,
    now: u64,
) -> Result<(), Error> {
    let window = *server.fs.window.lock().expect("the window");
    let held = server.serve_parking(request, |_, request| {
        if consol_server::asks(&request.words) {
            consol_server::serve(window.size, request)
        } else {
            refuse_malformed(request).map(|()| None)
        }
    })?;
    let Some(request) = held else { return Ok(()) };
    let waiting =
        if consol_server::asks(&request.words) { Waiting::Resize(window.changes) } else { Waiting::Read };
    let charge = server.charge_of(&request.caller);
    match parked.park(server.admission_mut(), request, charge, waiting, now) {
        Ok(()) => {
            if waiting != Waiting::Read {
                server.fs.window.lock().expect("the window").waiting += 1;
            }
            Ok(())
        }
        Err(NotParked(request)) if waiting == Waiting::Read => refuse(request, NineError::TOO_MANY),
        Err(NotParked(request)) => refuse_malformed(request),
    }
}

/// The parked console calls beside the skeleton's loop, as `consoled`'s: an input thread's
/// wake-up is a send the skeleton drops, and the turn after it answers what the input satisfies,
/// and every `resize` the window's change satisfies.
struct Readers(Parked<Waiting>);

impl Around<Stream> for Readers {
    fn call(&mut self, server: &mut NineServer<Stream>, request: Request, now: u64) {
        let _ = serve_or_park(server, &mut self.0, request, now);
    }

    /// Answers the `resize` calls the window's change satisfies and the parked reads the input
    /// satisfies, then the multiplexed reads waiting for it.
    fn turn(&mut self, server: &mut NineServer<Stream>, now: u64) {
        let window = *server.fs.window.lock().expect("the window");
        let due = |w: &Waiting| matches!(w, Waiting::Resize(from) if *from != window.changes);
        while let Some(call) = self.0.resume_first(server.admission_mut(), due) {
            server.fs.window.lock().expect("the window").waiting -= 1;
            if let Ok((request, _)) = call {
                let _ = consol_server::reply_resize(request, window.size);
            }
        }
        while server.fs.has_input() {
            let read = |w: &Waiting| *w == Waiting::Read;
            let Some(call) = self.0.resume_first(server.admission_mut(), read) else { break };
            let Ok((request, _)) = call else { continue };
            let _ = serve_or_park(server, &mut self.0, request, now);
        }
        if server.fs.has_input() {
            server.wake(now);
        }
    }

    fn abandoned(&mut self, server: &mut NineServer<Stream>, id: NonZeroU64) {
        if let Some(Waiting::Resize(_)) = self.0.abandoned(server.admission_mut(), id, &WORDS_9P) {
            server.fs.window.lock().expect("the window").waiting -= 1;
        }
    }
}

/// Parses a startup block as the fake process it was written for would.
pub fn startup(block: &[u8]) -> Startup<'_> { Startup::parse(block).expect("a startup block") }

// ---- a scripted `/net` ----

/// What a scripted `/net` was asked, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetSeen {
    /// A socket made by a read of `clone`.
    Clone(u32),
    /// `connect(addr, port)` written to a socket's ctl.
    Connect(u32, [u8; 4], u16),
    /// `listen(port, backlog)` written to a socket's ctl.
    Listen(u32, u16, u8),
    Close(u32),
    Abort(u32),
    /// Bytes written to a socket's data.
    Wrote(u32, Vec<u8>),
}

/// A socket of the script's, by what `ctl` reads for it.
#[derive(Default)]
struct NetSock {
    state: u32,
    /// What a read of `data` takes, in order.
    inbound: VecDeque<u8>,
    /// The peer has ended: a read with nothing left takes 0 bytes.
    ended: bool,
    remote: ([u8; 4], u16),
    /// A listener's accepted sockets not yet reported by a read of its ctl.
    accepted: VecDeque<u32>,
    /// When each read of its data or ctl was held ([`NetScript::holds`]).
    holds: Vec<u64>,
}

/// How long a read of `data` or `ctl` is held before it is answered `timeout`: a server holds a
/// request strictly shorter than its clients wait for it (`FILE_WAIT_US`, 10 s), so that the
/// client asks again and no late answer is dropped.
pub const NET_WAIT_US: u64 = 8_000_000;

/// The script of a `/net` served as `ipd` serves its tree (docs/servers/ipd.md, "The `/net`
/// tree"), short of a stack: a `connect` to an address in `reach` is established at once and to
/// any other reads back closed, an address in `own` is refused by name before anything else, a
/// read of `data` with nothing to read is held, 0 bytes once the peer has ended, and a write is
/// taken whole, echoed back when `echo` is set. The test feeds and ends a socket's stream and
/// hands a listener an accepted socket, then wakes the server.
pub struct NetScript {
    pub reach: Vec<([u8; 4], u16)>,
    pub own: Vec<[u8; 4]>,
    pub echo: bool,
    pub seen: Vec<NetSeen>,
    sockets: BTreeMap<u32, NetSock>,
    /// The test fed or ended something since the server last served its held reads again.
    dirty: bool,
}

impl NetScript {
    /// Bytes the peer of socket `n` sent.
    pub fn feed(&mut self, n: u32, bytes: &[u8]) {
        if let Some(s) = self.sockets.get_mut(&n) {
            s.inbound.extend(bytes);
            self.dirty = true;
        }
    }

    /// The peer of socket `n` ended its side.
    pub fn end(&mut self, n: u32) {
        if let Some(s) = self.sockets.get_mut(&n) {
            s.ended = true;
            self.dirty = true;
        }
    }

    /// A connection from `remote` accepted at listener `n`: the new socket's number.
    pub fn accept(&mut self, n: u32, remote: ([u8; 4], u16)) -> u32 {
        let m = (1..).find(|m| !self.sockets.contains_key(m)).expect("a free number");
        self.sockets.insert(m, NetSock { state: 2, remote, ..NetSock::default() });
        if let Some(l) = self.sockets.get_mut(&n) {
            l.accepted.push_back(m);
            self.dirty = true;
        }
        m
    }

    /// When (`time_now`, µs) each read of socket `n`'s data or ctl was held for want of
    /// anything to answer. A request is served again each time the server is woken, so one
    /// read may be held several times; a hold past a request's deadline is another request's,
    /// the first having been answered `timeout`.
    pub fn holds(&self, n: u32) -> Vec<u64> {
        self.sockets.get(&n).map(|s| s.holds.clone()).unwrap_or_default()
    }
}

/// A `/net` served from a script, on the fake kernel.
pub struct NetServer {
    pub pid: usize,
    pub endpoint: Handle,
    pub thread: JoinHandle<u32>,
    pub script: Arc<Mutex<NetScript>>,
    wake: Handle,
}

impl NetServer {
    /// Has the server look at its script again: what the test fed or ended answers the reads
    /// held for it.
    pub fn wake(&self) {
        let wake = Endpoint::from_handle(self.wake);
        let _ = fake().as_process(self.pid, || wake.send(&[0; 4], &[], None, FOREVER));
    }
}

/// Starts a [`NetServer`] that reaches `reach`, refuses `own`, and echoes when `echo`.
pub fn net(reach: &[([u8; 4], u16)], own: &[[u8; 4]], echo: bool) -> NetServer {
    let f = fake();
    let pid = f.process(0, &[]);
    let endpoint = f.endpoint(pid);
    let script = Arc::new(Mutex::new(NetScript {
        reach: reach.to_vec(),
        own: own.to_vec(),
        echo,
        seen: Vec::new(),
        sockets: BTreeMap::new(),
        dirty: false,
    }));
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    let served = Arc::clone(&script);
    let thread = f.run(pid, move || {
        let endpoint = Endpoint::from_handle(endpoint);
        let Ok(wake) = endpoint.mint(NonZeroU64::new(INPUT).expect("non-zero"), None) else { return 2 };
        wake_tx.send(wake.handle()).expect("the test waits for the wake handle");
        // `ipd`'s shape of share: requests to spare for the reads it holds.
        let limits = Limits { buckets: 4, in_flight: 16, files: 64, state: 16, requests: 80, pages: 2 };
        let random = redoubt_rt::handle::random_u64().unwrap_or(1);
        let Ok(mut server) = NineServer::new(NetFs(served), limits, random) else { return 1 };
        server.requests_wait(NET_WAIT_US);
        server.run_around(&endpoint, NetReads)
    });
    let wake = wake_rx.recv().expect("the wake handle");
    NetServer { pid, endpoint, thread, script, wake }
}

/// The tree's nodes: `/`, `/tcp`, `/tcp/clone` and a socket's directory and files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NetNode {
    Root,
    Tcp,
    Clone,
    Dir(u32),
    Ctl(u32),
    Data(u32),
    Remote(u32),
}

fn net_qid(node: NetNode) -> Qid {
    let (kind, path) = match node {
        NetNode::Root => (0x80, 0),
        NetNode::Tcp => (0x80, 1),
        NetNode::Clone => (0, 2),
        NetNode::Dir(n) => (0x80, 16 + u64::from(n) * 4),
        NetNode::Ctl(n) => (0, 17 + u64::from(n) * 4),
        NetNode::Data(n) => (0, 18 + u64::from(n) * 4),
        NetNode::Remote(n) => (0, 19 + u64::from(n) * 4),
    };
    Qid { kind, version: 0, path }
}

fn net_name(node: NetNode) -> String {
    match node {
        NetNode::Root => "/".into(),
        NetNode::Tcp => "tcp".into(),
        NetNode::Clone => "clone".into(),
        NetNode::Dir(n) => n.to_string(),
        NetNode::Ctl(_) => "ctl".into(),
        NetNode::Data(_) => "data".into(),
        NetNode::Remote(_) => "remote".into(),
    }
}

struct NetFs(Arc<Mutex<NetScript>>);

impl NetFs {
    fn script(&self) -> std::sync::MutexGuard<'_, NetScript> { self.0.lock().expect("the script") }
}

/// A `net_ctl` error as `ipd` names it in an `Rerror`.
fn net_error(name: &'static str) -> NineError { NineError(name) }

impl FileServer for NetFs {
    type Node = NetNode;

    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(NetNode, Qid), NineError> {
        Ok((NetNode::Root, net_qid(NetNode::Root)))
    }

    fn labels(&self, _: &NetNode) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, dir: &NetNode, name: &str) -> Result<(NetNode, Qid), NineError> {
        let node = match (dir, name) {
            (NetNode::Root, "tcp") => NetNode::Tcp,
            (NetNode::Tcp, "clone") => NetNode::Clone,
            (NetNode::Tcp, _) => {
                let n: u32 = name.parse().map_err(|_| NineError::NOT_FOUND)?;
                if !self.script().sockets.contains_key(&n) {
                    return Err(NineError::NOT_FOUND);
                }
                NetNode::Dir(n)
            }
            (NetNode::Dir(n), "ctl") => NetNode::Ctl(*n),
            (NetNode::Dir(n), "data") => NetNode::Data(*n),
            (NetNode::Dir(n), "remote") => NetNode::Remote(*n),
            (NetNode::Root | NetNode::Dir(_), _) => return Err(NineError::NOT_FOUND),
            _ => return Err(NineError::NOT_DIR),
        };
        Ok((node, net_qid(node)))
    }

    /// As `ipd`: only `ctl` and `data` take writes.
    fn open(&mut self, _: &Caller, node: &NetNode, m: u8) -> Result<Qid, NineError> {
        let writes = matches!(m & 3, mode::OWRITE | mode::ORDWR) || m & mode::OTRUNC != 0;
        if writes && !matches!(node, NetNode::Ctl(_) | NetNode::Data(_)) {
            return Err(NineError::PERMISSION);
        }
        Ok(net_qid(*node))
    }

    fn read(&mut self, _: &Caller, node: &NetNode, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let mut script = self.script();
        match *node {
            NetNode::Clone => {
                if offset != 0 {
                    return Ok(Read::Done(0));
                }
                let n = (1..).find(|n| !script.sockets.contains_key(n)).expect("a free number");
                script.sockets.insert(n, NetSock::default());
                script.seen.push(NetSeen::Clone(n));
                let out = out.get_mut(..4).ok_or(NineError::TOO_SMALL)?;
                out.copy_from_slice(&n.to_le_bytes());
                Ok(Read::Done(4))
            }
            NetNode::Ctl(n) => {
                let sock = script.sockets.get_mut(&n).ok_or_else(|| net_error("state"))?;
                let m = if sock.state == 5 {
                    let Some(m) = sock.accepted.pop_front() else {
                        sock.holds.push(redoubt_rt::handle::time_now().unwrap_or(0));
                        return Ok(Read::Wait);
                    };
                    m
                } else {
                    n
                };
                let out = out.get_mut(..8).ok_or(NineError::TOO_SMALL)?;
                out[..4].copy_from_slice(&sock.state.to_le_bytes());
                out[4..].copy_from_slice(&m.to_le_bytes());
                Ok(Read::Done(8))
            }
            NetNode::Data(n) => {
                let sock = script.sockets.get_mut(&n).ok_or_else(|| net_error("state"))?;
                if sock.state != 2 && sock.state != 3 {
                    return Err(net_error("state"));
                }
                if sock.inbound.is_empty() {
                    if sock.ended {
                        return Ok(Read::Done(0));
                    }
                    sock.holds.push(redoubt_rt::handle::time_now().unwrap_or(0));
                    return Ok(Read::Wait);
                }
                let k = out.len().min(sock.inbound.len());
                for (slot, byte) in out[..k].iter_mut().zip(sock.inbound.drain(..k)) {
                    *slot = byte;
                }
                Ok(Read::Done(k))
            }
            NetNode::Remote(n) => {
                if offset != 0 {
                    return Ok(Read::Done(0));
                }
                let sock = script.sockets.get(&n).ok_or_else(|| net_error("state"))?;
                let out = out.get_mut(..6).ok_or(NineError::TOO_SMALL)?;
                out[..4].copy_from_slice(&sock.remote.0);
                out[4..].copy_from_slice(&sock.remote.1.to_le_bytes());
                Ok(Read::Done(6))
            }
            NetNode::Root | NetNode::Tcp | NetNode::Dir(_) => Err(NineError::NOT_SUPPORTED),
        }
    }

    fn write(&mut self, _: &Caller, node: &NetNode, _offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let mut script = self.script();
        match *node {
            NetNode::Ctl(n) => {
                let op = net_ctl::Message::decode_file(data).map_err(|_| NineError::BAD_MESSAGE)?;
                if !script.sockets.contains_key(&n) {
                    return Err(net_error("state"));
                }
                match op {
                    net_ctl::Message::Connect(c) => {
                        let addr: [u8; 4] = c.addr.try_into().map_err(|_| NineError::BAD_MESSAGE)?;
                        script.seen.push(NetSeen::Connect(n, addr, c.port));
                        if script.own.contains(&addr) {
                            return Err(net_error("not_permitted"));
                        }
                        let reached = script.reach.contains(&(addr, c.port));
                        let sock = script.sockets.get_mut(&n).expect("checked above");
                        if sock.state != 0 {
                            return Err(net_error("state"));
                        }
                        sock.state = if reached { 2 } else { 4 };
                        sock.remote = (addr, c.port);
                    }
                    net_ctl::Message::Listen(l) => {
                        script.seen.push(NetSeen::Listen(n, l.port, l.backlog));
                        if l.backlog > 8 {
                            return Err(net_error("too_many"));
                        }
                        let sock = script.sockets.get_mut(&n).expect("checked above");
                        if sock.state != 0 {
                            return Err(net_error("state"));
                        }
                        sock.state = 5;
                    }
                    net_ctl::Message::Close(_) => {
                        script.seen.push(NetSeen::Close(n));
                        let sock = script.sockets.get_mut(&n).expect("checked above");
                        // The graceful end: what is left to read is read, then the socket goes.
                        sock.state = if sock.state == 2 { 3 } else { 4 };
                    }
                    net_ctl::Message::Abort(_) => {
                        script.seen.push(NetSeen::Abort(n));
                        script.sockets.remove(&n);
                    }
                }
                Ok(data.len())
            }
            NetNode::Data(n) => {
                let echo = script.echo;
                let sock = script.sockets.get_mut(&n).ok_or_else(|| net_error("state"))?;
                if sock.state != 2 {
                    return Err(net_error("state"));
                }
                if echo {
                    sock.inbound.extend(data);
                }
                script.seen.push(NetSeen::Wrote(n, data.to_vec()));
                Ok(data.len())
            }
            _ => Err(NineError::PERMISSION),
        }
    }

    fn stat(&mut self, _: &Caller, node: &NetNode) -> Result<FileStat, NineError> {
        let mode = if net_qid(*node).kind & 0x80 != 0 { 0o040_555 } else { 0o666 };
        Ok(FileStat { qid: net_qid(*node), mode, mtime: 0, length: 0, name: net_name(*node) })
    }

    fn dir_entry(
        &mut self,
        _: &Caller,
        _: &NetNode,
        _: u64,
    ) -> Result<Option<(NetNode, FileStat)>, NineError> {
        Ok(None)
    }
}

/// Beside the skeleton's loop: the hub's requests are multiplexed, so a read the file server
/// asks to hold stays with the skeleton, which serves it again on `wake` and answers it
/// `timeout` at [`NET_WAIT_US`], as `ipd`'s does; the test's feeding sets the script dirty and
/// wakes the server, whose next turn serves the held reads again.
struct NetReads;

impl Around<NetFs> for NetReads {
    fn call(&mut self, server: &mut NineServer<NetFs>, request: Request, _now: u64) {
        let _ = server.serve_with(request, |_, request| refuse_malformed(request).map(|()| None));
    }

    fn turn(&mut self, server: &mut NineServer<NetFs>, now: u64) {
        if std::mem::take(&mut server.fs.script().dirty) {
            server.wake(now);
        }
    }
}

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
pub fn volume(sectors: usize, args: &[&str]) -> Volume { volume_on(sectors, args, None) }

/// As [`volume`], but serving the endpoint a launch with `serve` gave `child`, its first handle:
/// the test plays the server the VM launched, as a session's `piped` is, in the child's budget.
pub fn volume_served(child: &Launched, sectors: usize, args: &[&str]) -> Volume {
    volume_on(sectors, args, Some(child))
}

fn volume_on(sectors: usize, args: &[&str], serving: Option<&Launched>) -> Volume {
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
    let receive = match serving {
        Some(child) => f.play(child, pid)[0],
        None => f.endpoint(pid),
    };
    let range = f.grant(blkd, blkd_receive, pid, 1);
    let mut builder = StartupBuilder::new(receive.index().max(range.index()));
    // The manifests' entry, as `init` writes it: 1 024 pages, no heap cap.
    builder
        .handle("littlefsd:data", receive)
        .handle("volume", range)
        .budget_pages(1024)
        .arg("endpoint=littlefsd:data");
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
