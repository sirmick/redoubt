//! Running the platform on a host, on the fake kernel (`redoubt-fake-kernel`): a console server
//! for it to reach, a session process to run it in, and the host's way to find its modules. Host
//! only (the `fake` feature); nothing here runs on the machine.
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
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Request};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{
    Around, FileServer, FileStat, NineError, NineServer, Qid, Read, WORDS_9P, mode, refuse, refuse_malformed,
};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::startup::{Startup, StartupBuilder};

use crate::{Modules, Unloaded};

/// The badge of the input thread's wake-ups, as `consoled`'s interrupt thread's are.
const INPUT: u64 = 1;

/// A console server running as a fake process.
pub struct ConsoleServer {
    pub pid: usize,
    /// Its receive endpoint, which a session is granted a connection to.
    pub endpoint: Handle,
    pub thread: JoinHandle<u32>,
}

/// Starts the console server, with `input` as what is typed and `output` as the screen.
pub fn console(input: Box<dyn std::io::Read + Send>, output: Box<dyn Write + Send>) -> ConsoleServer {
    let f = fake();
    let pid = f.process(0, &[]);
    let endpoint = f.endpoint(pid);
    let thread = f.run(pid, move || serve(endpoint, pid, input, output));
    ConsoleServer { pid, endpoint, thread }
}

/// A new session process, with a connection to `console` bound at `/dev/cons`, and its startup
/// block, as a launcher writes it.
pub fn session(console: &ConsoleServer) -> (usize, Vec<u8>) {
    let f = fake();
    let pid = f.process(1001, &[]);
    let conn = f.grant(console.pid, console.endpoint, pid, 0x20 + pid as u64);
    let block =
        StartupBuilder::new(conn.index()).namespace("/dev/cons", conn).finish().expect("a startup block");
    (pid, block)
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

/// The console: its input, shared with the input thread, and the screen.
struct Stream {
    input: Arc<Mutex<Input>>,
    output: Box<dyn Write + Send>,
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

    fn labels(&self, _: &Cons) -> &[u64] { &[] }

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

    fn write(&mut self, _: &Caller, _: &Cons, _offset: u64, data: &[u8]) -> Result<usize, NineError> {
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

/// Serves `/dev/cons` on `endpoint` until the endpoint is destroyed, as `consoled` does: calls
/// and multiplexed sessions on the skeleton's loop, with an input thread in place of its interrupt
/// thread.
fn serve(
    endpoint: Handle,
    pid: usize,
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
) -> u32 {
    let endpoint = Endpoint::from_handle(endpoint);
    // `consoled`'s shares: a page a badge for writes, and requests to spare for a parked read.
    let limits = Limits { buckets: 4, in_flight: 2, files: 4, state: 4, requests: 80, pages: 2 };
    let random = redoubt_rt::handle::random_u64().unwrap_or(1);
    let typed = Arc::new(Mutex::new(Input::default()));
    let stream = Stream { input: Arc::clone(&typed), output };
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
    let held = server.serve_parking(request, |_, request| refuse_malformed(request))?;
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
