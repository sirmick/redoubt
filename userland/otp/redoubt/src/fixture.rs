//! Running the platform on a host, on the fake kernel (`redoubt-fake-kernel`): a console server
//! for it to reach, a session process to run it in, and the host's ways to start its threads and
//! find its modules. Host only (the `fake` feature); nothing here runs on the machine.
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
use std::thread::JoinHandle;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Request};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{
    FileServer, FileStat, NineError, NineServer, Qid, Read, WORDS_9P, mode, refuse, refuse_malformed,
};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::startup::{Startup, StartupBuilder};

use crate::{Modules, Threads};

/// The badge the input thread's messages come with, as `consoled`'s interrupt thread's do.
const INPUT: u64 = 1;
/// Word 0 of an input message: bytes follow (word 1 their count, words 2 and 3 them).
const BYTES: u64 = 1;
/// Word 0 of an input message: the input has ended.
const END: u64 = 2;

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

/// Threads on a host: host threads that the fake kernel counts as process `pid`.
pub struct HostThreads {
    pub pid: usize,
}

impl Threads for HostThreads {
    fn spawn(&self, body: Box<dyn FnOnce() + Send + 'static>) {
        let pid = self.pid;
        std::thread::spawn(move || fake().as_process(pid, body));
    }
}

/// Modules from host directories, the first that has a file of the name, as beamlet's `-pa`.
pub struct Dirs(pub Vec<PathBuf>);

impl Modules for Dirs {
    fn load(&mut self, file: &str) -> Option<Vec<u8>> {
        if file.is_empty() || file.contains(['/', '\\', '\0']) || file.starts_with('.') {
            return None;
        }
        self.0.iter().map(|dir| dir.join(file)).find(|p| p.is_file()).and_then(|p| std::fs::read(p).ok())
    }
}

/// The file every connection attaches at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cons;

/// The console: what was typed and nobody has read yet, and the screen.
struct Stream {
    input: VecDeque<u8>,
    ended: bool,
    output: Box<dyn Write + Send>,
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
        if self.input.is_empty() {
            return Ok(if self.ended { Read::Done(0) } else { Read::Wait });
        }
        let n = out.len().min(self.input.len());
        for (slot, byte) in out[..n].iter_mut().zip(self.input.drain(..n)) {
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

/// Serves `/dev/cons` on `endpoint` until the endpoint is destroyed, as `consoled` does, with
/// an input thread in place of its interrupt thread.
fn serve(
    endpoint: Handle,
    pid: usize,
    input: Box<dyn std::io::Read + Send>,
    output: Box<dyn Write + Send>,
) -> u32 {
    let endpoint = Endpoint::from_handle(endpoint);
    let limits = Limits { buckets: 4, in_flight: 2, files: 4, state: 4 };
    let random = redoubt_rt::handle::random_u64().unwrap_or(1);
    let stream = Stream { input: VecDeque::new(), ended: false, output };
    let Ok(mut server) = NineServer::new(stream, limits, random) else { return 1 };
    let mut parked: Parked<()> = Parked::new(FOREVER);
    let Ok(wake) = endpoint.mint(NonZeroU64::new(INPUT).expect("non-zero"), None) else { return 2 };
    std::thread::spawn(move || fake().as_process(pid, move || feed(input, &wake)));
    loop {
        let now = redoubt_rt::handle::time_now().unwrap_or(0);
        wake_readers(&mut server, &mut parked, now);
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                let _ = serve_or_park(&mut server, &mut parked, request, now);
            }
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
                if delivery.caller.badge == INPUT {
                    let w = delivery.words;
                    match w[0] {
                        BYTES => {
                            let mut bytes = [0u8; 16];
                            bytes[..8].copy_from_slice(&w[2].to_le_bytes());
                            bytes[8..].copy_from_slice(&w[3].to_le_bytes());
                            server.fs.input.extend(&bytes[..(w[1] as usize).min(16)]);
                        }
                        END => server.fs.ended = true,
                        _ => {}
                    }
                }
            }
            Ok(Event::Abandoned(id)) => {
                parked.abandoned(server.admission_mut(), id, &WORDS_9P);
            }
            Ok(Event::Interrupt | Event::Exit(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return 3,
        }
    }
}

/// The input thread: reads the device, and sends what it reads to the serving thread, then the end.
fn feed(mut input: Box<dyn std::io::Read + Send>, to: &Endpoint) {
    let mut buf = [0u8; 16];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut chunk = [0u8; 16];
        chunk[..n].copy_from_slice(&buf[..n]);
        let low = u64::from_le_bytes(chunk[..8].try_into().expect("8 bytes"));
        let high = u64::from_le_bytes(chunk[8..].try_into().expect("8 bytes"));
        if to.send(&[BYTES, n as u64, low, high], &[], None, FOREVER).is_err() {
            return;
        }
    }
    let _ = to.send(&[END, 0, 0, 0], &[], None, FOREVER);
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

/// Answers the parked reads there is input for, or that the end of input answers.
fn wake_readers(server: &mut NineServer<Stream>, parked: &mut Parked<()>, now: u64) {
    while !server.fs.input.is_empty() || server.fs.ended {
        let Some(call) = parked.resume_first(server.admission_mut(), |_| true) else { return };
        let Ok((request, ())) = call else { continue };
        let _ = serve_or_park(server, parked, request, now);
    }
}

/// Parses a startup block as the fake process it was written for would.
pub fn startup(block: &[u8]) -> Startup<'_> { Startup::parse(block).expect("a startup block") }
