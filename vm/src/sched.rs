//! A scheduler: what running a process's instructions needs, without holding the lock on the
//! VM's shared state ([`System`]) except for the instructions that touch it.
//!
//! Most instructions only read and write the running process. The few VM-wide things they read
//! on every call are copied here (the atoms the VM names, the limits) or cached here (literal
//! chunks, resolved calls), checked against [`Generations`] counters that the system bumps when
//! code or literals change. Natives, timers and anything else take the lock for as long as they
//! run. With one scheduler the lock is a `RefCell` and costs next to nothing.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::atom::{Atom, Atoms};
use crate::module::Module;
use crate::process::Process;
use crate::sync::{Guard, Lock};
use crate::term::Pid;
use crate::term::{Literals, Ref};
use crate::vm::{Limits, System, Target};

/// What finding code needs: a scheduler asks through its caches, natives ask the system.
pub trait Code {
    fn atoms(&self) -> &Atoms;
    fn module(&mut self, name: &Atom) -> Option<Arc<Module>>;
    fn resolve(&mut self, module: &Atom, function: &Atom, arity: u32) -> Option<Target>;
}

impl Code for System {
    fn atoms(&self) -> &Atoms {
        &self.atoms
    }
    fn module(&mut self, name: &Atom) -> Option<Arc<Module>> {
        System::module(self, name)
    }
    fn resolve(&mut self, module: &Atom, function: &Atom, arity: u32) -> Option<Target> {
        System::resolve(self, module, function, arity)
    }
}

impl Code for Sched<'_> {
    fn atoms(&self) -> &Atoms {
        &self.atoms
    }
    fn module(&mut self, name: &Atom) -> Option<Arc<Module>> {
        Sched::module(self, name)
    }
    fn resolve(&mut self, module: &Atom, function: &Atom, arity: u32) -> Option<Target> {
        Sched::resolve(self, module, function, arity)
    }
}

/// Counters a scheduler checks its caches against, readable without the system lock.
#[derive(Default)]
pub struct Generations {
    /// Bumped whenever a module is loaded or deleted: resolved calls may have changed.
    pub(crate) code: AtomicUsize,
    /// The number of literal chunks.
    pub(crate) literals: AtomicUsize,
}

pub struct Sched<'v> {
    sys: &'v Lock<System>,
    generations: Arc<Generations>,
    pub atoms: Atoms,
    pub limits: Limits,
    literals: Literals,
    /// What calls resolved to, as of code generation `resolved_at`.
    resolved: BTreeMap<(usize, usize, u32), Target>,
    resolved_at: usize,
}

impl<'v> Sched<'v> {
    pub fn new(sys: &'v Lock<System>) -> Sched<'v> {
        let s = sys.lock();
        Sched {
            sys,
            generations: s.generations.clone(),
            atoms: s.atoms.clone(),
            limits: s.limits,
            literals: s.literals.clone(),
            resolved: BTreeMap::new(),
            resolved_at: s.generations.code.load(Ordering::Acquire),
        }
    }

    /// Exclusive access to the system, until the guard is dropped.
    pub fn lock(&self) -> Guard<'v, System> {
        self.sys.lock()
    }

    /// The literal chunks, brought up to date if code or persistent terms added some.
    pub fn literals(&mut self) -> &Literals {
        if self.generations.literals.load(Ordering::Acquire) != self.literals.chunks() {
            self.literals = self.sys.lock().literals.clone();
        }
        &self.literals
    }

    /// Bring `p`'s heap up to date with the literal chunks.
    pub fn refresh(&mut self, p: &mut Process) {
        let lits = self.literals();
        p.refresh(lits);
    }

    /// `module:function/arity`, from this scheduler's cache when code has not changed since.
    pub fn resolve(&mut self, module: &Atom, function: &Atom, arity: u32) -> Option<Target> {
        let now = self.generations.code.load(Ordering::Acquire);
        if now != self.resolved_at {
            self.resolved.clear();
            self.resolved_at = now;
        }
        let key = (module.id(), function.id(), arity);
        if let Some(t) = self.resolved.get(&key) {
            return Some(t.clone());
        }
        let t = self.sys.lock().resolve(module, function, arity)?;
        self.resolved.insert(key, t.clone());
        Some(t)
    }

    pub fn module(&mut self, name: &Atom) -> Option<Arc<Module>> {
        self.sys.lock().module(name)
    }

    pub fn atom(&mut self, name: &str) -> Atom {
        self.sys.lock().atom(name)
    }

    pub fn backtrace_depth(&self) -> usize {
        self.sys.lock().backtrace_depth
    }

    pub fn now_us(&mut self) -> u64 {
        self.sys.lock().now_us()
    }

    pub fn make_ref(&mut self) -> Ref {
        self.sys.lock().make_ref()
    }

    pub fn arm_timer(&mut self, pid: Pid, deadline: u64) {
        self.sys.lock().arm_timer(pid, deadline)
    }

    pub fn cancel_timer(&mut self, pid: Pid, deadline: u64) {
        self.sys.lock().cancel_timer(pid, deadline)
    }
}
