//! The VM: module registry, process table and the scheduler.
//!
//! One VM runs on one thread. Processes are scheduled round-robin, each for a fixed number of
//! reductions (function calls) before it must yield, so a busy process cannot starve the others.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::atom::{Atom, AtomTable, Atoms};
use crate::bif::{self, Native};
use crate::interp::{self, Stop};
use crate::loader::{self, LoadError};
use crate::module::Module;
use crate::platform::{ConsoleInput, Lookup, Platform};
use crate::process::{Class, Cp, Exception, Io, Process, State};
use crate::sched::Sched;
use crate::sync::{Lock, Sendable, Wakeup};
use crate::term::{Heap, Holdings, Literals, OwnedTerm, Pid, Ref, Resource, Store, Term, copy};

/// Reductions (calls) a process may run before it is preempted.
pub const TIME_SLICE: usize = 2000;
/// Most processes alive at once. Spawning more raises `system_limit`.
pub const MAX_PROCESSES: usize = 1 << 16;

/// Most keys `persistent_term` may hold; it is VM-wide state any process can grow.
pub const MAX_PERSISTENT_TERMS: usize = 1 << 16;

/// Resource limits for one VM, set by the embedder. Exceeding one raises `system_limit`.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Most messages queued for one process. A message that would exceed it kills the receiver
    /// (reason `{system_limit, message_queue}`): dropping messages silently breaks protocols
    /// in ways nobody notices, and a process that far behind is broken anyway.
    pub max_mailbox: usize,
    /// Most words (BEAM's measure, `erts_debug:flat_size/1`) one process may hold: registers,
    /// stack, mailbox and dictionary. A process may lower its own limit with
    /// `process_flag(max_heap_size, ...)`, never raise it above this. Exceeding it kills the
    /// process (reason `killed`), as BEAM's `max_heap_size` does.
    pub max_heap_words: u64,
    /// Most words all ETS tables of the VM may hold together. Inserts beyond it raise
    /// `system_limit`.
    pub max_ets_words: u64,
    /// Most words `persistent_term` may hold: its keys, and every value ever put, since a value
    /// replaced or erased is never freed. A put beyond it raises `system_limit`.
    pub max_persistent_words: u64,
    /// Largest binary or bitstring any one operation may build, in bits.
    pub max_binary_bits: usize,
    /// Most stack slots (Y registers plus one per frame) one process may use. Body recursion
    /// a few million deep is ordinary Erlang (`lists:map/2` on a long list), so this is large.
    pub max_stack_slots: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        // 128 MiB of binary; 16M stack slots (256 MiB of 16-byte terms, plus frames).
        Limits {
            max_mailbox: 1 << 20,
            max_heap_words: 1 << 27, // 1 GiB
            max_ets_words: 1 << 27,
            max_persistent_words: 1 << 27,
            max_binary_bits: 1 << 30,
            max_stack_slots: 1 << 24,
        }
    }
}

/// How an embedder sets up a VM.
#[derive(Default)]
pub struct Config {
    pub limits: Limits,
    /// Natives beyond the built-in ones, e.g. `beamlet_crypto::NATIVES`. A native whose
    /// module and function also exist in a loaded module replaces that function's body, the
    /// way `erlang:load_nif/2` does in BEAM.
    pub natives: &'static [bif::NativeSpec],
    /// Print the VM's memory breakdown on its console when it first waits for console input
    /// ([`crate::memory::footprint`]); the function gives the embedder's heap pages, if it knows them.
    pub report_memory: Option<crate::memory::HeapPages>,
}

/// Everything but the process table. The running process is borrowed separately, so native
/// functions get `&mut System` and `&mut Process` at the same time.
pub struct System {
    /// The platform, behind a lock of its own so file and console I/O need not hold up users of
    /// the rest of the system. Taken after the system lock, never before it.
    pub platform: Arc<Lock<Box<dyn Platform>>>,
    pub limits: Limits,
    pub ets: crate::ets::Tables,
    /// Process aliases: references that work as send destinations while active.
    pub aliases: BTreeMap<Ref, Alias>,
    /// The VM's own environment variables (`os:getenv/1`). Empty at start: the host's
    /// environment is not visible unless an embedder puts it here.
    pub env: BTreeMap<String, String>,
    /// `persistent_term`: VM-wide terms, written rarely and read often. The values are literals
    /// (each put makes a chunk), so reading one copies nothing, as in BEAM.
    pub persistent: BTreeMap<OwnedTerm, Term>,
    /// What `persistent_term` holds toward `Limits::max_persistent_words` ([`System::persist`]),
    /// but for resources: its keys' words, and the words of every value ever put.
    persistent_words: u64,
    /// The resources `persistent_term`'s keys and values hold, at their live sizes.
    persistent_held: Holdings,
    /// The literal chunks: modules' constants and persistent terms.
    pub literals: Literals,
    /// What schedulers check their caches against: bumped as code and literals change.
    pub(crate) generations: Arc<crate::sched::Generations>,
    pub atom_table: AtomTable,
    pub atoms: Atoms,
    pub(crate) modules: BTreeMap<String, &'static Module>,
    /// Modules replaced by a newer copy of themselves, whose old code stays allocated.
    pub(crate) replaced: usize,
    natives: bif::Registry,
    pub(crate) run_queue: VecDeque<Pid>,
    /// Everything waiting for a time: receive timeouts and message timers, by deadline.
    timers: BTreeSet<(u64, Timer)>,
    /// Message timers (`send_after`, `start_timer`) by reference: deadline, target (a pid or a
    /// name), message.
    pub(crate) message_timers: BTreeMap<Ref, (u64, Term, OwnedTerm)>,
    next_ref: u64,
    pub(crate) registered: BTreeMap<String, Pid>,
    pub(crate) procs: ProcTable,
    /// Exit signals waiting to be delivered.
    pub(crate) exits: VecDeque<ExitSignal>,
    /// Final results of processes someone is waiting for through [`Vm::run`].
    results: BTreeMap<Pid, Outcome>,
    watched: BTreeSet<Pid>,
    /// The group leader of processes that do not inherit one: the console I/O server.
    pub(crate) default_group_leader: Option<Pid>,
    /// This VM's working directory in the platform's file system (`file:get_cwd/0`).
    pub(crate) cwd: String,
    /// Open files: platform handle, and the process that opened it.
    pub(crate) files: BTreeMap<u64, Pid>,
    /// Processes waiting for a file operation the platform finishes later.
    io_waits: usize,
    /// Open ports, by the pid of the port's process, and the port behind each program handle.
    pub(crate) ports: BTreeMap<Pid, crate::bif::port::PortState>,
    pub(crate) program_ports: BTreeMap<u64, Pid>,
    /// Endpoints served and jobs running through the platform's `System`, whose events it polls.
    pub(crate) system_waits: usize,
    /// Set by `erlang:halt`: the VM stops with this status.
    pub(crate) halted: Option<i64>,
    /// The process that receives console input (`beamlet:console_subscribe/0`): the `user`
    /// I/O server. `None` once input has ended, or before anyone asked.
    pub(crate) console_reader: Option<Pid>,
    /// Entries in a stack trace (`system_flag(backtrace_depth, N)`; BEAM's default is 8).
    pub(crate) backtrace_depth: usize,
    /// Directories of the VM's own file system searched for `.beam` files after the platform
    /// (`code:add_patha/1` and friends), in order.
    pub(crate) code_path: Vec<String>,
    /// Directories of the VM's file system holding applications as `App` or `App-Vsn`
    /// directories (OTP's `lib`), for `code:lib_dir/1` and `code:priv_dir/1`.
    pub(crate) lib_roots: Vec<String>,
    /// For modules loaded from bytes (`code:load_binary/3`, `load_file/1`, `load_abs/1`): the
    /// file name they were loaded with, which `code:which/1` reports.
    pub(crate) module_files: BTreeMap<String, OwnedTerm>,
    /// Samples of where processes are at the end of each time slice (the top few functions),
    /// when profiling is on (`Vm::enable_profile`).
    pub(crate) profile: Option<BTreeMap<String, u64>>,
    /// What `resolve` found for `(module, function, arity)`, by atom identity. Emptied whenever
    /// a module is loaded or deleted, so it never holds stale code.
    resolved: BTreeMap<(usize, usize, u32), Target>,
    pub(crate) stats: Stats,
    /// Schedulers the VM runs on (`erlang:system_info(schedulers)`).
    pub(crate) schedulers: usize,
    /// Schedulers allowed to run (`system_flag(schedulers_online, N)`): helpers numbered from
    /// this one up park until it is raised.
    pub(crate) schedulers_online: usize,
    /// Schedulers in a time slice just now.
    pub(crate) running: usize,
    /// Schedulers waiting for work.
    pub(crate) sleepers: usize,
    /// Helper schedulers parked while offline (`schedulers_online`).
    pub(crate) parked: usize,
    /// Schedulers that have ended a time slice and not yet asked for their next process: each
    /// takes one from the run queue itself, and no sleeper is woken for it.
    pub(crate) taking: usize,
    /// Wake every sleeping scheduler when the lock is next released: a run's result arrived,
    /// the VM halted, or the run is over.
    pub(crate) wake_all: bool,
    /// The run is over: helper schedulers stop.
    stopping: bool,
    /// Nothing can ever run again.
    stuck: bool,
    /// [`Config::report_memory`], until the report is made.
    report_memory: Option<crate::memory::HeapPages>,
}

/// Most directories on the code path.
pub(crate) const MAX_PATHS: usize = 1024;

/// Where [`System::locate_module`] found a module: a file of the VM's code path, or the
/// platform.
pub(crate) enum Found {
    Path(String, Vec<u8>),
    Platform(Vec<u8>),
}

/// Counters behind `erlang:statistics/1`.
#[derive(Default)]
pub(crate) struct Stats {
    /// Monotonic time when the VM started.
    pub start_us: u64,
    /// Reductions of every process so far, living or not.
    pub reductions: u64,
    /// Time slices run.
    pub context_switches: u64,
    /// What `statistics(runtime | wall_clock | reductions)` last returned, for "since last call".
    pub last_runtime_us: u64,
    pub last_wall_us: u64,
    pub last_reductions: u64,
    /// The last `erlang:now/0`, which must strictly increase.
    pub last_now_us: u64,
}

/// Erlang modules every VM has, built from `vm/lib/*.erl` by `tools/build-lib`: the console
/// I/O server, and a small `logger` in place of the kernel application's.
const EMBEDDED: &[&[u8]] = &[
    include_bytes!("../lib/beamlet_io.beam"),
    include_bytes!("../lib/application.beam"),
    include_bytes!("../lib/gen_tcp.beam"),
    include_bytes!("../lib/beamlet_tcp.beam"),
    include_bytes!("../lib/beamlet_code.beam"),
    include_bytes!("../lib/beamlet_kernel.beam"),
    include_bytes!("../lib/beamlet_port.beam"),
    include_bytes!("../lib/ram_file.beam"),
];

/// Stand-ins for the kernel's `logger` and `error_logger`, loaded only when the platform does
/// not provide OTP's own (which then starts at boot, see `beamlet_kernel`).
const LOGGER_FALLBACK: &[&[u8]] =
    &[include_bytes!("../lib/logger.beam"), include_bytes!("../lib/error_logger.beam")];

enum Slot {
    Free,
    Present(Box<Process>),
    /// Taken out by the scheduler while it runs.
    Running {
        pid: Pid,
    },
}

pub(crate) struct ProcTable {
    slots: Vec<Slot>,
    /// Messages sent to each slot's process and not yet moved onto its heap, each on a heap of
    /// its own (BEAM's heap fragments): a sender never writes into another process's heap, so
    /// the receiver may be running on another scheduler.
    inboxes: Vec<VecDeque<OwnedTerm>>,
    /// Changes to each slot's process made while a scheduler was running it (links, monitors,
    /// names, group leader), applied when its time slice ends.
    deferred: Vec<Vec<Deferred>>,
    /// Each slot's memory use as of the end of its last time slice, for reports about a
    /// process a scheduler is running.
    usage: Vec<crate::memory::Usage>,
    /// Whether each slot's process traps exits (its `trap_exit` flag, mirrored here so a
    /// signal to it can be delivered while it runs).
    traps: Vec<bool>,
    free: Vec<u32>,
    live: usize,
    /// The serial of the next process. One counter for the whole table, so pids order by
    /// creation (as BEAM's do, which code relies on through map and `lists:sort` order) and a
    /// stale pid never matches a reused slot.
    next_serial: u32,
}

impl ProcTable {
    fn new() -> ProcTable {
        ProcTable {
            slots: Vec::new(),
            inboxes: Vec::new(),
            deferred: Vec::new(),
            usage: Vec::new(),
            traps: Vec::new(),
            free: Vec::new(),
            live: 0,
            next_serial: 0,
        }
    }

    fn allocate(&mut self, port: bool) -> Option<Pid> {
        if self.live >= MAX_PROCESSES {
            return None;
        }
        self.live += 1;
        let serial = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1);
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push(Slot::Free);
                self.inboxes.push(VecDeque::new());
                self.deferred.push(Vec::new());
                self.usage.push(Default::default());
                self.traps.push(false);
                (self.slots.len() - 1) as u32
            }
        };
        let pid = Pid { index, serial, port };
        self.slots[index as usize] = Slot::Running { pid };
        Some(pid)
    }

    pub(crate) fn get_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        match self.slots.get_mut(pid.index as usize) {
            Some(Slot::Present(p)) if p.pid == pid => Some(p),
            _ => None,
        }
    }

    /// Every process not running now (for memory reports).
    pub(crate) fn present_mut(&mut self) -> impl Iterator<Item = &mut Process> {
        self.slots.iter_mut().filter_map(|s| match s {
            Slot::Present(p) => Some(&mut **p),
            _ => None,
        })
    }

    pub(crate) fn is_alive(&self, pid: Pid) -> bool {
        match self.slots.get(pid.index as usize) {
            Some(Slot::Present(p)) => p.pid == pid,
            Some(Slot::Running { pid: running }) => *running == pid,
            _ => false,
        }
    }

    fn take(&mut self, pid: Pid) -> Option<Box<Process>> {
        let slot = self.slots.get_mut(pid.index as usize)?;
        match slot {
            Slot::Present(p) if p.pid == pid => {
                let Slot::Present(p) = core::mem::replace(slot, Slot::Running { pid }) else {
                    unreachable!()
                };
                Some(p)
            }
            _ => None,
        }
    }

    /// Record whether `pid` traps exits (set with its own flag).
    pub(crate) fn set_traps(&mut self, pid: Pid, traps: bool) {
        if self.is_alive(pid) {
            self.traps[pid.index as usize] = traps;
        }
    }

    /// Whether `pid` traps exits, if it is alive.
    fn traps(&self, pid: Pid) -> Option<bool> { self.is_alive(pid).then(|| self.traps[pid.index as usize]) }

    /// Whether a scheduler is running `pid` just now.
    pub(crate) fn is_running(&self, pid: Pid) -> bool {
        matches!(self.slots.get(pid.index as usize), Some(Slot::Running { pid: r }) if *r == pid)
    }

    /// Change process `pid`: now if it is in the table, or when its time slice ends if a
    /// scheduler is running it. `false` if there is no such process.
    pub(crate) fn update(&mut self, pid: Pid, f: impl FnOnce(&mut Process) + Sendable + 'static) -> bool {
        let index = pid.index as usize;
        match self.slots.get_mut(index) {
            Some(Slot::Present(p)) if p.pid == pid => {
                f(p);
                true
            }
            Some(Slot::Running { pid: r }) if *r == pid => {
                self.deferred[index].push(Box::new(f));
                true
            }
            _ => false,
        }
    }

    /// Apply the changes made to `p` while it ran.
    fn settle(&mut self, p: &mut Process) {
        for f in core::mem::take(&mut self.deferred[p.pid.index as usize]) {
            f(p);
        }
    }

    /// Memory use of `pid` (a process in the table, or as of the end of its last time slice).
    pub(crate) fn usage(&self, pid: Pid) -> Option<crate::memory::Usage> {
        match self.slots.get(pid.index as usize)? {
            Slot::Present(p) if p.pid == pid => Some(crate::memory::process(p)),
            Slot::Running { pid: r } if *r == pid => Some(self.usage[pid.index as usize]),
            _ => None,
        }
    }

    fn put(&mut self, mut p: Box<Process>) {
        self.settle(&mut p);
        let index = p.pid.index as usize;
        self.usage[index] = crate::memory::process(&p);
        self.slots[index] = Slot::Present(p);
    }

    pub(crate) fn count(&self) -> usize { self.live }

    /// Every live pid, including the running process's.
    pub(crate) fn pids(&self) -> Vec<Pid> {
        self.slots
            .iter()
            .filter_map(|s| match s {
                Slot::Present(p) => Some(p.pid),
                Slot::Running { pid } => Some(*pid),
                Slot::Free => None,
            })
            .collect()
    }

    /// The inbox of `pid`, if it is alive.
    pub(crate) fn inbox(&mut self, pid: Pid) -> Option<&mut VecDeque<OwnedTerm>> {
        if self.is_alive(pid) { self.inboxes.get_mut(pid.index as usize) } else { None }
    }

    /// Move the messages waiting in the inbox of `p` (the running process) onto its heap and
    /// into its mailbox. `false` if that overflows the mailbox: `p` must die.
    fn receive_pending(&mut self, p: &mut Process, max_mailbox: usize) -> bool {
        match self.inboxes.get_mut(p.pid.index as usize) {
            Some(inbox) => absorb(p, inbox, max_mailbox),
            None => true,
        }
    }

    /// `receive_pending` for a process in the table.
    fn receive_pending_of(&mut self, pid: Pid, max_mailbox: usize) -> bool {
        let index = pid.index as usize;
        match (self.slots.get_mut(index), self.inboxes.get_mut(index)) {
            (Some(Slot::Present(p)), Some(inbox)) if p.pid == pid => absorb(p, inbox, max_mailbox),
            _ => true,
        }
    }

    fn release(&mut self, pid: Pid) {
        self.inboxes[pid.index as usize].clear();
        self.deferred[pid.index as usize].clear();
        self.traps[pid.index as usize] = false;
        self.slots[pid.index as usize] = Slot::Free;
        self.free.push(pid.index);
        self.live -= 1;
    }
}

/// BEAM's preloaded modules that only make sense on top of its C runtime (ports, the file
/// system, the boot process, tracing). This VM does their job itself or not at all, so they are
/// never loaded, even if found on the code path; calls to them are `undef` unless a native
/// answers. (`erlang`, `erts_internal`, `persistent_term`, `atomics` and `counters` do load:
/// their Erlang code is useful and their NIF stubs are replaced by natives. So does `prim_eval`,
/// whose shipped `.beam` is BEAM assembly, not a stub: `erl_eval`'s `receive` runs on it.)
pub const RUNTIME_MODULES: &[&str] = &[
    "init",
    "erl_init",
    "erl_prim_loader",
    "erl_tracer",
    "erts_code_purger",
    "erts_dirty_process_signal_handler",
    "erts_literal_area_collector",
    "erts_trace_cleaner",
    "prim_inet",
    "prim_net",
    "prim_socket",
    "prim_zip",
    "socket_registry",
];

/// Most message timers the VM keeps at once (`system_limit` beyond).
pub const MAX_MESSAGE_TIMERS: usize = 1 << 16;

/// Something waiting for a deadline.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Timer {
    /// A process in `receive ... after`.
    Receive(Pid),
    /// A message timer, by reference.
    Message(Ref),
}

/// An active alias: who receives messages sent to it, and when it stops working.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Alias {
    pub owner: Pid,
    pub mode: AliasMode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AliasMode {
    /// Until `unalias/1`.
    Explicit,
    /// Until the monitor it came with is removed or fires.
    Demonitor,
    /// Like `Demonitor`, and also after the first message arrives through it.
    ReplyDemonitor,
}

/// An exit signal in flight. `kill` means "kill unconditionally" only when sent by `exit/2`;
/// a linked process that dies with reason `kill` sends an ordinary, trappable signal.
pub(crate) struct ExitSignal {
    pub target: Pid,
    pub from: Pid,
    /// Shared by all the signals of one exit.
    pub reason: Arc<OwnedTerm>,
    pub from_link: bool,
    /// Imposed by the VM (a resource limit): cannot be trapped, and `reason` is used as is.
    pub forced: bool,
}

/// How a process ended, kept outside it: the value its first function returned, or the
/// exception that ended it.
pub type Outcome = Result<OwnedTerm, OwnedException>;

/// An exception that ended a process.
#[derive(Clone, Debug)]
pub struct OwnedException {
    pub class: Class,
    pub reason: OwnedTerm,
    pub trace: Option<OwnedTerm>,
}

/// Why [`Vm::run`] returned.
#[derive(Debug)]
pub enum RunError {
    /// The watched process is still alive but nothing can run and no timer is pending.
    Deadlock,
    /// A process called `erlang:halt/0,1,2` with this status. Nothing runs after it.
    Halted(i64),
}

pub struct Vm {
    sys: Lock<System>,
    /// Where schedulers with nothing to do wait.
    wakeup: Wakeup,
}

impl Vm {
    pub fn new(platform: Box<dyn Platform>) -> Vm { Vm::with_limits(platform, Limits::default()) }

    pub fn with_limits(platform: Box<dyn Platform>, limits: Limits) -> Vm {
        Vm::with_config(platform, Config { limits, ..Default::default() })
    }

    /// A VM with resource limits and extra natives chosen by the embedder.
    pub fn with_config(platform: Box<dyn Platform>, config: Config) -> Vm {
        let limits = config.limits;
        let mut atom_table = AtomTable::new();
        let atoms = Atoms::new(&mut atom_table);
        let natives = bif::Registry::new(config.natives);
        Vm {
            sys: Lock::new(System {
                platform: Arc::new(Lock::new(platform)),
                limits,
                ets: crate::ets::Tables::default(),
                persistent: BTreeMap::new(),
                persistent_words: 0,
                persistent_held: Holdings::new(Store::Persistent),
                literals: Literals::default(),
                generations: Default::default(),
                env: BTreeMap::new(),
                aliases: BTreeMap::new(),
                atom_table,
                atoms,
                modules: BTreeMap::new(),
                replaced: 0,
                natives,
                run_queue: VecDeque::new(),
                timers: BTreeSet::new(),
                message_timers: BTreeMap::new(),
                next_ref: 1,
                registered: BTreeMap::new(),
                procs: ProcTable::new(),
                exits: VecDeque::new(),
                results: BTreeMap::new(),
                watched: BTreeSet::new(),
                default_group_leader: None,
                cwd: "/".into(),
                files: BTreeMap::new(),
                io_waits: 0,
                ports: BTreeMap::new(),
                program_ports: BTreeMap::new(),
                system_waits: 0,
                halted: None,
                console_reader: None,
                backtrace_depth: 8,
                code_path: Vec::new(),
                lib_roots: Vec::new(),
                module_files: BTreeMap::new(),
                profile: None,
                resolved: BTreeMap::new(),
                stats: Stats::default(),
                schedulers: 1,
                schedulers_online: 1,
                running: 0,
                sleepers: 0,
                parked: 0,
                taking: 0,
                wake_all: false,
                stopping: false,
                stuck: false,
                report_memory: config.report_memory,
            }),
            wakeup: Wakeup::default(),
        }
        .boot()
    }

    /// Start the console I/O servers, `user` and `standard_error`.
    fn boot(mut self) -> Vm {
        let sys = self.sys.get_mut();
        sys.stats.start_us = sys.platform.lock().monotonic_us();
        for module in EMBEDDED {
            self.sys.get_mut().load(module).expect("embedded modules load");
        }
        let real_logger = {
            let mut platform = self.sys.get_mut().platform.lock();
            matches!(platform.load_module("logger"), Lookup::Found(_))
                && matches!(platform.load_module("logger_sup"), Lookup::Found(_))
        };
        if !real_logger {
            for module in LOGGER_FALLBACK {
                self.sys.get_mut().load(module).expect("embedded modules load");
            }
        }
        // The shell `os:cmd/1` runs commands with (the kernel sets this at start). Programs run
        // outside the VM, so this is the host's shell, whatever the VM's file system holds.
        let key = self.atom("kernel_os_cmd_shell");
        let mut h = Heap::new(&self.sys.get_mut().literals);
        let shell = h.string("/bin/sh");
        let shell = self.sys.get_mut().make_literal(&h, shell);
        self.sys.get_mut().persistent.insert(OwnedTerm::immediate(key), shell);
        let user_name = self.atom("user");
        let user = self.spawn("beamlet_io", "start", |_| alloc::vec![user_name]).expect("spawn user");
        let stderr = self.atom("standard_error");
        let err = self.spawn("beamlet_io", "start", |_| alloc::vec![stderr]).expect("spawn standard_error");
        for pid in [user, err] {
            if let Some(p) = self.sys.get_mut().procs.get_mut(pid) {
                p.group_leader = Some(user);
            }
        }
        self.sys.get_mut().default_group_leader = Some(user);
        // OTP's file server, which `file` calls for most operations: started now, so that it is
        // registered before any other code runs (about 2 ms). Without a file system it still
        // answers `get_cwd`, which compilers ask for; file operations fail with `enotsup`.
        if let Ok(pid) = self.spawn("file_server", "start", |_| Vec::new()) {
            let _ = self.run_bounded(pid, 100_000);
        }
        if real_logger {
            if let Ok(pid) = self.spawn("beamlet_kernel", "start", |_| Vec::new()) {
                let _ = self.run_bounded(pid, 1_000_000);
            }
        }
        self
    }

    /// Load a module from `.beam` bytes, replacing any module of the same name.
    pub fn load(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> { self.sys.get_mut().load(bytes) }

    /// Spawn `module:function(args)` as a new process, loading the module if needed. `args`
    /// builds the arguments on the new process's heap.
    pub fn spawn(
        &mut self,
        module: &str,
        function: &str,
        args: impl FnOnce(&mut Heap) -> Vec<Term>,
    ) -> Result<Pid, OwnedException> {
        let m = self.sys.get_mut().atom(module);
        let f = self.sys.get_mut().atom(function);
        let mut heap = Heap::new(&self.sys.get_mut().literals);
        let args = args(&mut heap);
        self.sys.get_mut().spawn(&m, &f, heap, args).map_err(|e| OwnedException {
            class: e.class,
            reason: OwnedTerm::immediate(e.reason),
            trace: None,
        })
    }

    /// Run until process `pid` ends, and return its result: the value its first function
    /// returned, or the exception that ended it.
    pub fn run(&mut self, pid: Pid) -> Result<Outcome, RunError> {
        let sys = self.sys.get_mut();
        sys.watched.insert(pid);
        sys.stopping = false;
        sys.stuck = false;
        // A helper that stopped at the end of the last run may have ended a slice unpaid.
        sys.taking = 0;
        let helpers = sys.schedulers - 1;
        let (sys, wakeup) = (&self.sys, &self.wakeup);
        #[cfg(feature = "std")]
        if helpers > 0 {
            return std::thread::scope(|scope| {
                for index in 1..=helpers {
                    scope.spawn(move || help(sys, wakeup, index));
                }
                let result = drive(sys, wakeup, pid);
                sys.lock().stopping = true;
                wakeup.wake_all();
                result
            });
        }
        let _ = helpers;
        drive(sys, wakeup, pid)
    }

    /// Run on `n` schedulers (threads), at least one. Without the `std` feature there is only
    /// ever one.
    #[cfg(feature = "std")]
    pub fn set_schedulers(&mut self, n: usize) {
        let sys = self.sys.get_mut();
        sys.schedulers = n.max(1);
        sys.schedulers_online = sys.schedulers;
    }

    /// Set a variable of the VM's own environment (`os:getenv/1`), which starts empty.
    pub fn setenv(&mut self, name: &str, value: &str) {
        self.sys.get_mut().env.insert(String::from(name), String::from(value));
    }

    /// Add a directory of the VM's file system where applications live (`App-Vsn/ebin`,
    /// `App-Vsn/priv`, `App-Vsn/include`), searched by `code:lib_dir/1`.
    pub fn add_lib_root(&mut self, dir: &str) { self.sys.get_mut().lib_roots.push(String::from(dir)); }

    /// Start sampling where processes are at the end of each time slice (a statistical profile
    /// for finding hot code; see [`Vm::profile`]).
    pub fn enable_profile(&mut self) { self.sys.get_mut().profile = Some(BTreeMap::new()); }

    /// The samples so far, most frequent first: `(count, "m:f/a < caller < ...")`.
    pub fn profile(&self) -> Vec<(u64, String)> {
        let mut v: Vec<(u64, String)> =
            self.sys.lock().profile.iter().flatten().map(|(k, n)| (*n, k.clone())).collect();
        v.sort_by(|a, b| b.cmp(a));
        v
    }

    /// Like [`Vm::run`], but give up after `max_steps` scheduling steps and return `None`.
    /// For tests that run untrusted code which may legitimately loop forever.
    pub fn run_bounded(&mut self, pid: Pid, max_steps: usize) -> Option<Result<Outcome, RunError>> {
        let sys = self.sys.get_mut();
        sys.watched.insert(pid);
        sys.stuck = false;
        sys.stopping = false;
        let mut sched = Sched::new(&self.sys, &self.wakeup);
        for _ in 0..max_steps {
            if let Some(done) = sched.lock().result(pid) {
                return Some(done);
            }
            if !schedule(&mut sched) {
                return Some(Err(RunError::Deadlock));
            }
        }
        None
    }

    pub fn atom(&mut self, name: &str) -> Term { Term::Atom(self.sys.get_mut().atom(name)) }
}

impl System {
    /// Send an exit signal. To a process that traps it, it becomes an `{'EXIT', From, Reason}`
    /// message at once, in order with the other messages and signals the sender sends (a
    /// linked process's death and its monitors' `'DOWN'`s reach everyone in the order BEAM
    /// gives). Signals that end or may end the target are delivered between time slices.
    pub(crate) fn signal_exit(&mut self, signal: ExitSignal) {
        let kill = !signal.from_link && signal.reason.term().is_atom(&self.atoms.kill);
        if signal.forced || kill || self.procs.traps(signal.target) != Some(true) {
            self.exits.push_back(signal);
            return;
        }
        let exit = Term::Atom(self.atoms.exit_upper);
        let (from, reason) = (signal.from, signal.reason);
        self.send_with(signal.target, |h| {
            let r = reason.copy_into(h);
            h.tuple(&[exit, Term::Pid(from), r])
        });
    }

    /// Keep the just-spawned `child` of `parent` (the running process) off the run queue until
    /// the parent's time slice ends. With several schedulers another one would otherwise start
    /// it at once, and code like `monitor(process, spawn(F))` relies on the parent getting
    /// there first, as it does on BEAM (a new process goes to its parent's scheduler).
    pub(crate) fn hold_back(&mut self, parent: &mut Process, child: Pid) {
        if self.schedulers > 1 && self.run_queue.back() == Some(&child) {
            self.run_queue.pop_back();
            parent.spawned.push(child);
        }
    }

    /// Whether a scheduler with nothing to run should wait for others to make work.
    pub(crate) fn should_sleep(&self) -> bool {
        self.run_queue.is_empty()
            && self.running > 0
            && self.results.is_empty()
            && self.halted.is_none()
            && !self.stopping
            && !self.stuck
    }

    /// How the run for `pid` ended, if it has: the VM halted, `pid` finished, or nothing can
    /// run again.
    fn result(&mut self, pid: Pid) -> Option<Result<Outcome, RunError>> {
        let done = if let Some(status) = self.halted {
            Err(RunError::Halted(status))
        } else if let Some(r) = self.results.remove(&pid) {
            self.watched.remove(&pid);
            Ok(r)
        } else if self.stuck {
            self.stuck = false;
            Err(RunError::Deadlock)
        } else {
            return None;
        };
        // The run is over: helper schedulers stop (and none blocks in the platform first).
        self.stopping = true;
        self.wake_all = true;
        Some(done)
    }

    /// Intern an atom the VM needs. Only for names from code or the embedder, which are short.
    pub fn atom(&mut self, name: &str) -> Atom {
        self.atom_table.intern(name).expect("VM-internal atom names are within limits")
    }

    /// The memory breakdown, once, if the embedder asked for it: on the console, a line a row.
    fn report_memory(&mut self) {
        let Some(heap_pages) = self.report_memory.take() else { return };
        let lines = crate::memory::footprint(self, heap_pages);
        let mut platform = self.platform.lock();
        for line in lines {
            platform.console_write(line.as_bytes());
        }
    }

    /// Loaded code changed: every cache of resolved calls is stale.
    fn code_changed(&mut self) {
        self.resolved.clear();
        self.generations.code.fetch_add(1, core::sync::atomic::Ordering::Release);
    }

    /// Literal chunks were added: schedulers pick them up on their next check.
    fn literals_changed(&mut self) {
        let n = self.literals.chunks();
        self.generations.literals.store(n, core::sync::atomic::Ordering::Release);
    }

    pub fn make_ref(&mut self) -> Ref {
        let r = Ref(self.next_ref);
        self.next_ref += 1;
        r
    }

    /// Load a module from bytes, replacing any module of the same name.
    pub fn load_bytes(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> { self.load(bytes) }

    fn load(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> {
        let mut module = loader::load(bytes, &mut self.atom_table, &mut self.literals)?;
        self.literals_changed();
        for imp in &mut module.imports {
            imp.native = self.natives.get(&imp.module, &imp.function, imp.arity);
        }
        // Functions implemented natively (NIF stubs like `crypto:hash_nif/2`): replace each
        // body's entry, the `label` right after `func_info`, with a call to the native. Local
        // calls, exports and funs all enter through that label.
        let functions = module.functions.clone();
        for f in &functions {
            let Some(n) = self.natives.get(&module.name, &f.name, f.arity) else {
                continue;
            };
            module.replace_body(f.start as usize + 1, (n, f.name, f.arity));
        }
        let name = module.name;
        if self.modules.insert(name.as_str().to_string(), Box::leak(Box::new(module))).is_some() {
            self.replaced += 1;
        }
        self.code_changed();
        Ok(name)
    }

    /// The module named `name`, loading it through the platform on first use.
    pub fn module(&mut self, name: &Atom) -> Option<&'static Module> {
        if let Some(m) = self.modules.get(name.as_str()) {
            return Some(*m);
        }
        if RUNTIME_MODULES.contains(&name.as_str()) {
            return None;
        }
        let (path, bytes) = match self.locate_module(name.as_str())? {
            Found::Path(path, bytes) => (Some(path), bytes),
            Found::Platform(bytes) => (None, bytes),
        };
        let loaded = self.load(&bytes).ok()?;
        if &loaded != name {
            // A file that claims to be a different module than the one asked for.
            self.modules.remove(loaded.as_str());
            return None;
        }
        // A file of the code path is what `code:which/1` and the rest say it was loaded from.
        if let Some(path) = path {
            let file = OwnedTerm::build(&self.literals, |h| h.string(&path));
            self.module_files.insert(String::from(name.as_str()), file);
        }
        self.modules.get(name.as_str()).cloned()
    }

    /// Put `dir` on the code path, in front (`code:add_patha/1`) or at the end, taking it off
    /// where it was: `false` if the path is full.
    pub(crate) fn add_code_path(&mut self, dir: String, front: bool) -> bool {
        self.remove_code_path(&dir);
        if self.code_path.len() >= MAX_PATHS {
            return false;
        }
        if front {
            self.code_path.insert(0, dir);
        } else {
            self.code_path.push(dir);
        }
        true
    }

    /// Take `dir` off the code path: whether it was on it.
    pub(crate) fn remove_code_path(&mut self, dir: &str) -> bool {
        let Some(i) = self.code_path.iter().position(|p| p == dir) else {
            return false;
        };
        self.code_path.remove(i);
        true
    }

    /// Where `module`'s code is: the platform's modules (the system bundle) first, whatever the
    /// code path holds, so no directory shadows a system module; on absence, the code path in
    /// order. A refused platform lookup ends here.
    pub(crate) fn locate_module(&mut self, module: &str) -> Option<Found> {
        match self.platform.lock().load_module(module) {
            Lookup::Found(bytes) => return Some(Found::Platform(bytes)),
            Lookup::Refused => return None,
            Lookup::Absent => {}
        }
        self.find_in_code_path(module).map(|(path, bytes)| Found::Path(path, bytes))
    }

    /// `Module.beam` from the first directory of the VM's code path that has it: its path and
    /// its bytes.
    fn find_in_code_path(&mut self, module: &str) -> Option<(String, Vec<u8>)> {
        let max = self.limits.max_binary_bits / 8;
        if self.code_path.is_empty() {
            return None;
        }
        let mut platform = self.platform.lock();
        let files = platform.files()?;
        for dir in &self.code_path {
            let path = alloc::format!("{}/{}.beam", dir.trim_end_matches('/'), module);
            if let Ok(bytes) = crate::bif::read_whole_file(files, &path, max) {
                return Some((path, bytes));
            }
        }
        None
    }

    /// What decoding a term needs of the system at once: the atom table, and a view of the
    /// loaded code (a module's checksum, whether a function is exported), loading nothing.
    pub(crate) fn term_decoding(
        &mut self,
    ) -> (&mut AtomTable, impl Fn(&Atom) -> Option<[u8; 16]> + '_, impl Fn(&Atom, &Atom, u32) -> bool + '_)
    {
        let (modules, natives) = (&self.modules, &self.natives);
        let md5_of = move |m: &Atom| modules.get(m.as_str()).map(|m| m.md5);
        let exported = move |m: &Atom, f: &Atom, a: u32| {
            natives.get(m, f, a).is_some()
                || modules.get(m.as_str()).is_some_and(|md| md.export(f, a).is_some())
        };
        (&mut self.atom_table, md5_of, exported)
    }

    /// The checksum of a loaded module (without loading it).
    pub fn loaded_md5(&self, name: &Atom) -> Option<[u8; 16]> {
        self.modules.get(name.as_str()).map(|m| m.md5)
    }

    pub fn is_loaded(&self, name: &Atom) -> bool { self.modules.contains_key(name.as_str()) }

    /// Unload a module (`code:delete/1`): new calls no longer reach it, while code already
    /// running in it finishes (it is reference counted). A later call loads it afresh through
    /// the platform, if the platform has it. `false` if it was not loaded.
    pub fn delete_module(&mut self, name: &Atom) -> bool {
        self.code_changed();
        self.module_files.remove(name.as_str());
        self.modules.remove(name.as_str()).is_some()
    }

    /// Names of all loaded modules.
    pub fn loaded_modules(&mut self) -> Vec<Atom> {
        let names: Vec<String> = self.modules.keys().cloned().collect();
        names.iter().map(|n| self.atom(n)).collect()
    }

    pub fn native(&self, module: &Atom, function: &Atom, arity: u32) -> Option<Native> {
        self.natives.get(module, function, arity)
    }

    /// Resolve `module:function/arity` to code: a native function or an exported Erlang one.
    pub fn resolve(&mut self, module: &Atom, function: &Atom, arity: u32) -> Option<Target> {
        let key = (module.id(), function.id(), arity);
        if let Some(t) = self.resolved.get(&key) {
            return Some(t.clone());
        }
        let target = match self.native(module, function, arity) {
            Some(n) => Target::Native(n),
            None => {
                let m = self.module(module)?;
                let entry = m.export(function, arity)?;
                Target::Code(Cp { module: m, pc: entry })
            }
        };
        self.resolved.insert(key, target.clone());
        Some(target)
    }

    /// Spawn `module:function(args)`; `args` are terms of `heap`, which becomes the process's.
    pub fn spawn(
        &mut self,
        module: &Atom,
        function: &Atom,
        heap: Heap,
        args: Vec<Term>,
    ) -> Result<Pid, Exception> {
        let entry = match self.resolve(module, function, args.len() as u32) {
            Some(Target::Code(cp)) => cp,
            // Spawning a native directly: run it through a tiny trampoline is not supported yet.
            _ => return Err(Exception::error(Term::Atom(self.atoms.undef))),
        };
        self.spawn_as(entry, heap, args, false)
    }

    /// Spawn a process running `entry` with a copy of `args` (terms of `src`).
    pub fn spawn_copy(&mut self, entry: Cp, src: &Heap, args: &[Term], port: bool) -> Result<Pid, Exception> {
        let mut heap = Heap::new(&self.literals);
        let args = args.iter().map(|&a| copy(src, a, &mut heap)).collect();
        self.spawn_as(entry, heap, args, port)
    }

    /// Start a process, or (`port`) the process behind a new port; `args` are terms of `heap`.
    pub(crate) fn spawn_as(
        &mut self,
        entry: Cp,
        heap: Heap,
        args: Vec<Term>,
        port: bool,
    ) -> Result<Pid, Exception> {
        let pid =
            self.procs.allocate(port).ok_or_else(|| Exception::error(Term::Atom(self.atoms.system_limit)))?;
        let mut p = Process::new(pid, entry, heap, args);
        p.group_leader = self.default_group_leader;
        self.procs.put(Box::new(p));
        self.run_queue.push_back(pid);
        Ok(pid)
    }

    /// Queue a copy of `msg` (a term of `src`) for `to`. Sending to a dead process silently does
    /// nothing, as in Erlang. (The running process is not in the table: see `bif::proc::send_to`.)
    pub fn send(&mut self, to: Pid, src: &Heap, msg: Term) {
        self.send_with(to, |heap| copy(src, msg, heap));
    }

    /// Queue a message for `to`, built by `build` on a heap of its own.
    pub fn send_with(&mut self, to: Pid, build: impl FnOnce(&mut Heap) -> Term) {
        if self.procs.is_alive(to) {
            let fragment = OwnedTerm::build(&self.literals, build);
            self.send_owned(to, fragment);
        }
    }

    /// Queue `fragment` for `to` (dropped if `to` is not alive). `false` if that overflowed
    /// `to`'s mailbox, which ends it.
    pub fn send_owned(&mut self, to: Pid, fragment: OwnedTerm) -> bool {
        let Some(inbox) = self.procs.inbox(to) else {
            return true;
        };
        if inbox.len() >= self.limits.max_mailbox {
            let reason = mailbox_full(&mut self.atom_table, &self.atoms);
            self.exits.push_back(ExitSignal { target: to, from: to, reason, from_link: false, forced: true });
            return false;
        }
        inbox.push_back(fragment);
        if let Some(p) = self.procs.get_mut(to) {
            if p.state == State::Waiting && p.io != Io::Waiting {
                p.state = State::Runnable;
                self.run_queue.push_back(to);
            }
        }
        true
    }

    /// Move the messages waiting for `p` (the running process) into its mailbox. If that
    /// overflows it, `p` is ended with `{system_limit, message_queue}` after this instruction.
    pub(crate) fn receive_pending(&mut self, p: &mut Process) {
        if !self.procs.receive_pending(p, self.limits.max_mailbox) {
            let reason = mailbox_full(&mut self.atom_table, &self.atoms);
            p.pending_exit = Some(reason.copy_into(&mut p.heap));
        }
    }

    /// Move the messages waiting for `pid` (a process in the table, or none) into its mailbox,
    /// so it can be inspected. If that overflows it, the process is ended.
    pub(crate) fn receive_pending_of(&mut self, pid: Pid) {
        if !self.procs.receive_pending_of(pid, self.limits.max_mailbox) {
            let reason = mailbox_full(&mut self.atom_table, &self.atoms);
            self.exits.push_back(ExitSignal {
                target: pid,
                from: pid,
                reason,
                from_link: false,
                forced: true,
            });
        }
    }

    /// Put `t`, a term of `src`, in `persistent_term` under `key`, as a literal: reading it copies
    /// nothing, as in BEAM. Refused, with nothing changed, if it would take `persistent_term` past
    /// `Limits::max_persistent_words` or `MAX_PERSISTENT_TERMS` keys. A value replaced or erased
    /// stays in its chunk (BEAM frees it once no process refers to it), so each put counts for
    /// good; a key counts while it is there. Literals `t` refers to are not copied, and not
    /// counted again.
    pub fn persist(&mut self, key: OwnedTerm, src: &Heap, t: Term) -> Result<(), ()> {
        let new_key = !self.persistent.contains_key(&key);
        if new_key && self.persistent.len() >= MAX_PERSISTENT_TERMS {
            return Err(());
        }
        let mut heap = Heap::new(&Literals::default());
        let root = copy(src, t, &mut heap);
        let heaps: &[&Heap] = if new_key { &[&heap, key.heap()] } else { &[&heap] };
        let words: u64 = heaps.iter().map(|h| h.term_words()).sum();
        let held: u64 = heaps.iter().map(|h| Holdings::weigh(h)).sum();
        let total = self.persistent_words + words + (self.persistent_held.bytes() + held).div_ceil(8);
        if total > self.limits.max_persistent_words {
            return Err(());
        }
        self.persistent_words += words;
        heaps.iter().for_each(|h| self.persistent_held.enter(h));
        let mut roots = [root];
        self.literals.add(heap, &mut roots);
        self.literals_changed();
        // An existing key keeps the entry it was counted with.
        self.persistent.insert(key, roots[0]);
        Ok(())
    }

    /// Erase `key` from `persistent_term`: its key no longer counts; its value stays.
    pub fn unpersist(&mut self, key: &OwnedTerm) -> bool {
        let Some((k, _)) = self.persistent.remove_entry(key) else { return false };
        self.persistent_words = self.persistent_words.saturating_sub(k.heap().term_words());
        self.persistent_held.leave(k.heap());
        true
    }

    /// Memory `persistent_term` holds, in words: what counts toward `max_persistent_words`.
    pub fn persistent_words(&self) -> u64 { self.persistent_words + self.persistent_held.words() }

    /// Resource `r` changed its declared size from `old` bytes to `new`: the stores outside any
    /// process that hold it count the new size.
    pub fn resized(&mut self, r: &Resource, old: usize, new: usize) {
        self.ets.resized(r, old, new);
        self.persistent_held.resized(r, old, new);
    }

    /// A copy of `t` (a term of `src`) as a literal: in a chunk of its own, never freed.
    pub fn make_literal(&mut self, src: &Heap, t: Term) -> Term {
        let mut heap = Heap::new(&Literals::default());
        let mut roots = [copy(src, t, &mut heap)];
        self.literals.add(heap, &mut roots);
        self.literals_changed();
        roots[0]
    }

    /// Start a message timer: at `deadline`, send `msg` to `to` (a pid or registered name).
    pub fn start_message_timer(&mut self, deadline: u64, to: Term, msg: OwnedTerm) -> Option<Ref> {
        if self.message_timers.len() >= MAX_MESSAGE_TIMERS {
            return None;
        }
        let r = self.make_ref();
        self.message_timers.insert(r, (deadline, to, msg));
        self.timers.insert((deadline, Timer::Message(r)));
        Some(r)
    }

    /// Cancel a message timer; its deadline if it was still pending.
    pub fn cancel_message_timer(&mut self, r: Ref) -> Option<u64> {
        let (deadline, _, _) = self.message_timers.remove(&r)?;
        self.timers.remove(&(deadline, Timer::Message(r)));
        Some(deadline)
    }

    /// Send to a pid or a registered name; silently nothing if there is no such process.
    fn send_to_term(&mut self, to: &Term, msg: OwnedTerm) {
        let pid = match to {
            Term::Pid(p) => Some(*p),
            Term::Atom(name) => self.registered.get(name.as_str()).copied(),
            _ => None,
        };
        if let Some(pid) = pid {
            self.send(pid, msg.heap(), msg.term());
        }
    }

    pub fn arm_timer(&mut self, pid: Pid, deadline: u64) {
        self.timers.insert((deadline, Timer::Receive(pid)));
    }

    pub fn cancel_timer(&mut self, pid: Pid, deadline: u64) {
        self.timers.remove(&(deadline, Timer::Receive(pid)));
    }

    pub fn now_us(&mut self) -> u64 { self.platform.lock().monotonic_us() }

    /// Housekeeping, then the next process to run, taken out of the table.
    fn next(&mut self) -> Next {
        self.deliver_exits();
        self.fire_timers();
        self.poll_console();
        self.poll_programs();
        self.poll_system();
        self.poll_files();
        let Some(pid) = self.run_queue.pop_front() else {
            // Nothing runnable. While other schedulers run, wait for them: they may make work.
            if self.running > 0 {
                return Next::Sleep;
            }
            // A run's result (or a halt) waits to be collected, or the run is over: do not block
            // in the platform.
            if !self.results.is_empty() || self.halted.is_some() || self.stopping {
                return Next::Again;
            }
            // Nothing running either: sleep until the next timer, console input or the end of a
            // file operation, or give up if nothing can ever arrive.
            if self.console_reader.is_some() {
                self.report_memory();
            }
            return match self.timers.first() {
                Some(&(deadline, _)) => {
                    self.platform.lock().idle(Some(deadline));
                    Next::Again
                }
                None if self.console_reader.is_some()
                    || !self.program_ports.is_empty()
                    || self.system_waits > 0
                    || self.io_waits > 0 =>
                {
                    self.platform.lock().idle(None);
                    Next::Again
                }
                None if !self.exits.is_empty() => Next::Again,
                None => {
                    self.stuck = true;
                    self.wake_all = true;
                    Next::Stuck
                }
            };
        };
        let Some(mut p) = self.procs.take(pid) else {
            return Next::Again;
        };
        if p.state != State::Runnable {
            self.procs.put(p);
            return Next::Again;
        }
        p.budget = TIME_SLICE;
        p.refresh(&self.literals);
        self.running += 1;
        Next::Run(p)
    }

    /// The end of `p`'s time slice, which began with `before` reductions and ended with `stop`.
    fn finish(&mut self, mut p: Box<Process>, before: u64, mut stop: Stop) {
        let pid = p.pid;
        self.running -= 1;
        self.run_queue.extend(p.spawned.drain(..));
        self.procs.settle(&mut p);
        self.stats.reductions += p.reductions - before;
        if let Some(profile) = &mut self.profile {
            *profile.entry(crate::interp::where_is(&p, 3)).or_default() += 1;
        }
        self.stats.context_switches += 1;
        if matches!(stop, Stop::Yield | Stop::Wait) && self.over_memory(&mut p) {
            stop = Stop::Exit(Err(Exception::exit(Term::Atom(self.atoms.killed))));
        }
        match stop {
            // A native answered `Later`: it waits for the platform's operation, not for a message.
            Stop::Yield if p.io == Io::Asked => {
                p.io = Io::Waiting;
                p.state = State::Waiting;
                self.io_waits += 1;
                self.procs.put(p);
            }
            Stop::Yield => {
                self.procs.put(p);
                self.run_queue.push_back(pid);
            }
            Stop::Wait => {
                p.state = State::Waiting;
                // A message may have arrived while it was running (e.g. sent to itself).
                if p.save < p.mailbox.len()
                    || p.timed_out
                    || self.procs.inbox(pid).is_some_and(|i| !i.is_empty())
                {
                    p.state = State::Runnable;
                    self.run_queue.push_back(pid);
                }
                self.procs.put(p);
            }
            Stop::Exit(result) => self.terminate(p, result),
        }
    }

    /// Whether `p` must be killed for holding too much memory. A heap over a limit is collected
    /// first: what counts is what is live, as with BEAM's `max_heap_size`.
    fn over_memory(&mut self, p: &mut Process) -> bool {
        let vm_limit = self.limits.max_heap_words;
        let own = p.max_heap;
        let over = |p: &Process| {
            let usage = crate::memory::process(p);
            let used = if own.include_shared_binaries { usage.total_words() } else { usage.words };
            usage.total_words() > vm_limit || (own.size > 0 && used > own.size && own.kill)
        };
        if !over(p) {
            return false;
        }
        p.collect();
        p.usage = crate::memory::process(p);
        over(p)
    }

    fn fire_timers(&mut self) {
        if self.timers.is_empty() {
            return;
        }
        let now = self.platform.lock().monotonic_us();
        while let Some(&(deadline, timer)) = self.timers.first() {
            if deadline > now {
                break;
            }
            self.timers.pop_first();
            let pid = match timer {
                Timer::Receive(pid) => pid,
                Timer::Message(r) => {
                    if let Some((_, to, msg)) = self.message_timers.remove(&r) {
                        self.send_to_term(&to, msg);
                    }
                    continue;
                }
            };
            if let Some(p) = self.procs.get_mut(pid) {
                if p.timer == Some(deadline) {
                    p.timer = None;
                    p.timed_out = true;
                    if p.state == State::Waiting {
                        p.state = State::Runnable;
                        self.run_queue.push_back(pid);
                    }
                }
            } else {
                // Running: unless the receive ended meanwhile, it times out when it next waits.
                self.procs.update(pid, move |p| {
                    if p.timer == Some(deadline) {
                        p.timer = None;
                        p.timed_out = true;
                    }
                });
            }
        }
    }

    /// Make `reader` the process console input goes to (none for `None`), and tell the platform
    /// whether anyone listens: with nobody, input it holds is no reason to wake the VM.
    pub(crate) fn set_console_reader(&mut self, reader: Option<Pid>) {
        if self.console_reader.is_some() != reader.is_some() {
            self.platform.lock().console_listening(reader.is_some());
        }
        self.console_reader = reader;
    }

    /// End process `p`: tell its links and monitors, then free its slot.
    /// Pass console input, if any has arrived, to the process reading it, after the console's new
    /// size if it has changed: `{beamlet_console_resize, {Cols, Rows}}`.
    fn poll_console(&mut self) {
        let Some(reader) = self.console_reader else {
            return;
        };
        // The platform's lock goes before the match: an end tells the platform nobody listens.
        let (read, resized) = {
            let mut platform = self.platform.lock();
            let read = platform.console_read();
            (read, platform.console_resized())
        };
        if let Some((cols, rows)) = resized {
            let tag = Term::Atom(self.atom("beamlet_console_resize"));
            self.send_with(reader, |h| {
                let size = h.tuple(&[Term::Int(i64::from(cols)), Term::Int(i64::from(rows))]);
                h.tuple(&[tag, size])
            });
        }
        let input = match read {
            ConsoleInput::Nothing => return,
            ConsoleInput::Data(bytes) => Some(bytes),
            ConsoleInput::Eof => {
                self.set_console_reader(None);
                None
            }
        };
        let (tag, eof) = (Term::Atom(self.atom("beamlet_console")), Term::Atom(self.atom("eof")));
        self.send_with(reader, |h| {
            let msg = match &input {
                Some(bytes) => h.binary(bytes),
                None => eof,
            };
            h.tuple(&[tag, msg])
        });
    }

    /// Send `{log, error, "Error in process ~p with exit value:~n~p~n", [Pid, Reason], Meta}` to
    /// the `logger` process, if one is running, with the metadata BEAM gives these reports.
    fn report_crash(&mut self, p: &Process, reason: &OwnedTerm) {
        let Some(&logger) = self.registered.get("logger") else {
            return;
        };
        let atom = |s: &mut Self, name: &str| Term::Atom(s.atom(name));
        let [emulator, tag, error, error_logger, gl, pid_key, time_key, log] =
            ["emulator", "tag", "error", "error_logger", "gl", "pid", "time", "log"].map(|n| atom(self, n));
        let true_ = Term::Atom(self.atoms.true_);
        let time = crate::platform::system_time_us(&mut **self.platform.lock()) as i64;
        let (pid, leader) = (Term::Pid(p.pid), Term::Pid(p.group_leader.unwrap_or(p.pid)));
        self.send_with(logger, |h| {
            let format = h.string("Error in process ~p with exit value:~n~p~n");
            let el = h.map_from([(emulator, true_), (tag, error)]);
            let meta =
                h.map_from([(error_logger, el), (gl, leader), (pid_key, pid), (time_key, Term::Int(time))]);
            let reason = reason.copy_into(h);
            let args = h.list([pid, reason]);
            h.tuple(&[log, error, format, args, meta])
        });
    }

    /// Close the files `pid` opened.
    fn close_files(&mut self, pid: Pid) {
        let handles: Vec<u64> = self.files.iter().filter(|(_, &o)| o == pid).map(|(&h, _)| h).collect();
        for h in handles {
            self.files.remove(&h);
            if let Some(f) = self.platform.lock().files() {
                f.close(h);
            }
        }
    }

    /// Wake the processes whose file operations the platform has finished: a waiting one runs
    /// again; one still running, which has not yet waited, does not wait. Asked only while some
    /// process waits: one that has not waited yet is found once it does.
    fn poll_files(&mut self) {
        while self.io_waits > 0 {
            let finished = self.platform.lock().files().and_then(|f| f.finished());
            let Some(asker) = finished else { return };
            let pid = bif::asker_pid(asker);
            if let Some(p) = self.procs.get_mut(pid) {
                if p.io == Io::Waiting {
                    p.io = Io::Idle;
                    p.state = State::Runnable;
                    self.io_waits -= 1;
                    self.run_queue.push_back(pid);
                }
            } else {
                self.procs.update(pid, |p| {
                    if p.io == Io::Asked {
                        p.io = Io::Done;
                    }
                });
            }
        }
    }

    fn terminate(&mut self, mut p: Box<Process>, result: Result<Term, Exception>) {
        let pid = p.pid;
        let h = &mut p.heap;
        let reason = match &result {
            Ok(_) => Term::Atom(self.atoms.normal),
            Err(e) => match e.class {
                Class::Exit => e.reason,
                Class::Error => h.tuple(&[e.reason, e.trace.unwrap_or(Term::Nil)]),
                Class::Throw => {
                    let nocatch = h.tuple(&[Term::Atom(self.atoms.nocatch), e.reason]);
                    h.tuple(&[nocatch, e.trace.unwrap_or(Term::Nil)])
                }
            },
        };
        let reason = Arc::new(OwnedTerm::new(&p.heap, reason));
        if let Some(t) = p.timer {
            self.cancel_timer(pid, t);
        }
        // A file operation it began is dropped, with what it holds, when it ends.
        if p.io != Io::Idle {
            if p.io == Io::Waiting {
                self.io_waits -= 1;
            }
            if let Some(f) = self.platform.lock().files() {
                f.abandon(bif::asker(pid));
            }
        }
        // An uncaught error (or throw) is reported to the logger, as BEAM's emulator does.
        if matches!(&result, Err(e) if e.class != Class::Exit) {
            self.report_crash(&p, &reason);
        }
        self.aliases.retain(|_, a| a.owner != pid);
        self.close_files(pid);
        if pid.port {
            self.port_ended(pid);
        }
        if self.console_reader == Some(pid) {
            self.set_console_reader(None);
        }
        if let Some(name) = &p.registered_name {
            self.registered.remove(name.as_str());
        }
        for &other in &p.links {
            self.procs.update(other, move |o| {
                o.links.remove(&pid);
            });
            self.signal_exit(ExitSignal {
                target: other,
                from: pid,
                reason: reason.clone(),
                from_link: true,
                forced: false,
            });
        }
        let kind = if pid.port { Term::Atom(self.atom("port")) } else { Term::Atom(self.atoms.process) };
        let down = Term::Atom(self.atoms.down);
        for (r, crate::process::Monitor { watcher, object, tag }) in &p.monitored_by {
            let r = *r;
            self.procs.update(*watcher, move |w| {
                w.monitors.remove(&r);
            });
            // A monitor's alias ends when the monitor fires.
            if self.aliases.get(&r).is_some_and(|a| a.mode != AliasMode::Explicit) {
                self.aliases.remove(&r);
            }
            self.send_with(*watcher, |h| {
                let tag = tag.as_ref().map_or(down, |t| t.copy_into(h));
                let object = object.copy_into(h);
                let reason = reason.copy_into(h);
                h.tuple(&[tag, Term::Ref(r), kind, object, reason])
            });
        }
        for (&r, target) in &p.monitors {
            self.procs.update(*target, move |t| {
                t.monitored_by.remove(&r);
            });
        }
        // Its ETS tables go to their heirs, or are deleted.
        for tid in self.ets.owned_by(pid) {
            let heir = self.ets.get(tid).and_then(|t| t.heir().cloned());
            match heir {
                Some((to, data)) if to != pid && self.procs.is_alive(to) => {
                    let t = self.ets.get_mut(tid).expect("listed");
                    t.owner = to;
                    let id = if t.named { Term::Atom(t.name) } else { Term::Ref(Ref(t.tid)) };
                    let tag = Term::Atom(self.atom("ETS-TRANSFER"));
                    self.send_with(to, |h| {
                        let data = data.copy_into(h);
                        h.tuple(&[tag, id, Term::Pid(pid), data])
                    });
                }
                _ => {
                    self.ets.delete(tid);
                }
            }
        }
        if self.watched.contains(&pid) {
            let outcome = match result {
                Ok(v) => Ok(OwnedTerm::new(&p.heap, v)),
                Err(e) => Err(OwnedException {
                    class: e.class,
                    reason: OwnedTerm::new(&p.heap, e.reason),
                    trace: e.trace.map(|t| OwnedTerm::new(&p.heap, t)),
                }),
            };
            self.results.insert(pid, outcome);
            self.wake_all = true;
        }
        drop(p);
        self.procs.release(pid);
    }

    /// Deliver queued exit signals. A signal either becomes an `{'EXIT', From, Reason}` message
    /// (the target traps exits), is ignored (reason `normal`), or kills the target.
    /// A signal to a process that another scheduler is running waits, in order, until its time
    /// slice ends.
    fn deliver_exits(&mut self) {
        let mut later = VecDeque::new();
        while let Some(signal) = self.exits.pop_front() {
            if self.procs.is_running(signal.target) {
                later.push_back(signal);
                continue;
            }
            let ExitSignal { target, from, reason, from_link, forced } = signal;
            if forced {
                if let Some(mut p) = self.procs.take(target) {
                    let reason = reason.copy_into(&mut p.heap);
                    self.terminate(p, Err(Exception::exit(reason)));
                }
                continue;
            }
            let kill = !from_link && reason.term().is_atom(&self.atoms.kill);
            let normal = reason.term().is_atom(&self.atoms.normal);
            let Some(p) = self.procs.get_mut(target) else {
                continue;
            };
            if p.trap_exit && !kill {
                let exit = Term::Atom(self.atoms.exit_upper);
                self.send_with(target, |h| {
                    let r = reason.copy_into(h);
                    h.tuple(&[exit, Term::Pid(from), r])
                });
            } else if !normal || from == target {
                let mut p = self.procs.take(target).expect("present");
                let reason = if kill { Term::Atom(self.atoms.killed) } else { reason.copy_into(&mut p.heap) };
                // Remove it from the run queue lazily: `step` skips pids that are gone.
                self.terminate(p, Err(Exception::exit(reason)));
            }
        }
        self.exits = later;
    }
}

fn absorb(p: &mut Process, inbox: &mut VecDeque<OwnedTerm>, max_mailbox: usize) -> bool {
    while let Some(m) = inbox.pop_front() {
        if p.mailbox.len() >= max_mailbox {
            inbox.clear();
            return false;
        }
        let t = m.absorb_into(&mut p.heap);
        p.mailbox.push_back(t);
    }
    true
}

/// The exit reason of a process whose mailbox overflowed.
pub(crate) fn mailbox_full(table: &mut AtomTable, atoms: &Atoms) -> Arc<OwnedTerm> {
    let queue = Term::Atom(table.intern("message_queue").expect("short atom"));
    let limit = Term::Atom(atoms.system_limit);
    Arc::new(OwnedTerm::build(&Literals::default(), |h| h.tuple(&[limit, queue])))
}

/// A change to a process that a scheduler is running, made when its time slice ends.
#[cfg(feature = "std")]
pub(crate) type Deferred = Box<dyn FnOnce(&mut Process) + Send>;
#[cfg(not(feature = "std"))]
pub(crate) type Deferred = Box<dyn FnOnce(&mut Process)>;

/// What a scheduler does next.
enum Next {
    Run(Box<Process>),
    /// Nothing to run, but other schedulers are running: wait for them.
    Sleep,
    /// Nothing to run just now (or housekeeping happened): ask again.
    Again,
    /// Nothing can ever run again.
    Stuck,
}

/// Schedule on this thread until the run for `pid` ends.
fn drive(sys: &Lock<System>, wakeup: &Wakeup, pid: Pid) -> Result<Outcome, RunError> {
    let mut sched = Sched::new(sys, wakeup);
    loop {
        if let Some(done) = sched.lock().result(pid) {
            return done;
        }
        if !schedule(&mut sched) {
            // Nothing can run: the run is over, with a result if another scheduler ended it.
            return sched.lock().result(pid).unwrap_or(Err(RunError::Deadlock));
        }
    }
}

/// A helper scheduler: schedule until the run is over.
#[cfg(feature = "std")]
fn help(sys: &Lock<System>, wakeup: &Wakeup, index: usize) {
    let mut sched = Sched::new(sys, wakeup);
    loop {
        // Park while offline, or while the run's result waits for the main scheduler to
        // collect it, until the count is raised or the run is over.
        sched.park_while(|s| {
            !s.stopping && (index >= s.schedulers_online || !s.results.is_empty() || s.halted.is_some())
        });
        if sched.lock().stopping || !schedule(&mut sched) {
            return;
        }
    }
}

/// Run one scheduling step: the next process's time slice, with the system unlocked while its
/// instructions run. `false` when nothing can ever run again.
fn schedule(sched: &mut Sched<'_>) -> bool {
    let next = {
        let mut sys = sched.lock();
        if core::mem::take(&mut sched.owes) {
            sys.taking = sys.taking.saturating_sub(1);
        }
        sys.next()
    };
    match next {
        Next::Stuck => false,
        Next::Again => true,
        Next::Sleep => {
            sched.sleep();
            true
        }
        Next::Run(mut p) => {
            let before = p.reductions;
            let stop = interp::run(sched, &mut p);
            let mut sys = sched.lock();
            sys.finish(p, before, stop);
            // This scheduler takes its next process itself ([`Sched::owes`]).
            sys.taking += 1;
            drop(sys);
            sched.owes = true;
            true
        }
    }
}

/// What a call resolves to.
#[derive(Clone)]
pub enum Target {
    Native(Native),
    Code(Cp),
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::platform::{FileError, FileInfo, FileKind, Files, OpenMode, PlatformError, SeekFrom};

    /// A bundle holding module `m`, and a file system holding `/home/p/m.beam` and
    /// `/home/p/own.beam`, as a session's own directory would.
    struct Bundle {
        files: Home,
        app_attempts: Arc<AtomicUsize>,
        /// What the VM said of the console's reader, in order: 1 for listening, 2 for not.
        listening: Arc<AtomicUsize>,
    }

    struct Home {
        reading: Option<(Vec<u8>, bool)>,
        operations: Arc<AtomicUsize>,
    }

    impl Home {
        fn file(path: &str) -> Result<Vec<u8>, FileError> {
            match path {
                "/home/p/m.beam" => Ok(b"planted".to_vec()),
                "/home/p/own.beam" => Ok(b"own".to_vec()),
                "/home/p/refused.beam" => Ok(b"planted refusal".to_vec()),
                _ => Err(FileError::Enoent),
            }
        }
    }

    impl Files for Home {
        fn open(&mut self, path: &str, _mode: OpenMode) -> Result<u64, FileError> {
            self.operations.fetch_add(1, Ordering::Relaxed);
            self.reading = Some((Home::file(path)?, false));
            Ok(0)
        }

        fn close(&mut self, _handle: u64) {
            self.operations.fetch_add(1, Ordering::Relaxed);
            self.reading = None;
        }

        fn read(&mut self, _handle: u64, _len: usize) -> Result<Vec<u8>, FileError> {
            self.operations.fetch_add(1, Ordering::Relaxed);
            let (bytes, done) = self.reading.as_mut().ok_or(FileError::Ebadf)?;
            Ok(if core::mem::replace(done, true) { Vec::new() } else { bytes.clone() })
        }

        fn info(&mut self, path: &str, _follow: bool) -> Result<FileInfo, FileError> {
            self.operations.fetch_add(1, Ordering::Relaxed);
            let size = Home::file(path)?.len() as u64;
            Ok(FileInfo {
                unix: true,
                size,
                kind: FileKind::Regular,
                readable: true,
                writable: true,
                atime: 0,
                mtime: 0,
                ctime: 0,
                mode: 0o644,
                links: 1,
                inode: 0,
                uid: 0,
                gid: 0,
            })
        }

        fn write(&mut self, _: u64, _: &[u8]) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn pread(&mut self, _: u64, _: u64, _: usize) -> Result<Vec<u8>, FileError> {
            Err(FileError::Enotsup)
        }

        fn pwrite(&mut self, _: u64, _: u64, _: &[u8]) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn seek(&mut self, _: u64, _: SeekFrom) -> Result<u64, FileError> { Err(FileError::Enotsup) }

        fn truncate(&mut self, _: u64) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn sync(&mut self, _: u64) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn handle_info(&mut self, _: u64) -> Result<FileInfo, FileError> { Err(FileError::Enotsup) }

        fn list_dir(&mut self, _: &str) -> Result<Vec<Vec<u8>>, FileError> { Err(FileError::Enotsup) }

        fn make_dir(&mut self, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn delete(&mut self, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn del_dir(&mut self, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }

        fn rename(&mut self, _: &str, _: &str) -> Result<(), FileError> { Err(FileError::Enotsup) }
    }

    impl Platform for Bundle {
        fn monotonic_us(&mut self) -> u64 { 0 }

        fn system_time_us(&mut self) -> Option<u64> { None }

        fn idle(&mut self, _deadline: Option<u64>) {}

        fn console_write(&mut self, _bytes: &[u8]) {}

        fn console_listening(&mut self, listening: bool) {
            let said = self.listening.load(Ordering::Relaxed);
            self.listening.store(said * 10 + if listening { 1 } else { 2 }, Ordering::Relaxed);
        }

        fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

        fn load_module(&mut self, module: &str) -> Lookup {
            match module {
                "m" => Lookup::Found(b"bundle".to_vec()),
                "refused" => Lookup::Refused,
                _ => Lookup::Absent,
            }
        }

        fn load_app(&mut self, app: &str) -> Lookup {
            self.app_attempts.fetch_add(1, Ordering::Relaxed);
            match app {
                "found" => Lookup::Found(b"not an application spec".to_vec()),
                "refused" => Lookup::Refused,
                _ => Lookup::Absent,
            }
        }

        fn files(&mut self) -> Option<&mut dyn Files> {
            self.files.operations.fetch_add(1, Ordering::Relaxed);
            Some(&mut self.files)
        }
    }

    /// A VM whose code path has the session's directory in front, put there by what
    /// `code:add_patha/1` calls, behind a directory already on the path.
    fn vm_with_operations() -> (Vm, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let operations = Arc::new(AtomicUsize::new(0));
        let app_attempts = Arc::new(AtomicUsize::new(0));
        let mut vm = Vm::new(Box::new(Bundle {
            files: Home { reading: None, operations: Arc::clone(&operations) },
            app_attempts: Arc::clone(&app_attempts),
            listening: Arc::new(AtomicUsize::new(0)),
        }));
        let sys = vm.sys.get_mut();
        assert!(sys.add_code_path("/lib".into(), false));
        assert!(sys.add_code_path("/home/p".into(), true));
        (vm, operations, app_attempts)
    }

    /// The platform hears when the console gets a reader and when it has none (the reader's
    /// exit or the input's end), once each: with nobody reading, input it holds must not wake the
    /// VM. A second reader in place of the first changes nothing it is told.
    #[test]
    fn the_platform_hears_the_console_reader_come_and_go() {
        let listening = Arc::new(AtomicUsize::new(0));
        let mut vm = Vm::new(Box::new(Bundle {
            files: Home { reading: None, operations: Arc::new(AtomicUsize::new(0)) },
            app_attempts: Arc::new(AtomicUsize::new(0)),
            listening: Arc::clone(&listening),
        }));
        let sys = vm.sys.get_mut();
        let (first, second) =
            (Pid { serial: 1, index: 1, port: false }, Pid { serial: 1, index: 2, port: false });
        sys.set_console_reader(Some(first));
        sys.set_console_reader(Some(second));
        sys.set_console_reader(None);
        assert_eq!(listening.load(Ordering::Relaxed), 12, "listening once, then not once");
    }

    #[test]
    fn the_bundle_wins_over_a_front_directory() {
        let (mut vm, operations, _) = vm_with_operations();
        let found = vm.sys.get_mut().locate_module("m");
        assert!(matches!(found, Some(Found::Platform(b)) if b == b"bundle"));
        assert_eq!(operations.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_name_the_bundle_lacks_is_found_on_the_path() {
        let (mut vm, operations, _) = vm_with_operations();
        let found = vm.sys.get_mut().locate_module("own");
        assert!(matches!(found, Some(Found::Path(p, b)) if p == "/home/p/own.beam" && b == b"own"));
        assert!(operations.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn a_refused_system_module_never_touches_the_code_path() {
        let (mut vm, operations, _) = vm_with_operations();
        let found = vm.sys.get_mut().locate_module("refused");
        assert_eq!(operations.load(Ordering::Relaxed), 0);
        assert!(found.is_none());
    }

    #[test]
    fn app_spec_uses_one_source_attempt_and_keeps_its_erlang_result() {
        let (mut vm, operations, attempts) = vm_with_operations();
        for app in ["absent", "refused", "found"] {
            let name = vm.atom(app);
            let pid = vm.spawn("application", "load", |_| alloc::vec![name]).unwrap();
            let path_operations = operations.load(Ordering::Relaxed);
            let result = vm.run(pid).unwrap().unwrap();
            let outer = result.heap().as_tuple(result.term()).unwrap();
            assert!(matches!(outer[0], Term::Atom(a) if a.as_str() == "error"));
            if app == "found" {
                // Invalid bytes were returned to application:load/1 and parsed as a bad spec.
                assert!(result.to_string().contains("bad_application"), "{app}: {result}");
            } else {
                let detail = result.heap().as_tuple(outer[1]).unwrap();
                let chars = |term| {
                    result
                        .heap()
                        .to_vec(term)
                        .unwrap()
                        .into_iter()
                        .map(|t| match t {
                            Term::Int(n) => n as u8,
                            other => panic!("{other:?}"),
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(chars(detail[0]), b"no such file or directory");
                assert_eq!(chars(detail[1]), alloc::format!("{app}.app").as_bytes());
                assert_eq!(
                    operations.load(Ordering::Relaxed),
                    path_operations,
                    "{app}: app lookup never searched the code path"
                );
            }
            assert_eq!(
                attempts.load(Ordering::Relaxed),
                match app {
                    "absent" => 1,
                    "refused" => 2,
                    "found" => 3,
                    _ => unreachable!(),
                }
            );
        }
    }
}
