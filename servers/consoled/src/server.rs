//! `/dev/cons` behind the 9P skeleton: one file, whose reads are input and whose writes are
//! output.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::abi::PAGE_SIZE;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, Qid, REQUEST_STATE, Read, mode};
use redoubt_rt::server::{Cost, Limits};

use crate::uart::Uart;

/// Input bytes held for readers. Beyond this [`Console::drain`] still empties the UART's FIFO but
/// drops what it takes, so a flood keeps what was typed first.
pub const MAX_INPUT: usize = 1024;

/// The most columns or rows a size may name, as `sshd` cuts a window to (servers/sshd.md).
pub const MAX_SIDE: u16 = 1024;

/// The console's size from the arguments: `size=COLS,ROWS`, each from 1 to [`MAX_SIDE`], or
/// `None` when there is none: a UART cannot know the size of the terminal at its far end, so only
/// the manifest can say it. A malformed one, or a second, is refused: the manifest sized the
/// console wrongly, and `consoled` does not guess.
pub fn size_arg(args: &[&str]) -> Result<Option<(u16, u16)>, ()> {
    let mut named = args.iter().filter_map(|a| a.strip_prefix("size="));
    let Some(arg) = named.next() else { return Ok(None) };
    let side = |s: &str| s.parse::<u16>().ok().filter(|n| (1..=MAX_SIDE).contains(n));
    match (named.next(), arg.split_once(',')) {
        (None, Some((cols, rows))) => side(cols).zip(side(rows)).map(Some).ok_or(()),
        _ => Err(()),
    }
}

/// What admission lets each of `buckets` buckets hold (servers/serving.md R26); the count is the
/// manifest's `buckets=N` ([`redoubt_rt::server::buckets`]). The program refuses a count whose
/// buckets at their caps would not fit [`BUDGET`], or whose parked reads together would not stay
/// under `MAX_OPEN_CALLS` with its headroom ([`redoubt_rt::server::Admission::new`] checks that).
///
/// A bucket holds `MAX_THREADS` minted connections: `init` starts at most `MAX_THREADS - 1`
/// servers, one thread watching each, and mints every one of their consoles through its one root
/// badge here, so that badge's one bucket must hold them all (servers/consoled.md, "Started by
/// `init`"). Their fids are charged to that bucket too, so it holds [`CONSOLE_FIDS`] for each of
/// those consoles and for `init`'s own.
pub const fn limits(buckets: u32) -> Limits {
    let consoles = redoubt_rt::abi::MAX_THREADS as u32;
    Limits { buckets, in_flight: 2, files: CONSOLE_FIDS * consoles, state: consoles, requests: 80, pages: 2 }
}

/// The fids one console client holds open: the root its namespace attaches
/// (`redoubt_client::ns::Namespace::from_startup`) and the `cons` file it opens for reading and
/// writing (`redoubt_client::console::Console::open`).
pub const CONSOLE_FIDS: u32 = 2;
/// What one of each costs, in bytes. A parked read, or a multiplexed connection's completion call,
/// holds its caller's lend, charged to this server until it replies (kernel/ipc.md R3), which is
/// `MAX_LEND_PAGES` pages at worst; a fid and a minted connection are small records; a multiplexed
/// request is its record, and a page its transfer brought a page.
pub const COST: Cost =
    Cost { in_flight: 64 * 1024, file: 256, state: 256, request: REQUEST_STATE, page: PAGE_SIZE as u64 };
/// The bytes of this server's budget its clients may use between them; its manifest entry gives
/// it the budget, and the program refuses limits that would not fit. A bucket at its caps costs
/// 2 parked calls at 64 KiB, `MAX_THREADS` connections at 256 bytes with 2 fids each at 256, 80
/// requests at 256 and 2 pages at 4 KiB, so one account-0 share holds 64 batched reads: 183 552
/// bytes with `MAX_THREADS` at 31, so 11 buckets fit, and 355 584 at 255, so 5 do; the image's
/// manifest asks for 4.
pub const BUDGET: u64 = 2 * 1024 * 1024;

/// Bytes of the prefix a minted connection's lines start with: `[con `, the id in 16 lowercase
/// hex digits, and `] ` (servers/consoled.md, "Started by `init`").
pub const PREFIX_LEN: usize = 23;

/// The prefix for the connection with `id`.
pub fn prefix(id: u64) -> [u8; PREFIX_LEN] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut p = *b"[con 0000000000000000] ";
    for (i, slot) in p[5..21].iter_mut().enumerate() {
        *slot = HEX[(id >> (60 - 4 * i)) as usize & 0xf];
    }
    p
}

/// Who is writing the console's current line, so that every line says who wrote it
/// (servers/consoled.md, "Started by `init`"). A line written through a minted connection starts
/// with that connection's [`prefix`]; one through a root badge goes out as it is. A write through
/// another badge than the one that left a line unfinished ends that line first, so no line holds
/// two writers' bytes.
///
/// The output is any sink that takes what it can of some bytes and says how many it took: the
/// UART in the program, a buffer in the tests. A sink that takes less leaves the line open with
/// as much of its prefix as went out, and the rest goes first next time: a prefix cut short and
/// finished by the writer's own bytes could spell another connection's id.
#[derive(Debug, Default)]
pub struct Lines {
    /// The line a writer left unfinished, if one did.
    open: Option<Open>,
}

/// An unfinished line: the badge it was written through, and how many bytes of its prefix went
/// out (all of them, for a line with none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Open {
    badge: u64,
    prefix: usize,
}

impl Lines {
    pub const fn new() -> Lines { Lines { open: None } }

    /// Writes `data` from the caller with `badge`, which is the minted connection `id` or, with
    /// `None`, a root badge; how many bytes of `data` went out.
    pub fn write(
        &mut self,
        badge: u64,
        id: Option<u64>,
        data: &[u8],
        out: &mut impl FnMut(&[u8]) -> usize,
    ) -> usize {
        if data.is_empty() {
            return 0;
        }
        if self.open.is_some_and(|open| open.badge != badge) {
            if out(b"\n") != 1 {
                return 0;
            }
            self.open = None;
        }
        let mut done = 0;
        while done < data.len() {
            if let Some(id) = id {
                let sent = self.open.map_or(0, |open| open.prefix);
                if sent < PREFIX_LEN {
                    let n = out(&prefix(id)[sent..]);
                    self.open = Some(Open { badge, prefix: sent + n });
                    if sent + n < PREFIX_LEN {
                        return done;
                    }
                }
            }
            let rest = &data[done..];
            let line = rest.iter().position(|&b| b == b'\n').map_or(rest.len(), |i| i + 1);
            let n = out(&rest[..line]);
            if n > 0 {
                self.open = Some(Open { badge, prefix: PREFIX_LEN });
            }
            done += n;
            if n < line {
                return done;
            }
            if rest[line - 1] == b'\n' {
                self.open = None;
            }
        }
        done
    }
}

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
    /// (badge, id) of every connection minted here and not yet gone: a write through one of these
    /// badges is prefixed with its id. A badge not here is a root badge, `init`'s own.
    minted: Vec<(u64, u64)>,
    /// Who is writing the current line.
    lines: Lines,
    /// The size `consol`'s `size` answers, the manifest's; with none, `size` and `resize` are
    /// refused. A UART has no window, so it never changes, and a parked `resize` waits until its
    /// caller gives up.
    pub size: Option<(u16, u16)>,
}

impl Console {
    pub fn new(uart: Uart) -> Console {
        Console {
            uart,
            input: VecDeque::new(),
            dropped: 0,
            minted: Vec::new(),
            lines: Lines::new(),
            size: None,
        }
    }

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

    /// Keeps the id the connection's requester was given, which its lines will carry.
    fn minted(
        &mut self,
        _: &Caller,
        badge: u64,
        id: u64,
        _root: &Cons,
        _quota: u64,
    ) -> Result<(), NineError> {
        self.minted.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
        self.minted.push((badge, id));
        Ok(())
    }

    /// A line it left unfinished stays so: the next writer ends it.
    fn disconnected(&mut self, badge: u64) { self.minted.retain(|(b, _)| *b != badge); }

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

    /// Output, every line of it saying who wrote it ([`Lines`]). The offset is ignored, as for
    /// a read. A byte the transmitter would not take is a short write, which 9P allows, rather
    /// than a server that spins.
    fn write(&mut self, caller: &Caller, _: &Cons, _offset: u64, data: &[u8]) -> Result<usize, NineError> {
        let id = self.minted.iter().find(|(b, _)| *b == caller.badge).map(|(_, id)| *id);
        let uart = &self.uart;
        Ok(self.lines.write(caller.badge, id, data, &mut |bytes| uart.put_all(bytes)))
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
