//! The VM: module registry, process table and the scheduler.
//!
//! One VM runs on one thread. Processes are scheduled round-robin, each for a fixed number of
//! reductions (function calls) before it must yield, so a busy process cannot starve the others.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::atom::{Atom, AtomTable, Atoms};
use crate::bif::{self, Native};
use crate::interp::{self, Stop};
use crate::loader::{self, LoadError};
use crate::module::Module;
use crate::platform::{ConsoleInput, Platform};
use crate::process::{Class, Cp, Exception, Process, State};
use crate::term::{Pid, Ref, Term};

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
            max_heap_words: 1 << 27, // 1 GiB of words
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
    /// `persistent_term`: VM-wide terms, written rarely and read often.
    pub persistent: BTreeMap<crate::term::MapKey, Term>,
    pub atom_table: AtomTable,
    pub atoms: Atoms,
    modules: BTreeMap<String, Rc<Module>>,
    natives: bif::Registry,
    pub(crate) run_queue: VecDeque<Pid>,
    /// Everything waiting for a time: receive timeouts and message timers, by deadline.
    timers: BTreeSet<(u64, Timer)>,
    /// Message timers (`send_after`, `start_timer`) by reference: deadline, target, message.
    pub(crate) message_timers: BTreeMap<Ref, (u64, Term, Term)>,
    next_ref: u64,
    pub(crate) registered: BTreeMap<String, Pid>,
    pub(crate) procs: ProcTable,
    /// Exit signals waiting to be delivered.
    pub(crate) exits: VecDeque<ExitSignal>,
    /// Final results of processes someone is waiting for through [`Vm::run`].
    results: BTreeMap<Pid, Result<Term, Exception>>,
    watched: BTreeSet<Pid>,
    /// The group leader of processes that do not inherit one: the console I/O server.
    pub(crate) default_group_leader: Option<Pid>,
    /// This VM's working directory in the platform's file system (`file:get_cwd/0`).
    pub(crate) cwd: String,
    /// Open files: platform handle, and the process that opened it.
    pub(crate) files: BTreeMap<u64, Pid>,
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
    /// Samples of where processes are at the end of each time slice (the top few functions),
    /// when profiling is on (`Vm::enable_profile`).
    pub(crate) profile: Option<BTreeMap<String, u64>>,
    /// What `resolve` found for `(module, function, arity)`, by atom identity. Emptied whenever
    /// a module is loaded or deleted, so it never holds stale code.
    resolved: BTreeMap<(usize, usize, u32), Target>,
    pub(crate) stats: Stats,
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
    include_bytes!("../lib/logger.beam"),
    include_bytes!("../lib/error_logger.beam"),
    include_bytes!("../lib/application.beam"),
    include_bytes!("../lib/gen_tcp.beam"),
    include_bytes!("../lib/beamlet_tcp.beam"),
    include_bytes!("../lib/beamlet_code.beam"),
];

enum Slot {
    Free,
    Present(Box<Process>),
    /// Taken out by the scheduler while it runs.
    Running { serial: u32 },
}

pub(crate) struct ProcTable {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
    /// The serial of the next process. One counter for the whole table, so pids order by
    /// creation (as BEAM's do, which code relies on through map and `lists:sort` order) and a
    /// stale pid never matches a reused slot.
    next_serial: u32,
}

impl ProcTable {
    fn new() -> ProcTable {
        ProcTable { slots: Vec::new(), free: Vec::new(), live: 0, next_serial: 0 }
    }

    fn allocate(&mut self) -> Option<Pid> {
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
                (self.slots.len() - 1) as u32
            }
        };
        self.slots[index as usize] = Slot::Running { serial };
        Some(Pid { index, serial })
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
            Some(Slot::Running { serial }) => *serial == pid.serial,
            _ => false,
        }
    }

    fn take(&mut self, pid: Pid) -> Option<Box<Process>> {
        let slot = self.slots.get_mut(pid.index as usize)?;
        match slot {
            Slot::Present(p) if p.pid == pid => {
                let Slot::Present(p) = core::mem::replace(slot, Slot::Running { serial: pid.serial }) else {
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
            .enumerate()
            .filter_map(|(i, s)| match s {
                Slot::Present(p) => Some(p.pid),
                Slot::Running { serial } => Some(Pid { index: i as u32, serial: *serial }),
                Slot::Free => None,
            })
            .collect()
    }

    fn release(&mut self, pid: Pid) {
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
    "zlib",
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
    pub reason: Term,
    pub from_link: bool,
    /// Imposed by the VM (a resource limit): cannot be trapped, and `reason` is used as is.
    pub forced: bool,
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
        Vm::with_config(platform, Config { limits, natives: &[] })
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
                halted: None,
                console_reader: None,
                backtrace_depth: 8,
                code_path: Vec::new(),
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
        let user_name = self.atom("user");
        let user = self.spawn("beamlet_io", "start", alloc::vec![user_name]).expect("spawn user");
        let stderr = self.atom("standard_error");
        let err = self.spawn("beamlet_io", "start", alloc::vec![stderr]).expect("spawn standard_error");
        for pid in [user, err] {
            if let Some(p) = self.sys.procs.get_mut(pid) {
                p.group_leader = Some(user);
            }
        }
        self.sys.default_group_leader = Some(user);
        // OTP's file server, which `file` calls for most operations: started now, so that it is
        // registered before any other code runs (about 2 ms). Without a file system it still
        // answers `get_cwd`, which compilers ask for; file operations fail with `enotsup`.
        if let Ok(pid) = self.spawn("file_server", "start", Vec::new()) {
            let _ = self.run_bounded(pid, 100_000);
        }
        self
    }

    /// Load a module from `.beam` bytes, replacing any module of the same name.
    pub fn load(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> {
        self.sys.load(bytes)
    }

    /// Spawn `module:function(args)` as a new process, loading the module if needed.
    pub fn spawn(&mut self, module: &str, function: &str, args: Vec<Term>) -> Result<Pid, Exception> {
        let m = self.sys.atom(module);
        let f = self.sys.atom(function);
        self.sys.spawn(&m, &f, args)
    }

    /// Run until process `pid` ends, and return its result: the value its first function
    /// returned, or the exception that ended it.
    pub fn run(&mut self, pid: Pid) -> Result<Result<Term, Exception>, RunError> {
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

    /// Start sampling where processes are at the end of each time slice (a statistical profile
    /// for finding hot code; see [`Vm::profile`]).
    pub fn enable_profile(&mut self) {
        self.sys.profile = Some(BTreeMap::new());
    }

    /// The samples so far, most frequent first: `(count, "m:f/a < caller < ...")`.
    pub fn profile(&self) -> Vec<(u64, String)> {
        let mut v: Vec<(u64, String)> =
            self.sys.profile.iter().flatten().map(|(k, n)| (*n, k.clone())).collect();
        v.sort_by(|a, b| b.cmp(a));
        v
    }

    /// Like [`Vm::run`], but give up after `max_steps` scheduling steps and return `None`.
    /// For tests that run untrusted code which may legitimately loop forever.
    pub fn run_bounded(&mut self, pid: Pid, max_steps: usize) -> Option<Result<Result<Term, Exception>, RunError>> {
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
        self.atom_table.intern(name).expect("VM-internal atom names are within limits")
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
        let mut module = loader::load(bytes, &mut self.atom_table)?;
        for imp in &mut module.imports {
            imp.native = self.natives.get(&imp.module, &imp.function, imp.arity);
        }
        // Functions implemented natively (NIF stubs like `crypto:hash_nif/2`): replace each
        // body's entry, the `label` right after `func_info`, with a call to the native. Local
        // calls, exports and funs all enter through that label.
        let functions = module.functions.clone();
        for f in &functions {
            let Some(n) = self.natives.get(&module.name, &f.name, f.arity) else { continue };
            let entry = f.start as usize + 1;
            if module.code.get(entry).is_some_and(|i| i.op == crate::opcodes::LABEL) {
                let index = module.body_natives.len() as u64;
                module.body_natives.push((n, f.name.clone(), f.arity));
                module.code[entry] = crate::module::Instr { op: crate::module::NATIVE_BODY, args: alloc::vec![crate::module::Arg::U(index)] };
            }
        }
        let name = module.name.clone();
        self.modules.insert(name.as_str().to_string(), Rc::new(module));
        self.resolved.clear();
        Ok(name)
    }

    /// The module named `name`, loading it through the platform on first use.
    pub fn module(&mut self, name: &Atom) -> Option<Rc<Module>> {
        if let Some(m) = self.modules.get(name.as_str()) {
            return Some(m.clone());
        }
        if RUNTIME_MODULES.contains(&name.as_str()) {
            return None;
        }
        let bytes = match self.platform.load_module(name.as_str()) {
            Some(b) => b,
            None => self.find_in_code_path(name.as_str())?.1,
        };
        let loaded = self.load(&bytes).ok()?;
        if &loaded != name {
            // A file that claims to be a different module than the one asked for.
            self.modules.remove(loaded.as_str());
            return None;
        }
        self.modules.get(name.as_str()).cloned()
    }

    /// `Module.beam` from the first directory of the VM's code path that has it: its path and
    /// its bytes.
    pub(crate) fn find_in_code_path(&mut self, module: &str) -> Option<(String, Vec<u8>)> {
        let max = self.limits.max_binary_bits / 8;
        let files = self.platform.files()?;
        for dir in &self.code_path {
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

    pub fn spawn(&mut self, module: &Atom, function: &Atom, args: Vec<Term>) -> Result<Pid, Exception> {
        let entry = match self.resolve(module, function, args.len() as u32) {
            Some(Target::Code(cp)) => cp,
            // Spawning a native directly: run it through a tiny trampoline is not supported yet.
            _ => return Err(Exception::error(Term::Atom(self.atoms.undef.clone()))),
        };
        self.spawn_at(entry, args)
    }

    pub fn spawn_at(&mut self, entry: Cp, args: Vec<Term>) -> Result<Pid, Exception> {
        let pid = self
            .procs
            .allocate()
            .ok_or_else(|| Exception::error(Term::Atom(self.atoms.system_limit.clone())))?;
        let mut p = Process::new(pid, entry, args);
        p.group_leader = self.default_group_leader;
        self.procs.put(Box::new(p));
        self.run_queue.push_back(pid);
        Ok(pid)
    }

    /// Queue `msg` for `to`. Sending to a dead process silently does nothing, as in Erlang.
    pub fn send(&mut self, to: Pid, msg: Term) {
        if let Some(p) = self.procs.get_mut(to) {
            if !deliver(p, msg, &mut self.run_queue, self.limits.max_mailbox) {
                let reason = mailbox_full(&mut self.atom_table, &self.atoms);
                self.exits.push_back(ExitSignal { target: to, from: to, reason, from_link: false, forced: true });
            }
        }
    }

    /// Start a message timer: at `deadline`, send `msg` to `to` (a pid or registered name).
    pub fn start_message_timer(&mut self, deadline: u64, to: Term, msg: Term) -> Option<Ref> {
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
    fn send_to_term(&mut self, to: &Term, msg: Term) {
        let pid = match to {
            Term::Pid(p) => Some(*p),
            Term::Atom(name) => self.registered.get(name.as_str()).copied(),
            _ => None,
        };
        if let Some(pid) = pid {
            self.send(pid, msg);
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
        let Some(pid) = self.run_queue.pop_front() else {
            // Nothing runnable: sleep until the next timer or console input, or give up if
            // nothing can ever arrive.
            return match self.timers.first() {
                Some(&(deadline, _)) => {
                    self.platform.idle(Some(deadline));
                    true
                }
                None if self.console_reader.is_some() => {
                    self.platform.idle(None);
                    true
                }
                None => !self.exits.is_empty(),
            };
        };
        let Some(mut p) = self.procs.take(pid) else { return true };
        if p.state != State::Runnable {
            self.procs.put(p);
            return true;
        }
        p.budget = TIME_SLICE;
        let before = p.reductions;
        let mut stop = interp::run(self, &mut p);
        self.stats.reductions += p.reductions - before;
        if let Some(profile) = &mut self.profile {
            *profile.entry(crate::interp::where_is(&p, 3)).or_default() += 1;
        }
        self.stats.context_switches += 1;
        if matches!(stop, Stop::Yield | Stop::Wait) && self.over_memory(&mut p) {
            stop = Stop::Exit(Err(Exception::exit(Term::Atom(self.atoms.killed.clone()))));
        }
        match stop {
            Stop::Yield => {
                self.procs.put(p);
                self.run_queue.push_back(pid);
            }
            Stop::Wait => {
                p.state = State::Waiting;
                // A message may have arrived while it was running (e.g. sent to itself).
                if p.save < p.mailbox.len() || p.timed_out {
                    p.state = State::Runnable;
                    self.run_queue.push_back(pid);
                }
                self.procs.put(p);
            }
            Stop::Exit(result) => self.terminate(p, result),
        }
        true
    }

    /// Measure `p` if it has run long enough since the last measurement, and say whether it
    /// must be killed for holding too much memory.
    ///
    /// Measuring costs time in proportion to what the process holds, so it is done after the
    /// process has used as many reductions as half its last size in words: the cost stays a
    /// constant share of the process's own work, as a copying collector's does in BEAM. Between
    /// measurements a process can outgrow its limit, by a bounded factor for ordinary code.
    fn over_memory(&mut self, p: &mut Process) -> bool {
        let interval = (p.usage.words / 2).max(TIME_SLICE as u64);
        if p.reductions - p.measured_at < interval {
            return false;
        }
        let vm_limit = self.limits.max_heap_words;
        let own = p.max_heap;
        let budget = if own.size > 0 { own.size.min(vm_limit) } else { vm_limit };
        let usage = crate::memory::process(p, budget);
        p.usage = usage;
        p.measured_at = p.reductions;
        if usage.total_words() > vm_limit {
            return true;
        }
        let used = if own.include_shared_binaries { usage.total_words() } else { usage.words };
        own.size > 0 && used > own.size && own.kill
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
        let Some(reader) = self.console_reader else { return };
        let msg = match self.platform.console_read() {
            ConsoleInput::Nothing => return,
            ConsoleInput::Data(bytes) => Term::binary(&bytes),
            ConsoleInput::Eof => {
                self.console_reader = None;
                Term::Atom(self.atom("eof"))
            }
        };
        let tag = Term::Atom(self.atom("beamlet_console"));
        self.send(reader, Term::tuple(alloc::vec![tag, msg]));
    }

    /// Close the files `pid` opened.
    fn close_files(&mut self, pid: Pid) {
        let handles: Vec<u64> = self.files.iter().filter(|(_, &o)| o == pid).map(|(&h, _)| h).collect();
        for h in handles {
            self.files.remove(&h);
            if let Some(f) = self.platform.files() {
                f.close(h);
            }
        }
    }

    fn terminate(&mut self, p: Box<Process>, result: Result<Term, Exception>) {
        let pid = p.pid;
        let reason = match &result {
            Ok(_) => Term::Atom(self.atoms.normal.clone()),
            Err(e) => match e.class {
                Class::Exit => e.reason.clone(),
                Class::Error => Term::tuple(alloc::vec![e.reason.clone(), e.trace.clone().unwrap_or(Term::Nil)]),
                Class::Throw => Term::tuple(alloc::vec![
                    Term::tuple(alloc::vec![Term::Atom(self.atoms.nocatch.clone()), e.reason.clone()]),
                    e.trace.clone().unwrap_or(Term::Nil),
                ]),
            },
        };
        if let Some(t) = p.timer {
            self.cancel_timer(pid, t);
        }
        self.aliases.retain(|_, a| a.owner != pid);
        self.close_files(pid);
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
            self.exits.push_back(ExitSignal { target: other, from: pid, reason: reason.clone(), from_link: true, forced: false });
        }
        for (r, (watcher, object)) in &p.monitored_by {
            let msg = Term::tuple(alloc::vec![
                Term::Atom(self.atoms.down.clone()),
                Term::Ref(*r),
                Term::Atom(self.atoms.process.clone()),
                object.clone(),
                reason.clone(),
            ]);
            if let Some(w) = self.procs.get_mut(*watcher) {
                w.monitors.remove(r);
            }
            // A monitor's alias ends when the monitor fires.
            if self.aliases.get(r).is_some_and(|a| a.mode != AliasMode::Explicit) {
                self.aliases.remove(r);
            }
            self.send(*watcher, msg);
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
                    let id = if t.named { Term::Atom(t.name.clone()) } else { Term::Ref(Ref(t.tid)) };
                    let tag = Term::Atom(self.atom("ETS-TRANSFER"));
                    self.send(to, Term::tuple(alloc::vec![tag, id, Term::Pid(pid), data]));
                }
                _ => {
                    self.ets.delete(tid);
                }
            }
        }
        if self.watched.contains(&pid) {
            self.results.insert(pid, result);
        }
        drop(p);
        self.procs.release(pid);
    }

    /// Deliver queued exit signals. A signal either becomes an `{'EXIT', From, Reason}` message
    /// (the target traps exits), is ignored (reason `normal`), or kills the target.
    fn deliver_exits(&mut self) {
        while let Some(ExitSignal { target, from, reason, from_link, forced }) = self.exits.pop_front() {
            if forced {
                if let Some(p) = self.procs.take(target) {
                    self.terminate(p, Err(Exception::exit(reason)));
                }
                continue;
            }
            let kill = !from_link && reason.is_atom(&self.atoms.kill);
            let normal = reason.is_atom(&self.atoms.normal);
            let Some(p) = self.procs.get_mut(target) else { continue };
            if p.trap_exit && !kill {
                let msg = Term::tuple(alloc::vec![Term::Atom(self.atoms.exit_upper.clone()), Term::Pid(from), reason]);
                if !deliver(p, msg, &mut self.run_queue, self.limits.max_mailbox) {
                    let reason = mailbox_full(&mut self.atom_table, &self.atoms);
                    self.exits.push_back(ExitSignal { target, from: target, reason, from_link: false, forced: true });
                }
            } else if !normal || from == target {
                let reason = if kill { Term::Atom(self.atoms.killed.clone()) } else { reason };
                let p = self.procs.take(target).expect("present");
                // Remove it from the run queue lazily: `step` skips pids that are gone.
                self.terminate(p, Err(Exception::exit(reason)));
            }
        }
    }
}

/// The exit reason of a process whose mailbox overflowed.
pub(crate) fn mailbox_full(table: &mut AtomTable, atoms: &Atoms) -> Term {
    let queue = Term::Atom(table.intern("message_queue").expect("short atom"));
    Term::tuple(alloc::vec![Term::Atom(atoms.system_limit.clone()), queue])
}

/// Queue a message. `false` if the mailbox is full: the caller must then end the receiver.
pub(crate) fn deliver(p: &mut Process, msg: Term, run_queue: &mut VecDeque<Pid>, max_mailbox: usize) -> bool {
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
