//! `/dev/cons` behind the 9P skeleton: one file, whose reads are input and whose writes are
//! output.

use alloc::collections::VecDeque;
use alloc::string::String;

use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, Qid, Read, mode};
use redoubt_rt::server::{Cost, Limits};

use crate::uart::Uart;

/// Input bytes held for readers. Beyond this [`Console::drain`] still empties the UART's FIFO but
/// drops what it takes, so a flood keeps what was typed first.
pub const MAX_INPUT: usize = 1024;

/// What admission lets clients hold, sized so that every bucket at its cap fits [`BUDGET`]
/// (servers/serving.md R26), and so that the parked reads every bucket may hold together stay
/// well under `MAX_OPEN_CALLS` with its headroom ([`redoubt_rt::server::Admission::new`] checks
/// that).
pub const LIMITS: Limits = Limits { buckets: 4, in_flight: 2, files: 4, state: 4 };
/// What one of each costs, in bytes. A parked read holds its caller's lend, charged to this
/// server until it replies (kernel/ipc.md R3), which is `MAX_LEND_PAGES` pages at worst; a
/// fid and a minted connection are small records.
pub const COST: Cost = Cost { in_flight: 64 * 1024, file: 256, state: 256 };
/// The bytes of this server's budget its clients may use between them; its manifest entry gives
/// it the budget, and the program refuses limits that would not fit.
pub const BUDGET: u64 = 1024 * 1024;

/// What a fid rests on: there is one file, so there is one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cons;

/// The console: the UART and the input nobody has read yet.
pub struct Console {
    uart: Uart,
    input: VecDeque<u8>,
    /// Bytes taken from the FIFO that the ring had no room for, and so were dropped. No program
    /// path reports it; only tests read it, through [`Console::dropped`].
    dropped: u64,
}

impl Console {
    pub fn new(uart: Uart) -> Console { Console { uart, input: VecDeque::new(), dropped: 0 } }

    /// Takes what the UART has, up to [`crate::uart::FIFO`] bytes, and keeps each one unless the
    /// ring already holds [`MAX_INPUT`] bytes or cannot reserve room for it, in which case the byte
    /// is dropped and counted; how many it kept. Called before parking a read and at the top of
    /// every turn of the serving loop, wake-ups from the interrupt thread included, so a byte is
    /// never left in the FIFO with a reader waiting for it.
    ///
    /// The bound is the receive FIFO's depth, which is all a 16550 can be holding: a device
    /// that says "data ready" for ever — broken, or an FPGA card's line stuck low — then costs
    /// this server a bounded number of register reads per call instead of hanging it
    /// (TENETS.md 6: a hostile device gets a clean refusal, never a server that stops).
    pub fn drain(&mut self) -> usize {
        let mut taken = 0;
        for _ in 0..crate::uart::FIFO {
            let Some(byte) = self.uart.take() else { break };
            if self.input.len() >= MAX_INPUT || self.input.try_reserve(1).is_err() {
                self.dropped += 1;
                continue;
            }
            self.input.push_back(byte);
            taken += 1;
        }
        taken
    }

    /// Whether a read can be answered now.
    pub fn has_input(&self) -> bool { !self.input.is_empty() }

    /// How many input bytes [`Console::drain`] has dropped.
    pub fn dropped(&self) -> u64 { self.dropped }

    /// The UART, for the program's start-up banner and for tests.
    pub fn uart(&self) -> &Uart { &self.uart }
}

impl FileServer for Console {
    type Node = Cons;

    /// Every connection attaches at the file itself, not at a directory holding it: a namespace
    /// binds `/dev/cons` to the connection, and a program opens and reads it straight away
    /// (servers/consoled.md, "`/dev/cons`"; the runtime's panic reporter does exactly that).
    fn attach(&mut self, _: &Caller, _aname: &str) -> Result<(Cons, Qid), NineError> { Ok((Cons, qid())) }

    /// The physical console carries no labels (servers/consoled.md R69). So anyone may read it,
    /// and `check` lets only an unlabelled caller write to it: no write down onto a screen someone
    /// else is looking at.
    fn labels(&self, _: &Cons) -> &[u64] { &[] }

    /// `/dev/cons` is a file, not a directory; the skeleton refuses a walk from it before this
    /// is ever reached.
    fn walk(&mut self, _: &Caller, _: &Cons, _name: &str) -> Result<(Cons, Qid), NineError> {
        Err(NineError::NOT_DIR)
    }

    /// Reading, writing or both. `OEXEC` and `OTRUNC` mean nothing on a console.
    fn open(&mut self, _: &Caller, _: &Cons, open_mode: u8) -> Result<Qid, NineError> {
        if open_mode & mode::OTRUNC != 0 || !matches!(open_mode & 3, mode::OREAD | mode::OWRITE | mode::ORDWR)
        {
            return Err(NineError::BAD_MODE);
        }
        Ok(qid())
    }

    /// Input, or a request to hold the call. The offset is ignored: a console is a stream of
    /// what has been typed, not an array, so there is no position to seek to and no end to
    /// reach — which is why an empty ring waits instead of answering nothing.
    fn read(&mut self, _: &Caller, _: &Cons, _offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        if out.is_empty() {
            return Ok(Read::Done(0));
        }
        self.drain();
        if self.input.is_empty() {
            return Ok(Read::Wait);
        }
        let n = out.len().min(self.input.len());
        for slot in out[..n].iter_mut() {
            // The length was just measured, so every one of these is there.
            *slot = self.input.pop_front().unwrap_or(0);
        }
        Ok(Read::Done(n))
    }

    /// Output. The offset is ignored, as for a read. A byte the transmitter would not take is a
    /// short write, which 9P allows, rather than a server that spins.
    fn write(&mut self, _: &Caller, _: &Cons, _offset: u64, data: &[u8]) -> Result<usize, NineError> {
        Ok(self.uart.put_all(data))
    }

    /// Length 0: a console has no size, and a client that believed one would read the wrong
    /// amount.
    fn stat(&mut self, _: &Caller, _: &Cons) -> Result<FileStat, NineError> {
        let mut name = String::new();
        name.try_reserve(4).map_err(|_| NineError::NO_MEMORY)?;
        name.push_str("cons");
        Ok(FileStat { qid: qid(), mode: 0o666, mtime: 0, length: 0, name })
    }

    /// Never a directory, so never listed.
    fn dir_entry(&mut self, _: &Caller, _: &Cons, _: u64) -> Result<Option<(Cons, FileStat)>, NineError> {
        Ok(None)
    }
}

/// The one qid: a file (no `QTDIR`), whose version never changes because a console has no
/// contents to version.
pub fn qid() -> Qid { Qid { kind: 0, version: 0, path: 0 } }
