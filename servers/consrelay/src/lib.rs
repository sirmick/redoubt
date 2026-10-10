//! `consrelay`: a context's console, kept across SSH channels (servers/consrelay.md).
//!
//! The steward starts one beside each SSH context's VM, in the context's budget. It serves the
//! VM's `/dev/cons` on an endpoint of its own, and forwards it to whichever SSH channel the
//! context is attached to, which the steward names with `attach` and lets go with `detach` (the
//! `consrelay` protocol). While no channel is attached the VM's output is kept here, the newest
//! [`KEEP`] bytes, and replayed to the next channel.
//!
//! - [`Relay`] is everything the relay decides, with no system call in it: the VM's file, the kept output,
//!   the input, the console's size, and which channel each helper thread may use. The tests drive it alone.
//! - `src/bin/consrelay.rs` is the program: the serving thread, which owns the [`Relay`], and a reader, a
//!   writer and a sizer thread, which make the blocking calls on the channel's console so that a slow channel
//!   never holds the serving thread.
//!
//! **The bound** (TENETS.md rule 9). The output kept is at most [`KEEP`] bytes, allocated at the
//! start with the input's [`MAX_INPUT`], so a VM that later spends its budget's pages cannot
//! starve the relay. While a channel is attached a write waits for room, as on `sshd`'s console;
//! while none is, a write never waits: the oldest bytes go, and are counted. The next attach cuts
//! the kept bytes forward to the first line's end, so no half escape sequence leads them, and
//! says how many were dropped.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use redoubt_rt::ipc::Caller;
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, Qid, Read, Write, mode};

/// Output bytes kept while no channel is attached, and the most held for an attached one.
pub const KEEP: usize = 64 * 1024;
/// Input bytes held for the VM's reads.
pub const MAX_INPUT: usize = 4096;
/// The most bytes of a note the steward sends that are written: its notes are one line of a
/// context's name and a client's address, well inside this.
pub const NOTE_CAP: usize = 384;
/// What goes ahead of the kept output on an attach: the note and the drop count's line.
const HEAD_CAP: usize = NOTE_CAP + 64;

/// What the skeleton holds: the VM's console connection, its fids and its parked reads and
/// writes. Two buckets: the relay's own (account 0, through which it mints the VM's console) and
/// the context's account, as on `sshd`'s console.
pub const LIMITS: Limits = Limits { buckets: 2, in_flight: 4, files: 8, state: 2, requests: 64, pages: 4 };

/// What a fid rests on: there is one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File;

/// The one qid: a file whose version never changes.
pub fn qid() -> Qid { Qid { kind: 0, version: 0, path: 0 } }

/// The channel the context is attached to: its console's handle, by index, and its generation,
/// which every helper thread's call names, so that a thread still on a channel let go is told so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    pub generation: u64,
    pub handle: u32,
    /// The writer has opened the console, so the reader may use it too.
    pub ready: bool,
    /// No call on it has failed: once one has, output is kept as if detached until the steward
    /// says what happened.
    pub live: bool,
}

/// A helper thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thread {
    Reader,
    Writer,
    /// Asks the channel's console its size, then waits there for each change (`consol`).
    Sizer,
}

/// The helper threads, as many as [`Thread`] names.
const THREADS: usize = 3;

/// The size a console has before any channel says one: `sshd`'s for a channel without a pty.
pub const NO_SIZE: (u16, u16) = (80, 24);

/// What the writer is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// This many bytes, to write to its channel.
    Bytes(usize),
    /// Nothing yet: its call waits.
    Wait,
    /// Its channel is let go: it leaves it.
    Gone,
}

/// What the reader's bytes met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Give {
    /// This many were taken; with 0 its call waits for room.
    Taken(usize),
    /// Its channel is let go: the bytes are dropped, typed on a channel that no longer has the
    /// context.
    Gone,
}

/// The relay's state.
#[derive(Debug)]
pub struct Relay {
    /// The context's labels: the VM's file carries them.
    pub labels: Vec<u64>,
    input: VecDeque<u8>,
    output: VecDeque<u8>,
    /// Output bytes dropped since the last attach.
    dropped: u64,
    /// What goes to the attached channel ahead of `output`.
    head: VecDeque<u8>,
    channel: Option<Channel>,
    /// The generation of the channel let go last, while its writer has not written the note
    /// for it, and what is left of that note.
    farewell: Option<u64>,
    note: VecDeque<u8>,
    /// Channels let go, by (generation, handle), whose handle a thread may still be on.
    retired: Vec<(u64, u32)>,
    /// The channel detached last without a note, while no channel has been attached since: its
    /// reader's last input is still the VM's.
    closed: Option<u64>,
    /// The attached channel's input has ended (a channel without a pty): once what it gave is
    /// read, the VM's reads find the end of the file.
    input_over: bool,
    /// The generation each thread is on, from its being given it until it asks for the next.
    on: [Option<u64>; THREADS],
    /// The generation each thread was given last.
    last: [u64; THREADS],
    generation: u64,
    /// The console's size, the attached channel's last answer, and how many times it has changed:
    /// a VM's parked `resize` waits for the count to move.
    size: (u16, u16),
    resized: u64,
    /// The channel whose size is known: its first answer is a change, so a VM is told its new
    /// terminal on every attach.
    sized: u64,
}

/// No memory for the relay's buffers at its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoMemory;

fn reserved(n: usize) -> Result<VecDeque<u8>, NoMemory> {
    let mut q = VecDeque::new();
    q.try_reserve_exact(n).map_err(|_| NoMemory)?;
    Ok(q)
}

impl Relay {
    /// A relay with nothing attached, its buffers allocated.
    pub fn new(labels: Vec<u64>) -> Result<Relay, NoMemory> {
        Ok(Relay {
            labels,
            input: reserved(MAX_INPUT)?,
            output: reserved(KEEP)?,
            dropped: 0,
            head: reserved(HEAD_CAP)?,
            channel: None,
            farewell: None,
            note: reserved(NOTE_CAP)?,
            retired: Vec::new(),
            closed: None,
            input_over: false,
            on: [None; THREADS],
            last: [0; THREADS],
            generation: 0,
            size: NO_SIZE,
            resized: 0,
            sized: 0,
        })
    }

    /// The channel attached, if any.
    pub fn channel(&self) -> Option<Channel> { self.channel }

    /// Output bytes kept and not yet given to a channel.
    pub fn kept(&self) -> usize { self.output.len() }

    /// Output bytes dropped since the last attach.
    pub fn dropped(&self) -> u64 { self.dropped }

    /// Whether a VM read waiting now could be answered.
    pub fn readable(&self) -> bool { !self.input.is_empty() || self.input_over }

    /// `attach`: the channel whose console is `handle` takes the context, after `note`, the
    /// drop count if any were dropped, and the output kept. The channel attached before, if any,
    /// is let go without a word: the steward detaches it first when it has one to say.
    pub fn attach(&mut self, handle: u32, note: &str) {
        // A takeover: what was typed on the old channel and not yet read is not the new one's.
        if self.channel.is_some() {
            self.input.clear();
        }
        self.retire();
        self.closed = None;
        self.generation += 1;
        self.head.clear();
        push_capped(&mut self.head, note.as_bytes(), NOTE_CAP);
        if self.dropped > 0 {
            if let Some(end) = self.output.iter().position(|&b| b == b'\n') {
                self.output.drain(..=end);
                self.dropped += end as u64 + 1;
            }
            let mut line = Line::default();
            let _ = write!(line, "[{} bytes of output dropped while detached]\r\n", self.dropped);
            let room = HEAD_CAP - self.head.len();
            push_capped(&mut self.head, line.bytes(), room);
            self.dropped = 0;
        }
        let generation = self.generation;
        self.channel = Some(Channel { generation, handle, ready: false, live: true });
    }

    /// `detach`: the channel attached, if any, is let go after `note`. Whether the call waits
    /// for the note to be written: only if there is a note and a writer on a channel that works.
    pub fn detach(&mut self, note: &str) -> bool {
        let Some(channel) = self.channel else { return false };
        // A note is a takeover's: what was typed on the old channel and not yet read, or what its
        // reader brings in later, is not the new one's.
        self.closed = note.is_empty().then_some(channel.generation);
        if !note.is_empty() {
            self.input.clear();
        }
        let waits = !note.is_empty() && channel.ready && channel.live;
        self.note.clear();
        self.farewell = None;
        if waits {
            push_capped(&mut self.note, note.as_bytes(), NOTE_CAP);
            self.farewell = Some(channel.generation);
        }
        self.retire();
        waits
    }

    /// The note for the channel let go last is written, or will never be.
    fn end_farewell(&mut self) {
        self.farewell = None;
        self.note.clear();
    }

    /// Whether a detach's note is still to be written.
    pub fn farewell_pending(&self) -> bool { self.farewell.is_some() }

    /// Lets the current channel go. What was typed on it and not yet read stays for the VM: the
    /// last line a person typed before closing the terminal (`exit`) still counts.
    fn retire(&mut self) {
        self.input_over = false;
        if let Some(channel) = self.channel.take() {
            self.retired.push((channel.generation, channel.handle));
            self.head.clear();
        }
    }

    /// The channel `thread` may use next, newer than the one it was given last, which it has
    /// left: the writer is given one at once, the reader and the sizer once the writer has opened
    /// it.
    pub fn next(&mut self, thread: Thread) -> Option<Channel> {
        if thread == Thread::Writer
            && self.farewell.is_some()
            && self.farewell == self.on[Thread::Writer as usize]
        {
            self.end_farewell();
        }
        self.on[thread as usize] = None;
        let channel = self.channel?;
        let usable =
            channel.generation > self.last[thread as usize] && (thread == Thread::Writer || channel.ready);
        if !usable {
            return None;
        }
        self.on[thread as usize] = Some(channel.generation);
        self.last[thread as usize] = channel.generation;
        Some(channel)
    }

    /// The console's size, for `consol`'s `size`.
    pub fn size(&self) -> (u16, u16) { self.size }

    /// How many times the size has changed, which a parked `resize` waits to move from.
    pub fn resized(&self) -> u64 { self.resized }

    /// The sizer's answer from channel `generation`: a change if it differs, or if it is that
    /// channel's first. `false` if the channel is not the attached one, whose sizer leaves it.
    pub fn sized(&mut self, generation: u64, size: (u16, u16)) -> bool {
        if self.channel.is_none_or(|c| c.generation != generation) {
            return false;
        }
        if self.sized != generation || self.size != size {
            self.sized = generation;
            self.size = size;
            self.resized += 1;
        }
        true
    }

    /// Handles of channels let go that no thread is on any more: the caller closes them.
    pub fn closable(&mut self) -> Vec<u32> {
        let on = self.on;
        let mut gone = Vec::new();
        self.retired.retain(|(generation, handle)| {
            let used = on.contains(&Some(*generation));
            if !used {
                gone.push(*handle);
            }
            used
        });
        gone
    }

    /// The writer opened channel `generation`'s console, or could not (`opened` false).
    pub fn opened(&mut self, generation: u64, opened: bool) {
        if let Some(channel) = self.channel.as_mut().filter(|c| c.generation == generation) {
            channel.ready = opened;
            channel.live &= opened;
        }
    }

    /// A call on channel `generation` failed: until the steward says what
    /// happened, output is kept as if no channel were attached.
    pub fn broken(&mut self, generation: u64) {
        if self.farewell == Some(generation) {
            self.end_farewell();
        }
        if let Some(channel) = self.channel.as_mut().filter(|c| c.generation == generation) {
            channel.live = false;
        }
    }

    /// Channel `generation`'s input ended: `sshd` gives the end of a file only on a channel without
    /// a pty, and on any channel once it has ended, which the steward's detach comes before. If
    /// the channel is still the one attached, the VM reads to the end of what it gave and then
    /// the end of the file, as from a pipe.
    pub fn input_ended(&mut self, generation: u64) {
        if self.channel.is_some_and(|c| c.generation == generation) {
            self.input_over = true;
        }
    }

    /// Output for the writer on channel `generation`, into `out`: a detached channel's note, or
    /// an attached one's head and then the output kept.
    pub fn take_output(&mut self, generation: u64, out: &mut [u8]) -> Take {
        if self.farewell == Some(generation) {
            if self.note.is_empty() {
                self.end_farewell();
                return Take::Gone;
            }
            return Take::Bytes(drain_into(&mut self.note, out));
        }
        match self.channel {
            Some(c) if c.generation == generation && c.live => {
                let n = drain_into(&mut self.head, out);
                let n = n + drain_into(&mut self.output, &mut out[n..]);
                if n == 0 { Take::Wait } else { Take::Bytes(n) }
            }
            _ => Take::Gone,
        }
    }

    /// Input the reader read from channel `generation`, as much as there is room for. A
    /// channel detached without a note (its terminal closed) still gives what its reader read
    /// before the detach reached the relay, while no channel takes its place: the line typed just
    /// before the terminal closed (`exit`) is the VM's.
    pub fn give_input(&mut self, generation: u64, bytes: &[u8]) -> Give {
        let ours = match self.channel {
            Some(c) => c.generation == generation,
            None => self.closed == Some(generation),
        };
        if !ours {
            return Give::Gone;
        }
        let n = MAX_INPUT.saturating_sub(self.input.len()).min(bytes.len());
        self.input.extend(&bytes[..n]);
        Give::Taken(n)
    }

    /// Whether the VM's writes wait for room: only while a working channel is attached.
    fn attached(&self) -> bool { self.channel.is_some_and(|c| c.live) }

    /// The VM's output: while a working channel is attached, what there is room for, or a wait;
    /// otherwise all of it, the oldest kept bytes dropped and counted to make room.
    pub fn output(&mut self, data: &[u8]) -> Write {
        if self.attached() {
            let n = KEEP.saturating_sub(self.output.len()).min(data.len());
            if n == 0 && !data.is_empty() {
                return Write::Wait;
            }
            self.output.extend(&data[..n]);
            return Write::Done(n);
        }
        let kept = &data[data.len().saturating_sub(KEEP)..];
        let over = (self.output.len() + kept.len()).saturating_sub(KEEP);
        self.output.drain(..over);
        self.output.extend(kept);
        self.dropped += (over + data.len() - kept.len()) as u64;
        Write::Done(data.len())
    }

    /// Input for the VM's read, or a wait while there is none: a console has no end, but the
    /// attached channel's input may ([`Relay::input_ended`]).
    pub fn input(&mut self, out: &mut [u8]) -> Read {
        if out.is_empty() {
            return Read::Done(0);
        }
        match drain_into(&mut self.input, out) {
            0 if self.input_over => Read::Done(0),
            0 => Read::Wait,
            n => Read::Done(n),
        }
    }
}

/// The VM's `/dev/cons`, one file. The VM reaches it only through the connection the relay
/// minted for it at its start; an attach through any other badge is refused, as on `sshd`'s
/// console.
impl FileServer for Relay {
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

    fn read(&mut self, _: &Caller, _: &File, _offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        Ok(self.input(out))
    }

    fn write(&mut self, caller: &Caller, node: &File, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        match self.write_or_wait(caller, node, offset, data)? {
            Write::Done(n) => Ok(n),
            Write::Wait => Ok(0),
        }
    }

    fn write_or_wait(&mut self, _: &Caller, _: &File, _offset: u64, data: &[u8]) -> Result<Write, NineError> {
        Ok(self.output(data))
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

/// The drop count's line, made on the stack: an attach allocates nothing.
struct Line {
    bytes: [u8; 64],
    len: usize,
}

impl Default for Line {
    fn default() -> Line { Line { bytes: [0; 64], len: 0 } }
}

impl Line {
    fn bytes(&self) -> &[u8] { &self.bytes[..self.len] }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Moves what fits of `from` into `out`; how many bytes.
fn drain_into(from: &mut VecDeque<u8>, out: &mut [u8]) -> usize {
    let n = out.len().min(from.len());
    for (slot, byte) in out.iter_mut().zip(from.drain(..n)) {
        *slot = byte;
    }
    n
}

/// Appends at most `cap` bytes of `bytes` to `to`, which has the room reserved.
fn push_capped(to: &mut VecDeque<u8>, bytes: &[u8], cap: usize) { to.extend(&bytes[..bytes.len().min(cap)]); }
