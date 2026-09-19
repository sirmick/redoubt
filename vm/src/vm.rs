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
use crate::platform::Platform;
use crate::process::{Class, Cp, Exception, Process, State};
use crate::term::{Pid, Ref, Term};

/// Reductions (calls) a process may run before it is preempted.
pub const TIME_SLICE: usize = 2000;
/// Most processes alive at once. Spawning more raises `system_limit`.
pub const MAX_PROCESSES: usize = 1 << 16;
/// Most messages queued for one process. Sends beyond it are dropped (see DESIGN.md).
pub const MAX_MAILBOX: usize = 1 << 16;

/// Resource limits for one VM, set by the embedder. Exceeding one raises `system_limit`.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Largest binary or bitstring any one operation may build, in bits.
    pub max_binary_bits: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_binary_bits: 1 << 30 } // 128 MiB
    }
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
}

/// Erlang modules every VM has, built from `vm/lib/*.erl` by `tools/build-lib`: the console
/// I/O server, and a small `logger` in place of the kernel application's.
const EMBEDDED: &[&[u8]] = &[
    include_bytes!("../lib/beamlet_io.beam"),
    include_bytes!("../lib/logger.beam"),
    include_bytes!("../lib/error_logger.beam"),
];

enum Slot {
    Free { serial: u32 },
    Present(Box<Process>),
    /// Taken out by the scheduler while it runs.
    Running { serial: u32 },
}

pub(crate) struct ProcTable {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
}

impl ProcTable {
    fn new() -> ProcTable {
        ProcTable { slots: Vec::new(), free: Vec::new(), live: 0 }
    }

    fn allocate(&mut self) -> Option<Pid> {
        if self.live >= MAX_PROCESSES {
            return None;
        }
        self.live += 1;
        if let Some(index) = self.free.pop() {
            let serial = match self.slots[index as usize] {
                Slot::Free { serial } => serial.wrapping_add(1),
                _ => unreachable!("free list holds free slots"),
            };
            self.slots[index as usize] = Slot::Running { serial };
            return Some(Pid { index, serial });
        }
        let index = self.slots.len() as u32;
        self.slots.push(Slot::Running { serial: 0 });
        Some(Pid { index, serial: 0 })
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
                Slot::Free { .. } => None,
            })
            .collect()
    }

    fn release(&mut self, pid: Pid) {
        self.slots[pid.index as usize] = Slot::Free { serial: pid.serial };
        self.free.push(pid.index);
        self.live -= 1;
    }

}

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
}

/// Why [`Vm::run`] returned.
#[derive(Debug)]
pub enum RunError {
    /// The watched process is still alive but nothing can run and no timer is pending.
    Deadlock,
}

pub struct Vm {
    pub sys: System,
}

impl Vm {
    pub fn new(platform: Box<dyn Platform>) -> Vm {
        Vm::with_limits(platform, Limits::default())
    }

    pub fn with_limits(platform: Box<dyn Platform>, limits: Limits) -> Vm {
        let mut atom_table = AtomTable::new();
        let atoms = Atoms::new(&mut atom_table);
        let natives = bif::Registry::new();
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
            },
        }
        .boot()
    }

    /// Start the console I/O servers, `user` and `standard_error`.
    fn boot(mut self) -> Vm {
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
            if let Some(r) = self.sys.results.remove(&pid) {
                self.sys.watched.remove(&pid);
                return Ok(r);
            }
            if !self.sys.step() {
                return Err(RunError::Deadlock);
            }
        }
    }

    /// Like [`Vm::run`], but give up after `max_steps` scheduling steps and return `None`.
    /// For tests that run untrusted code which may legitimately loop forever.
    pub fn run_bounded(&mut self, pid: Pid, max_steps: usize) -> Option<Result<Result<Term, Exception>, RunError>> {
        self.sys.watched.insert(pid);
        for _ in 0..max_steps {
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

    fn load(&mut self, bytes: &[u8]) -> Result<Atom, LoadError> {
        let module = loader::load(bytes, &mut self.atom_table)?;
        let name = module.name.clone();
        self.modules.insert(name.as_str().to_string(), Rc::new(module));
        Ok(name)
    }

    /// The module named `name`, loading it through the platform on first use.
    pub fn module(&mut self, name: &Atom) -> Option<Rc<Module>> {
        if let Some(m) = self.modules.get(name.as_str()) {
            return Some(m.clone());
        }
        let bytes = self.platform.load_module(name.as_str())?;
        let loaded = self.load(&bytes).ok()?;
        if &loaded != name {
            // A file that claims to be a different module than the one asked for.
            self.modules.remove(loaded.as_str());
            return None;
        }
        self.modules.get(name.as_str()).cloned()
    }

    pub fn is_loaded(&self, name: &Atom) -> bool {
        self.modules.contains_key(name.as_str())
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
        if let Some(n) = self.native(module, function, arity) {
            return Some(Target::Native(n));
        }
        let m = self.module(module)?;
        let entry = m.export(function, arity)?;
        Some(Target::Code(Cp { module: m, pc: entry }))
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
            deliver(p, msg, &mut self.run_queue);
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
        let Some(pid) = self.run_queue.pop_front() else {
            // Nothing runnable: sleep until the next timer, or give up if there is none.
            return match self.timers.first() {
                Some(&(deadline, _)) => {
                    self.platform.idle(Some(deadline));
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
        let stop = interp::run(self, &mut p);
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
        if let Some(name) = &p.registered_name {
            self.registered.remove(name.as_str());
        }
        for &other in &p.links {
            if let Some(o) = self.procs.get_mut(other) {
                o.links.remove(&pid);
            }
            self.exits.push_back(ExitSignal { target: other, from: pid, reason: reason.clone(), from_link: true });
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
        while let Some(ExitSignal { target, from, reason, from_link }) = self.exits.pop_front() {
            let kill = !from_link && reason.is_atom(&self.atoms.kill);
            let normal = reason.is_atom(&self.atoms.normal);
            let Some(p) = self.procs.get_mut(target) else { continue };
            if p.trap_exit && !kill {
                let msg = Term::tuple(alloc::vec![Term::Atom(self.atoms.exit_upper.clone()), Term::Pid(from), reason]);
                deliver(p, msg, &mut self.run_queue);
            } else if !normal || from == target {
                let reason = if kill { Term::Atom(self.atoms.killed.clone()) } else { reason };
                let p = self.procs.take(target).expect("present");
                // Remove it from the run queue lazily: `step` skips pids that are gone.
                self.terminate(p, Err(Exception::exit(reason)));
            }
        }
    }
}

pub(crate) fn deliver(p: &mut Process, msg: Term, run_queue: &mut VecDeque<Pid>) {
    if p.mailbox.len() >= MAX_MAILBOX {
        return;
    }
    p.mailbox.push_back(msg);
    if p.state == State::Waiting {
        p.state = State::Runnable;
        run_queue.push_back(p.pid);
    }
}

/// What a call resolves to.
pub enum Target {
    Native(Native),
    Code(Cp),
}
