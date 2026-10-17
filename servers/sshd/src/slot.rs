//! A slot: how many there are, how long one is held before login, and its reader's end
//! (servers/sshd.md, "The box's platform"). Here, not in the program, so the host tests drive it.
//!
//! - **The count is the manifest's.** `sshd`'s entry carries `buckets=N` as every shared server's does, and N
//!   is its slots: connections served at once, each a channel for one (account, label set) the manifest
//!   declares, which is what `init` sizes N to (servers/init.md, "Sizing"). Each slot's two threads' stacks,
//!   and the watcher's, must fit the entry's budget ([`slots`]); a count they do not fit is refused at start,
//!   as every server refuses an entry it cannot hold. Each slot also keeps one read parked at `ipd` beside
//!   the listener's accept, so N + 1 is at most the parked calls `ipd`'s bucket for `sshd` holds
//!   (servers/ipd.md, "Sizing"): the manifest's business, held by a host test over every manifest in the
//!   tree, since `sshd` does not read `ipd`'s entry.
//! - **Before login a slot is held for a bounded time** ([`Gate`]): a connection that has not logged in
//!   within [`LOGIN_US`] is closed, silently, and the slot serves the next. After login the connection is a
//!   channel, bounded by its session.
//! - **The reader's end.** The reader thread calls the slot's driver with `DATA` for each read and `EOF` once
//!   the socket ends, once per connection. The driver may take that `EOF` while it still runs the connection
//!   (a client that hangs up first) or only after it has closed the socket (a connection the server ended).
//!   The slot is free once the reader's `EOF` is taken, and not before: the reader would otherwise call into
//!   the slot's next connection.

use redoubt_rt::abi::PAGE_SIZE;

/// A driver's stack, in pages: `sunset` moves its exchange state by value a few times (the
/// crate's header). A whole login, client and server in one host thread, needs more than 88 KiB
/// and fits 96 KiB (x86_64, release); twice that.
pub const DRIVER_STACK_PAGES: usize = 48;
/// A reader's: it holds one read's buffer and a lend.
pub const READER_STACK_PAGES: usize = 4;
/// The watcher's: one `watch` call at the steward, over and over.
pub const WATCHER_STACK_PAGES: usize = 4;

/// The longest a connection is held before it has logged in, in µs: twice the slowest login the
/// bench records, rounded up to a second. Measured from `ssh`'s start to the shell's prompt, one
/// run of each case on each width: `launch-system` 2.01 s (rv64) and 1.70 s (rv32);
/// `steward-ssh-two-principals` 2.00/1.78 s for alice and, beside her running session,
/// 2.41/2.06 s for bob. Twice 2.41 s is 4.82 s; 5 s.
pub const LOGIN_US: u64 = 5_000_000;

/// The pages the threads of `slots` slots and the watcher take for their stacks, which the
/// entry's budget pays beyond its first thread's stack and its heap cap.
pub const fn stack_pages(slots: usize) -> usize {
    slots * (DRIVER_STACK_PAGES + READER_STACK_PAGES) + WATCHER_STACK_PAGES
}

/// The entry's slots cannot be held: their stacks do not fit its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsized {
    /// The slots asked for.
    pub slots: usize,
    /// What their stacks need, in pages.
    pub need: usize,
    /// What the budget holds, in pages.
    pub have: usize,
}

/// The slots an entry of `buckets` buckets serves, if their stacks fit `budget` bytes
/// ([`redoubt_rt::server::budget`]: the entry's pages less its heap cap and its first stack).
pub fn slots(buckets: u32, budget: u64) -> Result<usize, Unsized> {
    let slots = buckets as usize;
    let need = stack_pages(slots);
    let have = usize::try_from(budget / PAGE_SIZE as u64).unwrap_or(usize::MAX);
    if need <= have { Ok(slots) } else { Err(Unsized { slots, need, have }) }
}

/// The bound on a connection before login: from its start until it has logged in, there is a
/// moment past which it is closed; after login there is none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    deadline: Option<u64>,
}

impl Gate {
    /// A connection that started at `now` (µs).
    pub fn new(now: u64) -> Gate { Gate { deadline: Some(now.saturating_add(LOGIN_US)) } }

    /// The connection has logged in: the bound no longer runs.
    pub fn logged_in(&mut self) { self.deadline = None; }

    /// When the connection is closed unless it has logged in by then; `None` once it has.
    pub fn deadline(&self) -> Option<u64> { self.deadline }

    /// Whether the connection is to be closed now: it has not logged in and the bound has passed.
    pub fn due(&self, now: u64) -> bool { self.deadline.is_some_and(|d| now >= d) }
}

/// Word 0 of the reader's call: its bytes are in the lend.
pub const DATA: u64 = 2;
/// Word 0 of the reader's last call for a connection: the socket ended.
pub const EOF: u64 = 3;

/// What a reader's call says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// Bytes, `n` of them in the call's lend.
    Data,
    /// The socket ended: the reader's last call for this connection.
    End,
}

/// One connection's reader, as its driver has heard it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reader {
    ended: bool,
}

impl Reader {
    /// The reader's call with word 0 `word`. Anything but [`DATA`] ends it, as its last call.
    pub fn took(&mut self, word: u64) -> Read {
        if word == DATA {
            Read::Data
        } else {
            self.ended = true;
            Read::End
        }
    }

    /// Whether the driver must still wait for the reader's last call before the slot is free:
    /// only if it has not taken it already.
    pub fn waits(&self) -> bool { !self.ended }
}
