//! The system calls of KERNEL-SPEC.md as data, their results, and the other things that can
//! happen to the machine (a user memory access, a fault, an interrupt, time passing).
//!
//! Every argument is a raw `u64` (or a list of them), as it arrives from user mode: a handle is
//! an index into the caller's table, an address is a user virtual address, a class is its
//! `redoubt-sys` tag. The kernel model validates all of them; no value can make it panic (I14).
//! Record contents are fields here; `Record` independently abstracts ownership/permissions
//! and completion copying, including real modeled mappings through `Record::Memory`.

use alloc::vec::Vec;

use crate::spec::{Cause, Counters, Error, WORDS};

/// A buffer argument: a lend (`call`) or a transfer (`send`), as a page range (`redoubt-sys`
/// `Pages`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub addr: u64,
    pub npages: u64,
}

/// `mint`'s `source`: a message id the caller is serving, or a badge-0 endpoint handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MintSource {
    Message(u64),
    Handle(u64),
}

/// One system call with its arguments, in KERNEL-SPEC.md's table order and argument order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Syscall {
    MapAnon {
        len: u64,
        flags: u64,
    },
    Unmap {
        addr: u64,
        len: u64,
    },
    SetFlags {
        addr: u64,
        len: u64,
        flags: u64,
    },
    MapDevice {
        h: u64,
    },
    DmaAlloc {
        h: u64,
        npages: u64,
    },
    ThreadCreate {
        entry: u64,
        sp: u64,
        arg: u64,
    },
    ThreadExit,
    ProcessExit {
        code: u64,
    },
    ProcessCreate {
        budget: u64,
        exit_endpoint: u64,
    },
    ProcessMap {
        process: u64,
        src: u64,
        dst: u64,
        len: u64,
        flags: u64,
    },
    /// `arg` is the address of the child's startup page, 0 for none (QUESTIONS 40).
    ProcessStart {
        process: u64,
        entry: u64,
        sp: u64,
        arg: u64,
        handles: Vec<u64>,
    },
    EndpointCreate,
    Mint {
        source: MintSource,
        badge: u64,
        budget: Option<u64>,
    },
    Call {
        h: u64,
        words: [u64; WORDS],
        handles: Vec<u64>,
        lend: Option<Buffer>,
        timeout: u64,
    },
    Send {
        h: u64,
        words: [u64; WORDS],
        handles: Vec<u64>,
        transfer: Option<Buffer>,
        timeout: u64,
    },
    /// `max_transfer` is in pages (as in `redoubt-sys`).
    Receive {
        h: Option<u64>,
        timeout: u64,
        max_transfer: u64,
    },
    Reply {
        msg_id: u64,
        words: [u64; WORDS],
        handles: Vec<u64>,
    },
    /// `serve(msg_id)`: the open call becomes the thread's current call.
    Serve {
        msg_id: u64,
    },
    HandleClose {
        h: u64,
    },
    BudgetCreate {
        parent: u64,
        pages: u64,
        processes: u64,
        weight: u64,
        labels: Vec<u64>,
        account: u64,
        /// Absolute time in µs; `FOREVER` means none.
        deadline: u64,
    },
    BudgetDestroy {
        h: u64,
    },
    BudgetUsage {
        h: u64,
    },
    TimeNow,
    Random,
    SystemReset {
        h: u64,
        kind: u64,
    },
    /// As `MapAnon`, but at exactly `addr`; never replaces a mapping (KERNEL-SPEC.md R11,
    /// answer 172). Appended last so earlier call numbers keep their values.
    MapFixed {
        addr: u64,
        len: u64,
        flags: u64,
    },
}

/// The calls' names, in KERNEL-SPEC.md's table order (the order of `redoubt-sys`'s numbers,
/// from 1). The one list of them: [`Syscall::name`], the trace and the tests use it.
pub const CALL_NAMES: [&str; 26] = [
    "map_anon",
    "unmap",
    "set_flags",
    "map_device",
    "dma_alloc",
    "thread_create",
    "thread_exit",
    "process_exit",
    "process_create",
    "process_map",
    "process_start",
    "endpoint_create",
    "mint",
    "call",
    "send",
    "receive",
    "reply",
    "serve",
    "handle_close",
    "budget_create",
    "budget_destroy",
    "budget_usage",
    "time_now",
    "random",
    "system_reset",
    "map_fixed",
];

impl Syscall {
    /// The call's number (`redoubt-sys`'s, from 1).
    pub fn number(&self) -> usize {
        match self {
            Syscall::MapAnon { .. } => 1,
            Syscall::Unmap { .. } => 2,
            Syscall::SetFlags { .. } => 3,
            Syscall::MapDevice { .. } => 4,
            Syscall::DmaAlloc { .. } => 5,
            Syscall::ThreadCreate { .. } => 6,
            Syscall::ThreadExit => 7,
            Syscall::ProcessExit { .. } => 8,
            Syscall::ProcessCreate { .. } => 9,
            Syscall::ProcessMap { .. } => 10,
            Syscall::ProcessStart { .. } => 11,
            Syscall::EndpointCreate => 12,
            Syscall::Mint { .. } => 13,
            Syscall::Call { .. } => 14,
            Syscall::Send { .. } => 15,
            Syscall::Receive { .. } => 16,
            Syscall::Reply { .. } => 17,
            Syscall::Serve { .. } => 18,
            Syscall::HandleClose { .. } => 19,
            Syscall::BudgetCreate { .. } => 20,
            Syscall::BudgetDestroy { .. } => 21,
            Syscall::BudgetUsage { .. } => 22,
            Syscall::TimeNow => 23,
            Syscall::Random => 24,
            Syscall::SystemReset { .. } => 25,
            Syscall::MapFixed { .. } => 26,
        }
    }

    /// The spec's name for this call.
    pub fn name(&self) -> &'static str { CALL_NAMES[self.number() - 1] }
}

/// How a message was sent. `receive` returns it (QUESTIONS 1): a `call`'s message is owed a
/// reply and may carry a lend; a `send`'s may carry a transfer and cannot be replied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MsgKind {
    Call,
    Send,
}

/// A delivered message, as `receive` returns it (KERNEL-SPEC.md, Messages). `msg_id` is unique
/// within the receiving process; a handle revoked while the message was queued arrives as 0
/// (R10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: MsgKind,
    pub msg_id: u64,
    pub badge: u64,
    pub account: u64,
    pub labels: Vec<u64>,
    pub words: [u64; WORDS],
    /// Indices of the delivered handles in the receiver's table.
    pub handles: Vec<u64>,
    /// The lend or transfer, if the message carries one.
    pub buffer: Option<Received>,
}

/// A buffer as the receiver sees it: where it is mapped and how many pages it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Received {
    pub kind: BufferKind,
    pub addr: u64,
    pub pages: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind {
    Lend,
    Transfer,
}

/// Lend ownership on every completed call, including decoding errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LendDisposition {
    None,
    Returned,
    Consumed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyRecord {
    pub words: [u64; WORDS],
    pub handles: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallCompletion {
    pub status: Result<(), Error>,
    pub lend: LendDisposition,
    pub reply: Option<ReplyRecord>,
}

/// Observable record validity, independent of record contents. Owned is an abstract backed
/// private record; Memory checks an actual modeled mapping (including records inside lends).
/// CopyFault is initially writable but cannot commit at completion, exercising rollback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Record {
    #[default]
    Owned,
    Unmapped,
    ReadOnly,
    Borrowed,
    Device,
    CopyFault,
    Memory(u64),
}

/// A successful result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ret {
    /// Calls with no result value.
    Unit,
    Addr(u64),
    /// `dma_alloc`.
    AddrPhys {
        addr: u64,
        phys: u64,
    },
    Tid(u64),
    Handle(u64),
    /// Status, memory ownership and committed reply are independent (answers 167-168).
    Call(CallCompletion),
    /// Successful server completion, including abandoned and uncommittable replies.
    Replied {
        delivered: bool,
        installed_mask: u8,
    },
    Message(Message),
    /// `receive` on an IRQ handle: the handle that fired.
    Interrupt {
        h: u64,
    },
    /// `blamed_labels` is the label set of the blamed call's sender: blame is keyed by
    /// (account, label set) (QUESTIONS 48).
    ExitNotice {
        pid: u64,
        cause: Cause,
        code: u64,
        blamed_account: u64,
        blamed_labels: Vec<u64>,
    },
    /// The open call `msg_id`, held by the receiving thread, was abandoned (R3).
    Abandoned {
        msg_id: u64,
    },
    Usage(Counters),
    Time(u64),
    /// A user load (not a system call): the word read.
    Word(u64),
    /// `random`'s u64: the model does not produce it (it is the kernel's CSPRNG output and cannot
    /// be compared).
    Random,
}

/// What a step did for the thread that made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done(Result<Ret, Error>),
    /// The call blocked; its result arrives later as a [`Wake`].
    Blocked,
    /// The calling thread no longer exists (`thread_exit`, `process_exit`, destroying its own
    /// budget, or a fault).
    Gone,
}

/// A result delivered to a blocked thread as a consequence of a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wake {
    pub pid: u64,
    pub tid: u64,
    pub result: Result<Ret, Error>,
}

/// One thing that happens to the machine. Only `Sys`, the memory accesses and `Fault` name a
/// thread; the thread must exist and be runnable (`Kernel::runnable`), otherwise the step is not
/// a legal event and `Kernel::step` returns `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Sys {
        pid: u64,
        tid: u64,
        call: Syscall,
    },
    /// Environment action: a sibling changes the pending record's mapping/validity.
    /// This acts between atomic kernel completions, never inside one.
    Record {
        pid: u64,
        tid: u64,
        record: Record,
    },
    /// A user store of one word; faults unless the page is mapped writable.
    Write {
        pid: u64,
        tid: u64,
        addr: u64,
        value: u64,
    },
    /// A user load of one word; faults unless the page is mapped readable.
    Read {
        pid: u64,
        tid: u64,
        addr: u64,
    },
    /// An instruction fetch; faults unless the page is mapped executable.
    Exec {
        pid: u64,
        tid: u64,
        addr: u64,
    },
    /// The thread faults (for example an illegal instruction).
    Fault {
        pid: u64,
        tid: u64,
    },
    /// Interrupt line `n` is raised.
    Irq {
        n: u64,
    },
    /// `dt` microseconds pass; the scheduler runs threads meanwhile (R12).
    Tick {
        dt: u64,
    },
}
