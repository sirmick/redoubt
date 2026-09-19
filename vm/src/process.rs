//! A process: registers, stack, mailbox and the bookkeeping for links and monitors.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use crate::atom::Atom;
use crate::loader::{FLOAT_REGS, X_REGS};
use crate::module::Module;
use crate::term::{Heap, Literals, OwnedTerm, Pid, Ref, Term};

/// A code address: an instruction in a module.
#[derive(Clone)]
pub struct Cp {
    pub module: Arc<Module>,
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
        Exception {
            class: Class::Error,
            reason,
            trace: None,
            cause: None,
        }
    }
    pub fn exit(reason: Term) -> Exception {
        Exception {
            class: Class::Exit,
            reason,
            trace: None,
            cause: None,
        }
    }
    pub fn throw(reason: Term) -> Exception {
        Exception {
            class: Class::Throw,
            reason,
            trace: None,
            cause: None,
        }
    }
    pub fn with_trace(class: Class, reason: Term, trace: Term) -> Exception {
        Exception {
            class,
            reason,
            trace: Some(trace),
            cause: None,
        }
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
        MaxHeap {
            size: 0,
            kill: true,
            error_logger: true,
            include_shared_binaries: false,
        }
    }
}

/// A monitor on a process, as the monitored process keeps it (outside its heap: it belongs to
/// the watcher).
#[derive(Clone)]
pub struct Monitor {
    /// Who is watching.
    pub watcher: Pid,
    /// How the watcher named the process (a pid, or `{Name, Node}`), for the message.
    pub object: OwnedTerm,
    /// The first element of the message: `'DOWN'`, or the `{tag, Tag}` option of `monitor/3`.
    pub tag: Option<OwnedTerm>,
}

/// The process dictionary: entries sorted by the exact order of their keys (terms on the
/// process's heap), so lookups are binary searches and `get/0` lists them in key order.
#[derive(Default)]
pub struct Dictionary {
    entries: Vec<(Term, Term)>,
}

impl Dictionary {
    fn find(&self, heap: &Heap, key: Term) -> Result<usize, usize> {
        self.entries
            .binary_search_by(|(k, _)| heap.cmp_exact(*k, key))
    }

    pub fn get(&self, heap: &Heap, key: Term) -> Option<Term> {
        self.find(heap, key).ok().map(|i| self.entries[i].1)
    }

    /// Set `key`; the old value if there was one.
    pub fn put(&mut self, heap: &Heap, key: Term, value: Term) -> Option<Term> {
        match self.find(heap, key) {
            Ok(i) => Some(core::mem::replace(&mut self.entries[i].1, value)),
            Err(i) => {
                self.entries.insert(i, (key, value));
                None
            }
        }
    }

    pub fn remove(&mut self, heap: &Heap, key: Term) -> Option<Term> {
        self.find(heap, key).ok().map(|i| self.entries.remove(i).1)
    }

    pub fn entries(&self) -> &[(Term, Term)] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn take(&mut self) -> Vec<(Term, Term)> {
        core::mem::take(&mut self.entries)
    }

    /// The terms, for a collection.
    pub fn terms_mut(&mut self) -> impl Iterator<Item = &mut Term> {
        self.entries.iter_mut().flat_map(|(k, v)| [k, v])
    }
}

/// Cells a heap may grow to before its first collection.
pub const MIN_HEAP_CELLS: usize = 1024;

/// Off-heap binary bytes a heap may reference before its first collection. Large binaries
/// cost the heap only a cell or two, so they get a budget of their own (BEAM's virtual heap).
pub const MIN_BINARY_BYTES: usize = 1 << 20;

pub struct Process {
    pub pid: Pid,
    /// Every term this process holds is on this heap (or a literal).
    pub heap: Heap,
    /// Collect when the heap reaches this many cells.
    pub gc_at: usize,
    /// Or when it references this many off-heap binary bytes.
    pub gc_bytes_at: usize,
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

    /// Messages, copied onto the heap when they arrive.
    pub mailbox: VecDeque<Term>,
    /// Index of the next message `loop_rec` looks at.
    pub save: usize,
    /// Deadline of the running `receive ... after`, if any.
    pub timer: Option<u64>,
    /// Where to continue when that timer fires (the instruction after `wait_timeout`).
    pub timeout_pc: u32,
    /// Set by the scheduler when the timer has fired; consumed by `wait_timeout`.
    pub timed_out: bool,
    /// Set by a native that cannot finish yet (it needs a process another scheduler is
    /// running): the call is made again when this process next runs.
    pub retry: bool,
    /// The native call to make again, with its arguments still in the x registers.
    pub resume: Option<crate::interp::Resume>,

    pub links: BTreeSet<Pid>,
    /// Monitors this process holds, by reference: the monitored process.
    pub monitors: BTreeMap<Ref, Pid>,
    /// Monitors on this process, by reference: the watching process, and how it named this one
    /// (a pid, or `{Name, Node}` for a monitor taken by registered name), for its `'DOWN'`.
    pub monitored_by: BTreeMap<Ref, Monitor>,
    pub trap_exit: bool,
    pub registered_name: Option<Atom>,
    pub dictionary: Dictionary,
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
    /// A process that will run `entry` with `args`, which are terms of `heap`.
    pub fn new(pid: Pid, entry: Cp, heap: Heap, args: Vec<Term>) -> Process {
        let mut x = vec![Term::Nil; X_REGS];
        for (i, a) in args.into_iter().enumerate() {
            x[i] = a;
        }
        Process {
            pid,
            gc_at: MIN_HEAP_CELLS.max(heap.len() * 2),
            gc_bytes_at: MIN_BINARY_BYTES.max(heap.offheap_bytes() * 2),
            heap,
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
            retry: false,
            resume: None,
            links: BTreeSet::new(),
            monitors: BTreeMap::new(),
            monitored_by: BTreeMap::new(),
            trap_exit: false,
            registered_name: None,
            dictionary: Dictionary::default(),
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

impl Process {
    /// Collect the heap if it has outgrown its threshold. Called only between instructions,
    /// when every term the process holds is in one of the places listed here.
    pub fn maybe_collect(&mut self) {
        if self.heap.len() >= self.gc_at || self.heap.offheap_bytes() >= self.gc_bytes_at {
            self.collect();
        }
    }

    /// Collect the heap now.
    pub fn collect(&mut self) {
        let live_guess = self.gc_at / 2;
        let mut gc = self.heap.collect(live_guess);
        for t in self.x.iter_mut() {
            gc.root(t);
        }
        for t in self.stack.iter_mut() {
            gc.root(t);
        }
        for t in self.mailbox.iter_mut() {
            gc.root(t);
        }
        for t in self.dictionary.terms_mut() {
            gc.root(t);
        }
        if let Some(t) = self.pending_exit.as_mut() {
            gc.root(t);
        }
        gc.finish();
        // Allow as much new allocation as the collection had to scan (what survived and the
        // roots), so collecting costs a constant share of the work however deep the stack is.
        let roots = self.x.len() + self.stack.len() + self.mailbox.len() + self.dictionary.len();
        let live = self.heap.len();
        self.gc_at = live + MIN_HEAP_CELLS.max(live + roots);
        self.gc_bytes_at = MIN_BINARY_BYTES.max(self.heap.offheap_bytes() * 2);
    }

    /// Refresh the heap's view of the literal chunks (after code was loaded).
    pub fn refresh(&mut self, lits: &Literals) {
        self.heap.refresh(lits);
    }
}
