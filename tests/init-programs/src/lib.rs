//! What the test programs `init` starts share: their console, the requests their servers answer,
//! and an empty file server. Each writes its lines through the `/dev/cons` connection `init`
//! minted for it, so `consoled` starts every one of them with that connection's id
//! (servers/consoled.md, "Started by `init`"), and the bench attributes a verdict by it
//! (docs/testbench.md, "The servers' cases under `init`"). The programs a tester starts in
//! `init`'s place share the rest.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;

use redoubt_client::console::Console;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, NineError, QTDIR, Qid, Read};
use redoubt_rt::startup::Startup;

/// Pages lent with each call: a console line or a read of a small entry.
pub const LEND_PAGES: usize = 1;

/// Exit codes: the program could not reach its console, so it had no line to say why.
pub mod code {
    pub const NO_LEND: u32 = 2;
    pub const NO_CONSOLE: u32 = 3;
}

/// The requests `restartee` serves beside 9P and `ninep_common`, by opcode, above theirs; each
/// is a call of four words, the opcode first.
pub mod restartee {
    /// Answers with a copy of its own console connection.
    pub const CONSOLE: u64 = 100;
    /// Answers with a handle it does not hold, so the kernel rejects the reply.
    pub const MISREPLY: u64 = 101;
    /// Held until a [`WAIT`] is held too; then served, and it faults while it serves it.
    pub const FAULT: u64 = 102;
    /// Answers, and nothing more.
    pub const PING: u64 = 103;
    /// Held, unanswered, until the fault: the caller waits for it there.
    pub const WAIT: u64 = 104;
    /// The words of an answer: status 0.
    pub const OK: [u64; 4] = [0; 4];
}

/// The requests `dma-driver` serves, each a call of four words, the opcode first.
pub mod dma_driver {
    /// Faults while it holds the call.
    pub const FAULT: u64 = 1;
    /// Answers [`OK`]: the instance that answers holds its DMA run.
    pub const PING: u64 = 2;
    /// The words of an answer: status 0.
    pub const OK: [u64; 4] = [0; 4];
}

/// The requests `orphan-server` serves beside 9P and `ninep_common`, and what its tester and the
/// programs it starts send each other, each four words, the opcode first.
pub mod orphan {
    /// To `orphan-server`: answers [`OK`] with the connections it minted and has not
    /// disconnected in word 1.
    pub const COUNT: u64 = 100;
    /// From the launcher to its tester, a call: it has minted the child's connection and handed
    /// it over. The answer tells it to fault.
    pub const MINTED: u64 = 1;
    /// From the child to its tester: it attached through the connection it was handed, word 1
    /// 1 if it could.
    pub const ATTACHED: u64 = 2;
    /// From the tester to the child: try the connection again.
    pub const AGAIN: u64 = 3;
    /// From the child to its tester: what trying again gave, word 1 1 if it was refused.
    pub const TRIED: u64 = 4;
    /// The words of an answer: status 0.
    pub const OK: [u64; 4] = [0; 4];
}

/// What `keeper` sends `passer`, four words, the opcode first.
pub mod passer {
    /// Exit: the next instance passes a badge of its own.
    pub const EXIT: u64 = 1;
}

/// A file server with one empty directory: a connection needs a root, nothing more.
pub struct Empty;

const ROOT: Qid = Qid { kind: QTDIR, version: 0, path: 0 };

impl FileServer for Empty {
    type Node = ();

    fn attach(&mut self, _: &Caller, _: &str) -> Result<((), Qid), NineError> { Ok(((), ROOT)) }

    fn labels(&self, _: &()) -> &[u64] { &[] }

    fn walk(&mut self, _: &Caller, _: &(), _: &str) -> Result<((), Qid), NineError> {
        Err(NineError::NOT_FOUND)
    }

    fn open(&mut self, _: &Caller, _: &(), _: u8) -> Result<Qid, NineError> { Ok(ROOT) }

    fn read(&mut self, _: &Caller, _: &(), _: u64, _: &mut [u8]) -> Result<Read, NineError> {
        Ok(Read::Done(0))
    }

    fn write(&mut self, _: &Caller, _: &(), _: u64, _: &[u8]) -> Result<usize, NineError> {
        Err(NineError::NOT_SUPPORTED)
    }

    fn stat(&mut self, _: &Caller, _: &()) -> Result<FileStat, NineError> {
        Ok(FileStat { qid: ROOT, mode: DMDIR | 0o555, mtime: 0, length: 0, name: String::from("/") })
    }

    fn dir_entry(&mut self, _: &Caller, _: &(), _: u64) -> Result<Option<((), FileStat)>, NineError> {
        Ok(None)
    }
}

/// Where a program ends once it has said its lines: `init` restarts a server that exits, so a
/// program whose exit is not the point of its case never exits.
pub fn park() -> ! {
    loop {
        let _ = redoubt_rt::handle::sleep(redoubt_rt::abi::FOREVER);
    }
}

/// The console `init` gave the program, and the lend its calls use.
pub struct Out {
    pub lend: Lend,
    /// For the console's other calls.
    pub console: Console,
}

impl Out {
    /// `/dev/cons` from the block's namespace.
    pub fn open(startup: &Startup) -> Result<Out, u32> {
        let mut lend = Lend::new(LEND_PAGES).map_err(|_| code::NO_LEND)?;
        let ns = Namespace::from_startup(startup, &mut lend).map_err(|_| code::NO_CONSOLE)?;
        let console = Console::open(&ns, &mut lend).map_err(|_| code::NO_CONSOLE)?;
        Ok(Out { lend, console })
    }

    /// Writes all of `text`, however many writes that takes.
    pub fn say(&mut self, text: &str) -> Result<(), Error> {
        let mut rest = text.as_bytes();
        while !rest.is_empty() {
            match self.console.write(&mut self.lend, rest)? {
                0 => return Err(Error::Rerror(redoubt_client::Name::Other)),
                n => rest = &rest[n..],
            }
        }
        Ok(())
    }
}
