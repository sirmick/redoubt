//! The box platform's console for one channel (servers/sshd.md, "A pty session"): the session's
//! `/dev/cons`, served over 9P on the connection's own endpoint as `consoled` serves the UART's,
//! and the [`Session`] the core moves the channel's bytes through. The two share one [`Chan`]
//! on the connection's one thread.
//!
//! - **Input** is what the client typed, held for the session's reads up to [`MAX_INPUT`]; a read with none
//!   waits ([`Read::Wait`]), and the core is told how much was taken, so the rest stays in SSH's window. An
//!   interrupt is the session's own key, Ctrl+\ (0x1C), which the shell never forwards, so a full-screen
//!   program that takes Ctrl+C as a key cannot take the interrupt too.
//! - **Output** is what the session wrote, held for the channel up to [`MAX_OUTPUT`]; a write with no room
//!   waits ([`Write::Wait`]) until the channel has taken some.
//! - **The end.** Once the session has ended ([`Chan::end`]), a read finds the end of the file and a write is
//!   refused; the core then sends the client the status, EOF and close.
//! - **Labels.** The file carries the channel's labels, the session's, so the skeleton's label check
//!   (servers/serving.md R25) lets only that session read and write it.

use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use redoubt_rt::ipc::Caller;
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, Qid, Read, Write, mode};

use crate::{Session, Window};

/// Input bytes held for the session's reads.
pub const MAX_INPUT: usize = 4096;
/// Output bytes held for the channel.
pub const MAX_OUTPUT: usize = 16 * 1024;
/// The session's own key, Ctrl+\: the interrupt the shell keeps whatever is in front
/// (userland/shell.md, "Interrupting and killing jobs").
const INTERRUPT: u8 = 0x1C;

/// One channel's console, shared by its file and its session.
#[derive(Debug, Default)]
pub struct Chan {
    input: VecDeque<u8>,
    output: VecDeque<u8>,
    /// The client sends no more.
    input_ended: bool,
    /// The session's exit status, once it has ended.
    ended: Option<u32>,
    /// The pty's size, if the client asked for one.
    pub window: Option<Window>,
}

impl Chan {
    /// The session has ended with `status`: its VM is gone, or the steward ended it.
    pub fn end(&mut self, status: u32) {
        if self.ended.is_none() {
            self.ended = Some(status);
        }
    }

    /// Whether a read waiting now could be answered.
    pub fn readable(&self) -> bool { !self.input.is_empty() || self.input_ended || self.ended.is_some() }

    /// Whether a write waiting now could be answered.
    pub fn writable(&self) -> bool { self.output.len() < MAX_OUTPUT || self.ended.is_some() }
}

/// What the file and the session hold of the channel.
pub type Shared = Rc<RefCell<Chan>>;

/// What one channel's skeleton holds: the session's console connection, its fids and its parked
/// reads and writes. Two buckets: the platform's own (account 0, through which it mints the
/// console) and the session's account and labels.
pub const LIMITS: Limits = Limits { buckets: 2, in_flight: 4, files: 8, state: 2, requests: 64, pages: 4 };

/// The session's `/dev/cons`, one file. A connection reaches it only through the connection the
/// platform minted for the session at its login; an attach through any other badge is refused.
pub struct Cons {
    pub chan: Shared,
    /// The channel's labels, the session's, sorted as the kernel sorts a budget's.
    pub labels: Vec<u64>,
}

/// What a fid rests on: there is one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File;

/// The one qid: a file whose version never changes.
pub fn qid() -> Qid { Qid { kind: 0, version: 0, path: 0 } }

impl FileServer for Cons {
    type Node = File;

    fn attach(&mut self, _: &Caller, _: &str) -> Result<(File, Qid), NineError> { Err(NineError::PERMISSION) }

    fn labels(&self, _: &File) -> &[u64] { &self.labels }

    fn walk(&mut self, _: &Caller, _: &File, _: &str) -> Result<(File, Qid), NineError> {
        Err(NineError::NOT_DIR)
    }

    fn open(&mut self, _: &Caller, _: &File, open_mode: u8) -> Result<Qid, NineError> {
        if open_mode & mode::OTRUNC != 0 || !matches!(open_mode & 3, mode::OREAD | mode::OWRITE | mode::ORDWR)
        {
            return Err(NineError::BAD_MODE);
        }
        Ok(qid())
    }

    /// Input, the end of the file once the session or the client has ended, or a wait.
    fn read(&mut self, _: &Caller, _: &File, _offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        let mut chan = self.chan.borrow_mut();
        if out.is_empty() {
            return Ok(Read::Done(0));
        }
        if chan.input.is_empty() {
            return Ok(if chan.input_ended || chan.ended.is_some() { Read::Done(0) } else { Read::Wait });
        }
        let n = out.len().min(chan.input.len());
        for (slot, byte) in out.iter_mut().zip(chan.input.drain(..n)) {
            *slot = byte;
        }
        Ok(Read::Done(n))
    }

    fn write(&mut self, caller: &Caller, node: &File, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        match self.write_or_wait(caller, node, offset, data)? {
            Write::Done(n) => Ok(n),
            Write::Wait => Ok(0),
        }
    }

    /// Output for the channel, as much as there is room for, or a wait when there is none.
    fn write_or_wait(&mut self, _: &Caller, _: &File, _offset: u64, data: &[u8]) -> Result<Write, NineError> {
        let mut chan = self.chan.borrow_mut();
        if chan.ended.is_some() {
            return Err(NineError::NO_CONNECTION);
        }
        if data.is_empty() {
            return Ok(Write::Done(0));
        }
        let room = MAX_OUTPUT.saturating_sub(chan.output.len());
        if room == 0 {
            return Ok(Write::Wait);
        }
        let n = room.min(data.len());
        chan.output.try_reserve(n).map_err(|_| NineError::NO_MEMORY)?;
        chan.output.extend(&data[..n]);
        Ok(Write::Done(n))
    }

    /// Length 0: a console has no size.
    fn stat(&mut self, _: &Caller, _: &File) -> Result<FileStat, NineError> {
        let mut name = String::new();
        name.try_reserve(4).map_err(|_| NineError::NO_MEMORY)?;
        name.push_str("cons");
        Ok(FileStat { qid: qid(), mode: 0o666, mtime: 0, length: 0, name })
    }

    fn dir_entry(&mut self, _: &Caller, _: &File, _: u64) -> Result<Option<(File, FileStat)>, NineError> {
        Ok(None)
    }
}

/// The session the core holds for the channel: its console's other side.
pub struct Console {
    pub chan: Shared,
    /// The steward's id for the session.
    pub id: u64,
    labelled: bool,
}

impl Console {
    pub fn new(chan: Shared, id: u64, labelled: bool) -> Console { Console { chan, id, labelled } }
}

impl Session for Console {
    fn labelled(&self) -> bool { self.labelled }

    fn start(&mut self, pty: Option<Window>) { self.chan.borrow_mut().window = pty; }

    fn input(&mut self, bytes: &[u8]) -> usize {
        let mut chan = self.chan.borrow_mut();
        let n = MAX_INPUT.saturating_sub(chan.input.len()).min(bytes.len());
        if chan.input.try_reserve(n).is_err() {
            return 0;
        }
        chan.input.extend(&bytes[..n]);
        n
    }

    fn input_ended(&mut self) { self.chan.borrow_mut().input_ended = true; }

    fn output(&mut self, buf: &mut [u8]) -> usize {
        let mut chan = self.chan.borrow_mut();
        let n = buf.len().min(chan.output.len());
        for (slot, byte) in buf.iter_mut().zip(chan.output.drain(..n)) {
            *slot = byte;
        }
        n
    }

    fn window(&mut self, w: Window) { self.chan.borrow_mut().window = Some(w); }

    /// The interrupt reaches the session as its own key's byte; with no room for it the input is
    /// full, and the session is not reading.
    fn interrupt(&mut self) { let _ = self.input(&[INTERRUPT]); }

    /// The core sends the status once the output held here has gone.
    fn ended(&self) -> Option<u32> { self.chan.borrow().ended }
}
