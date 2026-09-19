//! The system calls of KERNEL-SPEC.md as data, their results, and the other things that can
//! happen to the machine (a user memory access, a fault, an interrupt, time passing).
//!
//! Every argument is a raw `u64` (or a list of them), as it arrives from user mode: a handle is
//! an index into the caller's table, an address is a user virtual address, a class is its
//! `redoubt-sys` tag. The kernel model validates all of them; no value can make it panic (I14).
//! The model does not model the ABI's argument buffers themselves (`redoubt-sys` passes message
//! bodies, budget specs and handle lists through user memory): their contents are the fields here.

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
    ProcessStart {
        process: u64,
        entry: u64,
        sp: u64,
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
    HandleClose {
        h: u64,
    },
    BudgetCreate {
        parent: u64,
        pages: u64,
        processes: u64,
        weight: u64,
        class: u64,
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
    SystemReset {
        h: u64,
        kind: u64,
    },
    Random {
        len: u64,
    },
}

impl Syscall {
    /// The spec's name for this call.
    pub fn name(&self) -> &'static str {
        match self {
            Syscall::MapAnon { .. } => "map_anon",
            Syscall::Unmap { .. } => "unmap",
            Syscall::SetFlags { .. } => "set_flags",
            Syscall::MapDevice { .. } => "map_device",
            Syscall::DmaAlloc { .. } => "dma_alloc",
            Syscall::ThreadCreate { .. } => "thread_create",
            Syscall::ThreadExit => "thread_exit",
            Syscall::ProcessExit { .. } => "process_exit",
            Syscall::ProcessCreate { .. } => "process_create",
            Syscall::ProcessMap { .. } => "process_map",
            Syscall::ProcessStart { .. } => "process_start",
            Syscall::EndpointCreate => "endpoint_create",
            Syscall::Mint { .. } => "mint",
            Syscall::Call { .. } => "call",
            Syscall::Send { .. } => "send",
            Syscall::Receive { .. } => "receive",
            Syscall::Reply { .. } => "reply",
            Syscall::HandleClose { .. } => "handle_close",
            Syscall::BudgetCreate { .. } => "budget_create",
            Syscall::BudgetDestroy { .. } => "budget_destroy",
            Syscall::BudgetUsage { .. } => "budget_usage",
            Syscall::TimeNow => "time_now",
            Syscall::SystemReset { .. } => "system_reset",
            Syscall::Random { .. } => "random",
        }
    }
}

/// A delivered message, as `receive` returns it (KERNEL-SPEC.md, Messages).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
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
    /// The reply to a `call`: words and the handles installed in the caller's table.
    Reply {
        words: [u64; WORDS],
        handles: Vec<u64>,
    },
    Message(Message),
    /// `receive` on an IRQ handle: the handle that fired.
    Interrupt {
        h: u64,
    },
    ExitNotice {
        pid: u64,
        cause: Cause,
        code: u64,
        blamed_account: u64,
    },
    Usage(Counters),
    Time(u64),
    /// A user load (not a system call): the word read.
    Word(u64),
    /// `random`: the model does not produce the bytes (they are the kernel's CSPRNG output and
    /// cannot be compared), only how many there are.
    Random {
        len: u64,
    },
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
