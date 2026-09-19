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
use crate::platform::{ConsoleInput, Platform};
use crate::process::{Class, Cp, Exception, Process, State};
use crate::term::{copy, Heap, Literals, OwnedTerm, Pid, Ref, Term};

/// Reductions (calls) a process may run before it is preempted.
pub const TIME_SLICE: usize = 2000;
/// Most processes alive at once. Spawning more raises `system_limit`.
pub const MAX_PROCESSES: usize = 1 << 16;

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
}

/// Everything but the process table. The running process is borrowed separately, so native
/// functions get `&mut System` and `&mut Process` at the same time.
pub struct System {
    pub platform: Box<dyn Platform>,
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
    /// The literal chunks: modules' constants and persistent terms.
    pub literals: Literals,
    pub atom_table: AtomTable,
    pub atoms: Atoms,
    modules: BTreeMap<String, Arc<Module>>,
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
    /// Open ports, by the pid of the port's process, and the port behind each program handle.
    pub(crate) ports: BTreeMap<Pid, crate::bif::port::PortState>,
    pub(crate) program_ports: BTreeMap<u64, Pid>,
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
    /// Where the platform's modules come in the code path: directories before this index
    /// (added with `code:add_patha/1`) are searched before the platform, the rest after it.
    pub(crate) platform_at: usize,
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
}

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
const LOGGER_FALLBACK: &[&[u8]] = &[
    include_bytes!("../lib/logger.beam"),
    include_bytes!("../lib/error_logger.beam"),
];

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
                (self.slots.len() - 1) as u32
            }
        };
        let pid = Pid {
            index,
            serial,
            port,
        };
        self.slots[index as usize] = Slot::Running { pid };
        Some(pid)
    }

    pub(crate) fn get_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        match self.slots.get_mut(pid.index as usize) {
            Some(Slot::Present(p)) if p.pid == pid => Some(p),
            _ => None,
        }
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

    fn put(&mut self, p: Box<Process>) {
        let index = p.pid.index as usize;
        self.slots[index] = Slot::Present(p);
    }

    pub(crate) fn count(&self) -> usize {
        self.live
    }

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
        if self.is_alive(pid) {
            self.inboxes.get_mut(pid.index as usize)
        } else {
            None
        }
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
        self.slots[pid.index as usize] = Slot::Free;
        self.free.push(pid.index);
        self.live -= 1;
    }
}

/// BEAM's preloaded modules that only make sense on top of its C runtime (ports, the file
/// system, the boot process, tracing). This VM does their job itself or not at all, so they are
/// never loaded, even if found on the code path; calls to them are `undef` unless a native
/// answers. (`erlang`, `erts_internal`, `persistent_term`, `atomics` and `counters` do load:
/// their Erlang code is useful and their NIF stubs are replaced by natives.)
pub const RUNTIME_MODULES: &[&str] = &[
    "init",
    "erl_init",
    "erl_prim_loader",
    "erl_tracer",
    "erts_code_purger",
    "erts_dirty_process_signal_handler",
    "erts_literal_area_collector",
    "erts_trace_cleaner",
    "prim_eval",
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
    pub sys: System,
}

impl Vm {
    pub fn new(platform: Box<dyn Platform>) -> Vm {
        Vm::with_limits(platform, Limits::default())
    }

    pub fn with_limits(platform: Box<dyn Platform>, limits: Limits) -> Vm {
        Vm::with_config(
            platform,
            Config {
                limits,
                natives: &[],
            },
        )
    }

    /// A VM with resource limits and extra natives chosen by the embedder.
    pub fn with_config(platform: Box<dyn Platform>, config: Config) -> Vm {
        let limits = config.limits;
        let mut atom_table = AtomTable::new();
        let atoms = Atoms::new(&mut atom_table);
        let natives = bif::Registry::new(config.natives);
        Vm {
            sys: System {
                platform,
                limits,
                ets: crate::ets::Tables::default(),
                persistent: BTreeMap::new(),
                literals: Literals::default(),
                env: BTreeMap::new(),
                aliases: BTreeMap::new(),
                atom_table,
                atoms,
                modules: BTreeMap::new(),
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
                ports: BTreeMap::new(),
                program_ports: BTreeMap::new(),
                halted: None,
                console_reader: None,
                backtrace_depth: 8,
                code_path: Vec::new(),
                platform_at: 0,
                lib_roots: Vec::new(),
                module_files: BTreeMap::new(),
                profile: None,
                resolved: BTreeMap::new(),
                stats: Stats::default(),
            },
        }
        .boot()
    }

    /// Start the console I/O servers, `user` and `standard_error`.
    fn boot(mut self) -> Vm {
        self.sys.stats.start_us = self.sys.platform.monotonic_us();
        for module in EMBEDDED {
            self.sys.load(module).expect("embedded modules load");
        }
        let real_logger = self.sys.platform.load_module("logger").is_some()
            && self.sys.platform.load_module("logger_sup").is_some();
        if !real_logger {
            for module in LOGGER_FALLBACK {
                self.sys.load(module).expect("embedded modules load");
            }
        }
        // The shell `os:cmd/1` runs commands with (the kernel sets this at start). Programs run
        // outside the VM, so this is the host's shell, whatever the VM's file system holds.
        let key = self.atom("kernel_os_cmd_shell");
        let mut h = Heap::new(&self.sys.literals);
        let shell = h.string("/bin/sh");
        let shell = self.sys.make_literal(&h, shell);
        self.sys.persistent.insert(OwnedTerm::immediate(key), shell);
        let user_name = self.atom("user");
        let user = self
            .spawn("beamlet_io", "start", |_| alloc::vec![user_name])
            .expect("spawn user");
        let stderr = self.atom("standard_error");
        let err = self
            .spawn("beamlet_io", "start", |_| alloc::vec![stderr])
            .expect("spawn standard_error");
        for pid in [user, err] {
            if let Some(p) = self.sys.procs.get_mut(pid) {
                p.group_leader = Some(user);
            }
        }
        self.sys.default_group_leader = Some(user);
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
    pub fn load(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> {
        self.sys.load(bytes)
    }

    /// Spawn `module:function(args)` as a new process, loading the module if needed. `args`
    /// builds the arguments on the new process's heap.
    pub fn spawn(
        &mut self,
        module: &str,
        function: &str,
        args: impl FnOnce(&mut Heap) -> Vec<Term>,
    ) -> Result<Pid, OwnedException> {
        let m = self.sys.atom(module);
        let f = self.sys.atom(function);
        let mut heap = Heap::new(&self.sys.literals);
        let args = args(&mut heap);
        self.sys
            .spawn(&m, &f, heap, args)
            .map_err(|e| OwnedException {
                class: e.class,
                reason: OwnedTerm::immediate(e.reason),
                trace: None,
            })
    }

    /// Run until process `pid` ends, and return its result: the value its first function
    /// returned, or the exception that ended it.
    pub fn run(&mut self, pid: Pid) -> Result<Outcome, RunError> {
        self.sys.watched.insert(pid);
        loop {
            if let Some(status) = self.sys.halted {
                return Err(RunError::Halted(status));
            }
            if let Some(r) = self.sys.results.remove(&pid) {
                self.sys.watched.remove(&pid);
                return Ok(r);
            }
            if !self.sys.step() {
                return Err(RunError::Deadlock);
            }
        }
    }

    /// Set a variable of the VM's own environment (`os:getenv/1`), which starts empty.
    pub fn setenv(&mut self, name: &str, value: &str) {
        self.sys.env.insert(String::from(name), String::from(value));
    }

    /// Add a directory of the VM's file system where applications live (`App-Vsn/ebin`,
    /// `App-Vsn/priv`, `App-Vsn/include`), searched by `code:lib_dir/1`.
    pub fn add_lib_root(&mut self, dir: &str) {
        self.sys.lib_roots.push(String::from(dir));
    }

    /// Start sampling where processes are at the end of each time slice (a statistical profile
    /// for finding hot code; see [`Vm::profile`]).
    pub fn enable_profile(&mut self) {
        self.sys.profile = Some(BTreeMap::new());
    }

    /// The samples so far, most frequent first: `(count, "m:f/a < caller < ...")`.
    pub fn profile(&self) -> Vec<(u64, String)> {
        let mut v: Vec<(u64, String)> = self
            .sys
            .profile
            .iter()
            .flatten()
            .map(|(k, n)| (*n, k.clone()))
            .collect();
        v.sort_by(|a, b| b.cmp(a));
        v
    }

    /// Like [`Vm::run`], but give up after `max_steps` scheduling steps and return `None`.
    /// For tests that run untrusted code which may legitimately loop forever.
    pub fn run_bounded(&mut self, pid: Pid, max_steps: usize) -> Option<Result<Outcome, RunError>> {
        self.sys.watched.insert(pid);
        for _ in 0..max_steps {
            if let Some(status) = self.sys.halted {
                return Some(Err(RunError::Halted(status)));
            }
            if let Some(r) = self.sys.results.remove(&pid) {
                self.sys.watched.remove(&pid);
                return Some(Ok(r));
            }
            if !self.sys.step() {
                return Some(Err(RunError::Deadlock));
            }
        }
        None
    }

    pub fn atom(&mut self, name: &str) -> Term {
        Term::Atom(self.sys.atom(name))
    }
}

impl System {
    /// Intern an atom the VM needs. Only for names from code or the embedder, which are short.
    pub fn atom(&mut self, name: &str) -> Atom {
        self.atom_table
            .intern(name)
            .expect("VM-internal atom names are within limits")
    }

    pub fn make_ref(&mut self) -> Ref {
        let r = Ref(self.next_ref);
        self.next_ref += 1;
        r
    }

    /// Load a module from bytes, replacing any module of the same name.
    pub fn load_bytes(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> {
        self.load(bytes)
    }

    fn load(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> {
        let mut module = loader::load(bytes, &mut self.atom_table, &mut self.literals)?;
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
            let entry = f.start as usize + 1;
            if module
                .code
                .get(entry)
                .is_some_and(|i| i.op == crate::opcodes::LABEL)
            {
                let index = module.body_natives.len() as u64;
                module.body_natives.push((n, f.name, f.arity));
                module.code[entry] = crate::module::Instr {
                    op: crate::module::NATIVE_BODY,
                    args: alloc::vec![crate::module::Arg::U(index)],
                };
            }
        }
        let name = module.name;
        self.modules
            .insert(name.as_str().to_string(), Arc::new(module));
        self.resolved.clear();
        Ok(name)
    }

    /// The module named `name`, loading it through the platform on first use.
    pub fn module(&mut self, name: &Atom) -> Option<Arc<Module>> {
        if let Some(m) = self.modules.get(name.as_str()) {
            return Some(m.clone());
        }
        if RUNTIME_MODULES.contains(&name.as_str()) {
            return None;
        }
        let bytes = match self.locate_module(name.as_str())? {
            Found::Path(_, bytes) | Found::Platform(bytes) => bytes,
        };
        let loaded = self.load(&bytes).ok()?;
        if &loaded != name {
            // A file that claims to be a different module than the one asked for.
            self.modules.remove(loaded.as_str());
            return None;
        }
        self.modules.get(name.as_str()).cloned()
    }

    /// Where `module`'s code is, in code path order: the directories added in front of the
    /// platform's modules, the platform, then the other directories.
    pub(crate) fn locate_module(&mut self, module: &str) -> Option<Found> {
        if let Some((path, bytes)) = self.find_in_code_path(module, true) {
            return Some(Found::Path(path, bytes));
        }
        if let Some(bytes) = self.platform.load_module(module) {
            return Some(Found::Platform(bytes));
        }
        self.find_in_code_path(module, false)
            .map(|(path, bytes)| Found::Path(path, bytes))
    }

    /// `Module.beam` from the first directory of the VM's code path that has it, among those
    /// before the platform (`front`) or after it: its path and its bytes.
    fn find_in_code_path(&mut self, module: &str, front: bool) -> Option<(String, Vec<u8>)> {
        let max = self.limits.max_binary_bits / 8;
        let dirs = if front {
            &self.code_path[..self.platform_at]
        } else {
            &self.code_path[self.platform_at..]
        };
        if dirs.is_empty() {
            return None;
        }
        let files = self.platform.files()?;
        for dir in dirs {
            let path = alloc::format!("{}/{}.beam", dir.trim_end_matches('/'), module);
            if let Ok(bytes) = crate::bif::read_whole_file(files, &path, max) {
                return Some((path, bytes));
            }
        }
        None
    }

    /// The checksum of a loaded module (without loading it).
    pub fn loaded_md5(&self, name: &Atom) -> Option<[u8; 16]> {
        self.modules.get(name.as_str()).map(|m| m.md5)
    }

    pub fn is_loaded(&self, name: &Atom) -> bool {
        self.modules.contains_key(name.as_str())
    }

    /// Unload a module (`code:delete/1`): new calls no longer reach it, while code already
    /// running in it finishes (it is reference counted). A later call loads it afresh through
    /// the platform, if the platform has it. `false` if it was not loaded.
    pub fn delete_module(&mut self, name: &Atom) -> bool {
        self.resolved.clear();
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
                Target::Code(Cp {
                    module: m,
                    pc: entry,
                })
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
    pub fn spawn_copy(
        &mut self,
        entry: Cp,
        src: &Heap,
        args: &[Term],
        port: bool,
    ) -> Result<Pid, Exception> {
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
        let pid = self
            .procs
            .allocate(port)
            .ok_or_else(|| Exception::error(Term::Atom(self.atoms.system_limit)))?;
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

    /// Queue a message for `to`, built by `build` on its heap.
    pub fn send_with(&mut self, to: Pid, build: impl FnOnce(&mut Heap) -> Term) {
        let Some(inbox) = self.procs.inbox(to) else {
            return;
        };
        if inbox.len() >= self.limits.max_mailbox {
            let reason = mailbox_full(&mut self.atom_table, &self.atoms);
            self.exits.push_back(ExitSignal {
                target: to,
                from: to,
                reason,
                from_link: false,
                forced: true,
            });
            return;
        }
        inbox.push_back(OwnedTerm::build(&self.literals, build));
        if let Some(p) = self.procs.get_mut(to) {
            if p.state == State::Waiting {
                p.state = State::Runnable;
                self.run_queue.push_back(to);
            }
        }
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

    /// A copy of `t` (a term of `src`) as a literal: in a chunk of its own, never freed.
    pub fn make_literal(&mut self, src: &Heap, t: Term) -> Term {
        let mut heap = Heap::new(&Literals::default());
        let mut roots = [copy(src, t, &mut heap)];
        self.literals.add(heap, &mut roots);
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

    pub fn now_us(&mut self) -> u64 {
        self.platform.monotonic_us()
    }

    /// Run one scheduling step. Returns `false` when nothing can ever run again.
    fn step(&mut self) -> bool {
        self.deliver_exits();
        self.fire_timers();
        self.poll_console();
        self.poll_programs();
        let Some(pid) = self.run_queue.pop_front() else {
            // Nothing runnable: sleep until the next timer or console input, or give up if
            // nothing can ever arrive.
            return match self.timers.first() {
                Some(&(deadline, _)) => {
                    self.platform.idle(Some(deadline));
                    true
                }
                None if self.console_reader.is_some() || !self.program_ports.is_empty() => {
                    self.platform.idle(None);
                    true
                }
                None => !self.exits.is_empty(),
            };
        };
        let Some(mut p) = self.procs.take(pid) else {
            return true;
        };
        if p.state != State::Runnable {
            self.procs.put(p);
            return true;
        }
        p.budget = TIME_SLICE;
        p.refresh(&self.literals);
        let before = p.reductions;
        let mut stop = interp::run(self, &mut p);
        self.stats.reductions += p.reductions - before;
        if let Some(profile) = &mut self.profile {
            *profile.entry(crate::interp::where_is(&p, 3)).or_default() += 1;
        }
        self.stats.context_switches += 1;
        if matches!(stop, Stop::Yield | Stop::Wait) && self.over_memory(&mut p) {
            stop = Stop::Exit(Err(Exception::exit(Term::Atom(self.atoms.killed))));
        }
        match stop {
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
        true
    }

    /// Whether `p` must be killed for holding too much memory. A heap over a limit is collected
    /// first: what counts is what is live, as with BEAM's `max_heap_size`.
    fn over_memory(&mut self, p: &mut Process) -> bool {
        let vm_limit = self.limits.max_heap_words;
        let own = p.max_heap;
        let over = |p: &Process| {
            let usage = crate::memory::process(p);
            let used = if own.include_shared_binaries {
                usage.total_words()
            } else {
                usage.words
            };
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
        let now = self.platform.monotonic_us();
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
            }
        }
    }

    /// End process `p`: tell its links and monitors, then free its slot.
    /// Pass console input, if any has arrived, to the process reading it.
    fn poll_console(&mut self) {
        let Some(reader) = self.console_reader else {
            return;
        };
        let input = match self.platform.console_read() {
            ConsoleInput::Nothing => return,
            ConsoleInput::Data(bytes) => Some(bytes),
            ConsoleInput::Eof => {
                self.console_reader = None;
                None
            }
        };
        let (tag, eof) = (
            Term::Atom(self.atom("beamlet_console")),
            Term::Atom(self.atom("eof")),
        );
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
        let [emulator, tag, error, error_logger, gl, pid_key, time_key, log] = [
            "emulator",
            "tag",
            "error",
            "error_logger",
            "gl",
            "pid",
            "time",
            "log",
        ]
        .map(|n| atom(self, n));
        let true_ = Term::Atom(self.atoms.true_);
        let time = self.platform.system_time_us().unwrap_or(0) as i64;
        let (pid, leader) = (Term::Pid(p.pid), Term::Pid(p.group_leader.unwrap_or(p.pid)));
        self.send_with(logger, |h| {
            let format = h.string("Error in process ~p with exit value:~n~p~n");
            let el = h.map_from([(emulator, true_), (tag, error)]);
            let meta = h.map_from([
                (error_logger, el),
                (gl, leader),
                (pid_key, pid),
                (time_key, Term::Int(time)),
            ]);
            let reason = reason.copy_into(h);
            let args = h.list([pid, reason]);
            h.tuple(&[log, error, format, args, meta])
        });
    }

    /// Close the files `pid` opened.
    fn close_files(&mut self, pid: Pid) {
        let handles: Vec<u64> = self
            .files
            .iter()
            .filter(|(_, &o)| o == pid)
            .map(|(&h, _)| h)
            .collect();
        for h in handles {
            self.files.remove(&h);
            if let Some(f) = self.platform.files() {
                f.close(h);
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
            self.console_reader = None;
        }
        if let Some(name) = &p.registered_name {
            self.registered.remove(name.as_str());
        }
        for &other in &p.links {
            if let Some(o) = self.procs.get_mut(other) {
                o.links.remove(&pid);
            }
            self.exits.push_back(ExitSignal {
                target: other,
                from: pid,
                reason: reason.clone(),
                from_link: true,
                forced: false,
            });
        }
        let kind = if pid.port {
            Term::Atom(self.atom("port"))
        } else {
            Term::Atom(self.atoms.process)
        };
        let down = Term::Atom(self.atoms.down);
        for (
            r,
            crate::process::Monitor {
                watcher,
                object,
                tag,
            },
        ) in &p.monitored_by
        {
            if let Some(w) = self.procs.get_mut(*watcher) {
                w.monitors.remove(r);
            }
            // A monitor's alias ends when the monitor fires.
            if self
                .aliases
                .get(r)
                .is_some_and(|a| a.mode != AliasMode::Explicit)
            {
                self.aliases.remove(r);
            }
            self.send_with(*watcher, |h| {
                let tag = tag.as_ref().map_or(down, |t| t.copy_into(h));
                let object = object.copy_into(h);
                let reason = reason.copy_into(h);
                h.tuple(&[tag, Term::Ref(*r), kind, object, reason])
            });
        }
        for (r, target) in &p.monitors {
            if let Some(t) = self.procs.get_mut(*target) {
                t.monitored_by.remove(r);
            }
        }
        // Its ETS tables go to their heirs, or are deleted.
        for tid in self.ets.owned_by(pid) {
            let heir = self.ets.get(tid).and_then(|t| t.heir.clone());
            match heir {
                Some((to, data)) if to != pid && self.procs.is_alive(to) => {
                    let t = self.ets.get_mut(tid).expect("listed");
                    t.owner = to;
                    let id = if t.named {
                        Term::Atom(t.name)
                    } else {
                        Term::Ref(Ref(t.tid))
                    };
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
        }
        drop(p);
        self.procs.release(pid);
    }

    /// Deliver queued exit signals. A signal either becomes an `{'EXIT', From, Reason}` message
    /// (the target traps exits), is ignored (reason `normal`), or kills the target.
    fn deliver_exits(&mut self) {
        while let Some(ExitSignal {
            target,
            from,
            reason,
            from_link,
            forced,
        }) = self.exits.pop_front()
        {
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
                let reason = if kill {
                    Term::Atom(self.atoms.killed)
                } else {
                    reason.copy_into(&mut p.heap)
                };
                // Remove it from the run queue lazily: `step` skips pids that are gone.
                self.terminate(p, Err(Exception::exit(reason)));
            }
        }
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
    Arc::new(OwnedTerm::build(&Literals::default(), |h| {
        h.tuple(&[limit, queue])
    }))
}

/// Queue a message. `false` if the mailbox is full: the caller must then end the receiver.
pub(crate) fn deliver(
    p: &mut Process,
    msg: Term,
    run_queue: &mut VecDeque<Pid>,
    max_mailbox: usize,
) -> bool {
    if p.mailbox.len() >= max_mailbox {
        return false;
    }
    p.mailbox.push_back(msg);
    if p.state == State::Waiting {
        p.state = State::Runnable;
        run_queue.push_back(p.pid);
    }
    true
}

/// What a call resolves to.
#[derive(Clone)]
pub enum Target {
    Native(Native),
    Code(Cp),
}
