//! A process: registers, stack, mailbox and the bookkeeping for links and monitors.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;

use crate::atom::Atom;
use crate::loader::{FLOAT_REGS, X_REGS};
use crate::module::Module;
use crate::term::{MapKey, Pid, Ref, Term};

/// A code address: an instruction in a module.
#[derive(Clone)]
pub struct Cp {
    pub module: Rc<Module>,
    pub pc: u32,
}

/// A stack frame made by `allocate`: `size` Y registers starting at `base` in the stack, plus
/// the continuation pointer to restore on `deallocate`.
pub struct Frame {
    pub base: usize,
    pub size: usize,
    pub cp: Option<Cp>,
}

/// An active `try` or `catch`: where to go, and which frame it belongs to.
pub struct Handler {
    /// Number of frames when the handler was installed; the handler's frame is the last of them.
    pub depth: usize,
    /// The Y register named by the `try`/`catch` instruction, to check its matching end.
    pub y: u16,
    pub target: Cp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Runnable,
    /// Blocked in `receive` until a message arrives or its timer fires.
    Waiting,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Error,
    Exit,
    Throw,
}

/// An exception in flight.
#[derive(Clone, Debug)]
pub struct Exception {
    pub class: Class,
    pub reason: Term,
    /// The stack trace as a list of `{M, F, A, Location}`. `None` until the interpreter records
    /// where the exception was raised; `erlang:raise/3` supplies one.
    pub trace: Option<Term>,
    /// For an error from a native: what went wrong, more precisely than the reason (the
    /// `cause` of BEAM's `error_info`, e.g. `id` for an ETS table that does not exist).
    pub cause: Option<Term>,
}

impl Exception {
    pub fn error(reason: Term) -> Exception {
        Exception { class: Class::Error, reason, trace: None, cause: None }
    }
    pub fn exit(reason: Term) -> Exception {
        Exception { class: Class::Exit, reason, trace: None, cause: None }
    }
    pub fn throw(reason: Term) -> Exception {
        Exception { class: Class::Throw, reason, trace: None, cause: None }
    }
    pub fn with_trace(class: Class, reason: Term, trace: Term) -> Exception {
        Exception { class, reason, trace: Some(trace), cause: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Low,
    Normal,
    High,
    Max,
}

impl Priority {
    pub fn name(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
            Priority::Max => "max",
        }
    }

    pub fn from_name(s: &str) -> Option<Priority> {
        Some(match s {
            "low" => Priority::Low,
            "normal" => Priority::Normal,
            "high" => Priority::High,
            "max" => Priority::Max,
            _ => return None,
        })
    }
}

/// A process's own memory limit (`process_flag(max_heap_size, ...)`), as BEAM keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaxHeap {
    /// Words; 0 means no limit of its own (the VM-wide one still applies).
    pub size: u64,
    /// Kill the process when over; otherwise the limit is only reported.
    pub kill: bool,
    pub error_logger: bool,
    /// Count off-heap binaries against `size` too.
    pub include_shared_binaries: bool,
}

impl Default for MaxHeap {
    fn default() -> MaxHeap {
        MaxHeap { size: 0, kill: true, error_logger: true, include_shared_binaries: false }
    }
}

/// A monitor on a process, as the monitored process keeps it.
#[derive(Clone)]
pub struct Monitor {
    /// Who is watching.
    pub watcher: Pid,
    /// How the watcher named the process (a pid, or `{Name, Node}`), for the message.
    pub object: Term,
    /// The first element of the message: `'DOWN'`, or the `{tag, Tag}` option of `monitor/3`.
    pub tag: Option<Term>,
}

pub struct Process {
    pub pid: Pid,
    pub x: Vec<Term>,
    pub f: Vec<f64>,
    /// Y registers of all frames, innermost last.
    pub stack: Vec<Term>,
    pub frames: Vec<Frame>,
    pub handlers: Vec<Handler>,
    /// Where `return` goes. `None` in the process's first function: returning ends the process.
    pub cp: Option<Cp>,
    pub pc: Cp,
    /// Set while control is at a handler after an exception, until `try_case`/`catch_end`.
    pub in_exception: bool,
    pub state: State,

    pub mailbox: VecDeque<Term>,
    /// Index of the next message `loop_rec` looks at.
    pub save: usize,
    /// Deadline of the running `receive ... after`, if any.
    pub timer: Option<u64>,
    /// Where to continue when that timer fires (the instruction after `wait_timeout`).
    pub timeout_pc: u32,
    /// Set by the scheduler when the timer has fired; consumed by `wait_timeout`.
    pub timed_out: bool,

    pub links: BTreeSet<Pid>,
    /// Monitors this process holds, by reference: the monitored process.
    pub monitors: BTreeMap<Ref, Pid>,
    /// Monitors on this process, by reference: the watching process, and how it named this one
    /// (a pid, or `{Name, Node}` for a monitor taken by registered name), for its `'DOWN'`.
    pub monitored_by: BTreeMap<Ref, Monitor>,
    pub trap_exit: bool,
    pub registered_name: Option<Atom>,
    pub dictionary: BTreeMap<MapKey, Term>,
    pub group_leader: Option<Pid>,
    /// Exit reason delivered while running (e.g. `exit(self(), kill)`), acted on at once.
    pub pending_exit: Option<Term>,
    /// Reductions left in this time slice.
    pub budget: usize,
    /// Reductions used since the process started (`process_info(P, reductions)`).
    pub reductions: u64,
    pub max_heap: MaxHeap,
    /// The module whose `undefined_function/3` handles calls to missing functions
    /// (`process_flag(error_handler, M)`); `None` for the default, which raises `undef`.
    pub error_handler: Option<Atom>,
    /// `low`, `normal`, `high` or `max`, as set; the scheduler does not act on it yet.
    pub priority: Priority,
    /// The last measurement of this process's memory, and `reductions` when it was taken.
    pub usage: crate::memory::Usage,
    pub measured_at: u64,
}

impl Process {
    pub fn new(pid: Pid, entry: Cp, args: Vec<Term>) -> Process {
        let mut x = vec![Term::Nil; X_REGS];
        for (i, a) in args.into_iter().enumerate() {
            x[i] = a;
        }
        Process {
            pid,
            x,
            f: vec![0.0; FLOAT_REGS],
            stack: Vec::new(),
            frames: Vec::new(),
            handlers: Vec::new(),
            cp: None,
            pc: entry,
            in_exception: false,
            state: State::Runnable,
            mailbox: VecDeque::new(),
            save: 0,
            timer: None,
            timeout_pc: 0,
            timed_out: false,
            links: BTreeSet::new(),
            monitors: BTreeMap::new(),
            monitored_by: BTreeMap::new(),
            trap_exit: false,
            registered_name: None,
            dictionary: BTreeMap::new(),
            group_leader: None,
            pending_exit: None,
            budget: 0,
            reductions: 0,
            max_heap: MaxHeap::default(),
            error_handler: None,
            priority: Priority::Normal,
            usage: crate::memory::Usage::default(),
            measured_at: 0,
        }
    }
}
