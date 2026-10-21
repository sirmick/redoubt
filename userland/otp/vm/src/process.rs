//! A process: registers, stack, mailbox and the bookkeeping for links and monitors.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec;
use alloc::vec::Vec;

use crate::atom::Atom;
use crate::loader::{FLOAT_REGS, X_REGS};
use crate::module::Module;
use crate::term::{Heap, Literals, OwnedTerm, Pid, Ref, Term};

/// A code address: an instruction in a module. Loaded modules live as long as the VM (a
/// replaced version is never freed, as its literals are not), so an address is a plain copy:
/// calls and returns touch no reference counts shared between schedulers.
#[derive(Clone, Copy)]
pub struct Cp {
    pub module: &'static Module,
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
        self.entries.binary_search_by(|(k, _)| heap.cmp_exact(*k, key))
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

    pub fn entries(&self) -> &[(Term, Term)] { &self.entries }

    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    pub fn take(&mut self) -> Vec<(Term, Term)> { core::mem::take(&mut self.entries) }

    /// The terms, for a collection.
    pub fn terms_mut(&mut self) -> impl Iterator<Item = &mut Term> {
        self.entries.iter_mut().flat_map(|(k, v)| [k, v])
    }
}

/// Cells a heap may grow to before its first collection.
pub const MIN_HEAP_CELLS: usize = 1024;

/// Cells a heap keeps free at every safe point, so that an instruction building no more than
/// this never grows it: few, as every process keeps them (a kilobyte, about the slack a heap
/// growing by doubling has anyway). One that builds more of a size the process chose asks first
/// ([`Process::ask`]); one whose size its code fixes (a tuple written out in it) grows the heap
/// as the safe point does, room to grow into first and then exactly what it needs, and only a
/// refusal of both is the allocator's backstop.
pub const HEADROOM_CELLS: usize = 64;

/// Where a process stands with an allocation it asked for ([`Process::ask`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Room {
    #[default]
    Free,
    /// Refused for now: the scheduler collects the heap at the end of the slice, and the
    /// instruction or native that asked runs again.
    Wanted,
    /// Collected for it: asked again and refused again, the process is killed.
    Collected,
    /// Refused after the collection: the process is to be killed.
    Refused,
}

/// The answer to [`Process::ask`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Granted,
    /// Stop without changing anything and run again after a collection.
    Collect,
    /// Over a limit, or refused by the allocator, even after a collection: kill the process.
    Refused,
}

/// Off-heap binary bytes a heap may reference before its first collection. Large binaries
/// cost the heap only a cell or two, so they get a budget of their own (BEAM's virtual heap).
pub const MIN_BINARY_BYTES: usize = 1 << 20;

/// A process's file operation that the platform finishes later ([`crate::platform::Files`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Io {
    /// None.
    #[default]
    Idle,
    /// A native has just been answered `Later`: at the end of its time slice the process waits.
    Asked,
    /// Waiting for the operation's end: a message does not wake it.
    Waiting,
    /// The operation ended before the process waited: it runs again at once.
    Done,
}

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
    /// An allocation asked for and not yet granted ([`Process::ask`]).
    pub room: Room,
    /// Where it stands with a file operation the platform finishes later.
    pub io: Io,
    /// Processes this one spawned during its time slice, which join the run queue when the
    /// slice ends (with several schedulers; see [`crate::vm::System::hold_back`]).
    pub spawned: Vec<Pid>,

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
            room: Room::Free,
            resume: None,
            io: Io::Idle,
            spawned: Vec::new(),
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
    /// Collect the heap if it has outgrown its threshold, and keep [`HEADROOM_CELLS`] free. Called
    /// only between instructions, when every term the process holds is in one of the places
    /// listed here. `false` if the allocator refuses the room even after a collection: the
    /// process is to be killed. `vm_limit` is the VM's limit on a process, in words.
    #[inline]
    pub fn maybe_collect(&mut self, vm_limit: u64) -> bool {
        let due = self.heap.len() >= self.gc_at || self.heap.offheap_bytes() >= self.gc_bytes_at;
        if !due && self.heap.spare() >= HEADROOM_CELLS {
            return true;
        }
        self.collect_and_keep_headroom(due, vm_limit)
    }

    #[cold]
    fn collect_and_keep_headroom(&mut self, due: bool, vm_limit: u64) -> bool {
        if due && !self.collect() {
            return false;
        }
        if self.heap.spare() >= HEADROOM_CELLS {
            return true;
        }
        let most = self.most_cells(vm_limit);
        self.heap.try_room(HEADROOM_CELLS, most)
            || (self.collect() && self.heap.try_room(HEADROOM_CELLS, most))
    }

    /// Ask for an allocation of `words` (in the VM's 8-byte words) before making it, as BEAM
    /// collects before a large allocation: granted if the process stays within its limits
    /// counting it and `alloc` (which makes the allocation, or reserves room for it, and says
    /// whether the allocator gave it) succeeds. Otherwise the first time the process is to stop
    /// with nothing changed and run the asking instruction again after a collection; the second
    /// time, it is to be killed, as a process over `max_heap_size` is.
    pub fn ask(&mut self, vm_limit: u64, words: u64, alloc: impl FnOnce(&mut Heap, usize) -> bool) -> Ask {
        // A collection made for the asking instruction serves all it asks for until it ends,
        // when the interpreter clears it.
        let collected = self.room == Room::Collected;
        let most = self.most_cells(vm_limit);
        // No more than the headroom every heap keeps is checked against the limit at the end of
        // the slice, as the heap's own growth is.
        let small = words <= 2 * HEADROOM_CELLS as u64;
        if (small || self.within(vm_limit, words)) && alloc(&mut self.heap, most) {
            return Ask::Granted;
        }
        if collected {
            self.room = Room::Refused;
            return Ask::Refused;
        }
        self.room = Room::Wanted;
        Ask::Collect
    }

    /// Ask for room for `cells` more heap cells ([`Process::ask`]).
    pub fn ask_cells(&mut self, vm_limit: u64, cells: usize) -> Ask {
        self.ask(vm_limit, 2 * cells as u64, |heap, most| heap.try_room(cells, most))
    }

    /// Whether the process stays within the VM's limit and its own `max_heap_size` holding
    /// `words` more than it does.
    pub fn within(&self, vm_limit: u64, words: u64) -> bool {
        let usage = crate::memory::process(self);
        let own = self.max_heap;
        let total = usage.total_words().saturating_add(words);
        let used = if own.include_shared_binaries { total } else { usage.words.saturating_add(words) };
        total <= vm_limit && !(own.size > 0 && used > own.size && own.kill)
    }

    /// The most cells the heap may grow to by itself: its limit's worth. Room to grow into is
    /// asked for only up to here, so its capacity does not run ahead of what it may hold.
    fn most_cells(&self, vm_limit: u64) -> usize {
        let own = self.max_heap;
        let words = if own.size > 0 && own.kill { own.size.min(vm_limit) } else { vm_limit };
        usize::try_from(words / 2).unwrap_or(usize::MAX)
    }

    /// Collect the heap now. `false` if the allocator refuses the copy: the process is to be
    /// killed. Refused at the start, the heap is untouched; refused halfway, the process's terms
    /// are half moved, so it is left holding none (its registers, stack, mailbox and dictionary
    /// emptied with its heap), and nothing can read one before it ends.
    pub fn collect(&mut self) -> bool {
        let live_guess = self.gc_at / 2;
        let Some(mut gc) = self.heap.collect(live_guess) else { return false };
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
        if !gc.finish() {
            self.heap.clear();
            self.x.fill(Term::Nil);
            self.stack.clear();
            self.frames.clear();
            self.handlers.clear();
            self.mailbox.clear();
            self.save = 0;
            self.dictionary = Dictionary::default();
            self.pending_exit = None;
            self.resume = None;
            return false;
        }
        // Allow as much new allocation as the collection had to scan (what survived and the
        // roots), so collecting costs a constant share of the work however deep the stack is.
        let roots = self.x.len() + self.stack.len() + self.mailbox.len() + self.dictionary.len();
        let live = self.heap.len();
        self.gc_at = live + MIN_HEAP_CELLS.max(live + roots);
        self.gc_bytes_at = MIN_BINARY_BYTES.max(self.heap.offheap_bytes() * 2);
        true
    }

    /// Refresh the heap's view of the literal chunks (after code was loaded).
    pub fn refresh(&mut self, lits: &Literals) { self.heap.refresh(lits); }
}
