//! Random operation sequences for the property tests, from one `u64` seed.
//!
//! The generator looks at the model's state to choose plausible arguments (a handle the actor
//! holds, a page it has mapped, a message it serves), because uniformly random arguments almost
//! always fail the first check and test nothing else. About one argument in ten is hostile
//! instead (a handle index the actor does not have, an unaligned address, a list over its cap).
//!
//! Each sequence starts with a short setup, adapted to the random choices, that builds a world
//! worth attacking: principals' budgets under `users` (some labelled, with accounts), a system
//! budget, shared endpoints, handles minted into the principals' budgets, and a process in each
//! budget. Then random operations follow; now and then a "flood" phase piles blocked senders of
//! two accounts onto one endpoint (R2).

use alloc::vec;
use alloc::vec::Vec;

use crate::kernel::{
    Backing, DeviceKind, INIT_PID, KERNEL_CHOSEN_BASE, Kernel, MapState, Object, ROOT, SYSTEM, USER_TOP,
    USERS,
};
use crate::spec::*;
use crate::syscall::*;

/// A handle to a DMA-flagged device (WP-K5b, answer 173).
fn dma_device(k: &Kernel, h: &crate::kernel::Handle) -> bool {
    matches!(h.object, Object::Device(d)
        if k.devices.get(&d).is_some_and(|dev| matches!(dev.kind, DeviceKind::Mmio { dma: true, .. })))
}

/// splitmix64: small, fast, and good enough to drive a search; not for secrets.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng { Rng(seed ^ 0x5eed_0fd0_0b70) }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: u64) -> u64 { if n == 0 { 0 } else { self.next_u64() % n } }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 { lo + self.below(hi - lo + 1) }

    /// True with probability `percent`/100.
    pub fn pct(&mut self, percent: u64) -> bool { self.below(100) < percent }

    pub fn pick<T: Copy>(&mut self, v: &[T]) -> Option<T> {
        if v.is_empty() { None } else { Some(v[self.below(v.len() as u64) as usize]) }
    }
}

/// Labels and accounts the generator draws from, so that equal and unequal sets both occur.
pub const LABEL_POOL: [u64; 3] = [7, 8, 9];
pub const ACCOUNT_POOL: [u64; 4] = [0, 1001, 1002, 1003];

#[derive(Clone, Debug)]
pub struct Gen {
    pub rng: Rng,
    setup: u32,
    principals: u64,
    /// Budgets the setup already tried to start a process in.
    tried: Vec<u64>,
    flood: u32,
    flood_endpoint: Option<u64>,
    /// During a flood, receivers take calls and never reply (open calls pile up, R4a).
    hoard: bool,
    /// A thread bomb in progress: (process, steps left).
    bomb: Option<(u64, u32)>,
    /// The relay after the setup (see `relay_op`): steps left, and the user process and the
    /// endpoint it created, once chosen.
    relay: u32,
    relay_from: Option<(u64, u64)>,
    relay_sent: bool,
}

impl Gen {
    pub fn new(seed: u64) -> Gen {
        let mut rng = Rng::new(seed);
        let principals = rng.range(2, 3);
        // One sequence in five skips the setup and starts from bare `init`.
        let setup = if rng.pct(20) { u32::MAX } else { 0 };
        let relay = if setup == 0 && rng.pct(70) { 30 } else { 0 };
        Gen {
            rng,
            setup,
            principals,
            tried: Vec::new(),
            flood: 0,
            flood_endpoint: None,
            hoard: false,
            bomb: None,
            relay,
            relay_from: None,
            relay_sent: false,
        }
    }

    /// The next op for this state. Always one the model accepts as a legal event (`step` returns
    /// `Some`), unless the machine is halted.
    pub fn next_op(&mut self, k: &Kernel) -> Op {
        if self.setup != u32::MAX {
            if let Some(op) = self.setup_op(k) {
                return op;
            }
            self.setup = u32::MAX;
        }
        while self.relay > 0 {
            self.relay -= 1;
            if let Some(op) = self.relay_op(k) {
                return op;
            }
        }
        if self.rng.pct(3) && k.threads.values().any(|t| t.call_lend.is_some()) {
            let calls: Vec<_> = k.threads.values().filter(|t| t.call_lend.is_some()).collect();
            let t = calls[self.rng.below(calls.len() as u64) as usize];
            let record = match self.rng.below(8) {
                0 => crate::syscall::Record::Unmapped,
                1 => crate::syscall::Record::ReadOnly,
                2 => crate::syscall::Record::Borrowed,
                3 => crate::syscall::Record::Device,
                4 => crate::syscall::Record::CopyFault,
                _ => crate::syscall::Record::Owned,
            };
            return Op::Record { pid: t.pid, tid: t.tid, record };
        }
        if let Some(t) =
            k.threads.values().find(|t| t.wait.is_none() && t.record == crate::syscall::Record::CopyFault)
        {
            return Op::Record { pid: t.pid, tid: t.tid, record: crate::syscall::Record::Owned };
        }
        let runnable = k.runnable();
        if runnable.is_empty() || self.rng.pct(6) {
            return self.event(k);
        }
        self.actor_op(k, &runnable)
    }

    /// A thread event selected directly from this budget's runnable subtree. No global setup,
    /// relay, clock or IRQ event is sampled and discarded. In particular, an outside thread's
    /// pending record repair cannot prevent an inside thread from acting.
    pub(crate) fn next_op_in(&mut self, k: &Kernel, budget: u64) -> Option<Op> {
        let runnable: Vec<_> = k
            .runnable()
            .into_iter()
            .filter(|(pid, _)| k.budget_of(*pid).is_some_and(|b| k.is_descendant_or_self(b, budget)))
            .collect();
        if runnable.is_empty() {
            return None;
        }
        if let Some(&(pid, tid)) = runnable.iter().find(|(_, tid)| k.threads[tid].record == Record::CopyFault)
        {
            return Some(Op::Record { pid, tid, record: Record::Owned });
        }
        Some(self.actor_op(k, &runnable))
    }

    /// Generate a runnable thread's event; all actor selection uses the supplied domain.
    fn actor_op(&mut self, k: &Kernel, runnable: &[(u64, u64)]) -> Op {
        // A thread bomb: one process creates threads until it hits MAX_THREADS or its page
        // limit (RESOURCES.md: a bomb hits its own limit; others keep going).
        if self.bomb.is_none() && self.rng.pct(1) {
            let (pid, _) = self.rng.pick(runnable).unwrap();
            self.bomb = Some((pid, 40));
        }
        if let Some((pid, left)) = self.bomb {
            self.bomb = if left > 1 { Some((pid, left - 1)) } else { None };
            if let Some((pid, tid)) = runnable.iter().copied().find(|(p, _)| *p == pid) {
                return Op::Sys { pid, tid, call: Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 } };
            }
            self.bomb = None;
        }
        if self.flood == 0 && self.rng.pct(2) {
            self.flood = self.rng.range(20, 60) as u32;
            self.flood_endpoint = None;
            self.hoard = self.rng.pct(30);
        }
        // Destroy something while messages are in flight to or from it (R3, R10).
        if self.rng.pct(3) {
            if let Some(op) = self.destroy_in_flight(k, runnable) {
                return op;
            }
        }
        if self.flood > 0 {
            self.flood -= 1;
            if let Some(op) = self.flood_op(k, runnable) {
                return op;
            }
        }
        let (pid, tid) = self.rng.pick(runnable).unwrap();
        let r = self.rng.below(100);
        if r < 3 {
            return Op::Fault { pid, tid };
        }
        if r < 13 {
            return self.memory_op(k, pid, tid);
        }
        // A process with little memory maps some, so that lends and transfers happen.
        let pages = k.processes[&pid].space.values().filter(|m| m.state == MapState::Own).count();
        if pages < 4 && self.rng.pct(25) {
            let n = self.rng.range(1, 4);
            return Op::Sys {
                pid,
                tid,
                call: Syscall::MapAnon { len: n * PAGE_SIZE, flags: FLAG_R | FLAG_W },
            };
        }
        let call = if self.rng.pct(4) { self.hostile_syscall() } else { self.syscall(k, pid, tid) };
        Op::Sys { pid, tid, call }
    }

    /// A value for a hostile argument: small, near a boundary, or anything.
    fn any(&mut self) -> u64 {
        match self.rng.below(8) {
            0 => 0,
            1 => self.rng.below(16),
            2 => U32_MAX - 1 + self.rng.below(3),
            3 => u64::MAX - self.rng.below(2),
            4 => self.rng.below(8) * PAGE_SIZE,
            5 => (1 << 38) - PAGE_SIZE,
            6 => 1 << self.rng.below(64),
            _ => self.rng.next_u64(),
        }
    }

    fn any_list(&mut self) -> Vec<u64> {
        let n = self.rng.below(10);
        (0..n).map(|_| self.any()).collect()
    }

    /// Any call with arguments that ignore the state entirely (I14: nothing can panic the
    /// kernel).
    pub fn hostile_syscall(&mut self) -> Syscall {
        use Syscall as S;
        let w = [self.any(), self.any(), self.any(), self.any()];
        let buf =
            |g: &mut Gen| if g.rng.pct(50) { None } else { Some(Buffer { addr: g.any(), npages: g.any() }) };
        match self.rng.below(25) {
            0 => S::MapAnon { len: self.any(), flags: self.any() },
            1 => S::Unmap { addr: self.any(), len: self.any() },
            2 => S::SetFlags { addr: self.any(), len: self.any(), flags: self.any() },
            3 => S::MapDevice { h: self.any() },
            4 => S::DmaAlloc { h: self.any(), npages: self.any() },
            5 => S::ThreadCreate { entry: self.any(), sp: self.any(), arg: self.any() },
            6 => S::ProcessExit { code: self.any() },
            7 => S::ProcessCreate { budget: self.any(), exit_endpoint: self.any() },
            8 => S::ProcessMap {
                process: self.any(),
                src: self.any(),
                dst: self.any(),
                len: self.any(),
                flags: self.any(),
            },
            9 => S::ProcessStart {
                process: self.any(),
                entry: self.any(),
                sp: self.any(),
                arg: self.any(),
                handles: self.any_list(),
            },
            10 => S::EndpointCreate,
            11 => {
                let source = if self.rng.pct(50) {
                    MintSource::Message(self.any())
                } else {
                    MintSource::Handle(self.any())
                };
                let budget = if self.rng.pct(50) { None } else { Some(self.any()) };
                S::Mint { source, badge: self.any(), budget }
            }
            12 => S::Call {
                h: self.any(),
                words: w,
                handles: self.any_list(),
                lend: buf(self),
                timeout: self.any(),
            },
            13 => S::Send {
                h: self.any(),
                words: w,
                handles: self.any_list(),
                transfer: buf(self),
                timeout: self.any(),
            },
            14 => S::Receive {
                h: if self.rng.pct(20) { None } else { Some(self.any()) },
                timeout: self.any(),
                max_transfer: self.any(),
            },
            15 => {
                if self.rng.pct(50) {
                    S::Reply { msg_id: self.any(), words: w, handles: self.any_list() }
                } else {
                    S::Serve { msg_id: self.any() }
                }
            }
            16 => S::HandleClose { h: self.any() },
            17 => S::BudgetCreate {
                parent: self.any(),
                pages: self.any(),
                processes: self.any(),
                weight: self.any(),

                labels: self.any_list(),
                account: self.any(),
                deadline: self.any(),
            },
            18 => S::BudgetDestroy { h: self.any() },
            19 => S::BudgetUsage { h: self.any() },
            20 => S::TimeNow,
            21 => S::SystemReset { h: self.any(), kind: self.any() },
            22 => S::Random,
            23 => S::MapFixed { addr: self.any(), len: self.any(), flags: self.any() },
            _ => S::ThreadCreate { entry: 0, sp: 0, arg: 0 },
        }
    }

    fn event(&mut self, k: &Kernel) -> Op {
        if self.rng.pct(25) {
            let n = if self.rng.pct(90) { self.rng.range(10, 11) } else { self.rng.below(64) };
            return Op::Irq { n };
        }
        let dt = match self.rng.below(4) {
            0 => 1,
            1 => self.rng.range(1, SLICE),
            2 => self.rng.range(SLICE, 5 * SLICE),
            _ => k.next_event().map_or(SLICE, |e| e.saturating_sub(k.now).clamp(1, 100 * SLICE)),
        };
        Op::Tick { dt }
    }

    // -------------------------------------------------------------------------------------------
    // Setup: a world with principals, a system server, shared endpoints and minted handles.

    fn setup_op(&mut self, k: &Kernel) -> Option<Op> {
        let init = k.processes.get(&INIT_PID)?;
        let tid = *init.threads.iter().find(|t| k.threads[t].wait.is_none())?;
        let sys = |call| Some(Op::Sys { pid: INIT_PID, tid, call });
        let handles_of = |f: &dyn Fn(Object) -> bool| -> Vec<u64> {
            init.handles.iter().filter(|(_, h)| f(h.object) && h.badge == 0).map(|(i, _)| *i).collect()
        };
        let endpoints = handles_of(&|o| matches!(o, Object::Endpoint(_)));
        let budget_h =
            |b: u64| init.handles.iter().find(|(_, h)| h.object == Object::Budget(b)).map(|(i, _)| *i);
        let step = self.setup;
        self.setup += 1;
        // Two shared endpoints.
        if endpoints.len() < 2 && step < 4 {
            return sys(Syscall::EndpointCreate);
        }
        let made: Vec<u64> = k
            .budgets
            .keys()
            .copied()
            .filter(|b| *b != ROOT && *b != SYSTEM && *b != USERS && budget_h(*b).is_some())
            .collect();
        // Principals under `users`, one budget under `system`, one nested budget.
        if made.len() < self.principals as usize + 2 && step < 16 {
            let i = made.len() as u64;
            let (parent, class, account, processes): (u64, Class, u64, u64) = if i < self.principals {
                (USERS, Class::User, 1001 + i, 3)
            } else if i == self.principals {
                (SYSTEM, Class::System, 0, 2)
            } else {
                // An agent-like budget under the first principal: its labels, maybe one more.
                (*made.first().unwrap_or(&USERS), Class::User, self.rng.pick(&ACCOUNT_POOL).unwrap(), 1)
            };
            // Labelled budgets of both classes, so that the label rules are judged against
            // system-class targets too.
            let mut labels = k.budgets[&parent].labels.clone();
            if self.rng.pct(if class == Class::User { 40 } else { 50 }) {
                labels.push(self.rng.pick(&LABEL_POOL).unwrap());
            }
            let free = k.budgets[&parent].pages_limit.saturating_sub(k.budgets[&parent].pages_used);
            return sys(Syscall::BudgetCreate {
                parent: budget_h(parent)?,
                pages: (free / 3).clamp(8, 120),
                processes,
                weight: self.rng.range(20, 100),

                labels,
                account,
                deadline: if self.rng.pct(15) { k.now + self.rng.range(SLICE, 40 * SLICE) } else { FOREVER },
            });
        }
        // A process in each budget init made, created and then started.
        for b in &made {
            if !self.tried.contains(b) && step < 40 {
                self.tried.push(*b);
                let e = self.rng.pick(&endpoints)?;
                return sys(Syscall::ProcessCreate { budget: budget_h(*b)?, exit_endpoint: e });
            }
        }
        let unstarted: Vec<(u64, u64)> = init
            .handles
            .iter()
            .filter_map(|(i, h)| match h.object {
                Object::Process(p) if !k.processes[&p].started => Some((*i, p)),
                _ => None,
            })
            .collect();
        if let Some((ph, p)) = unstarted.first().copied() {
            if step >= 60 {
                return None;
            }
            let b = k.processes[&p].budget;
            // Hand over: its own budget, a badge-0 endpoint (a receive right) or a handle minted
            // into its budget, and sometimes the other endpoint.
            let minted: Vec<u64> =
                init.handles.iter().filter(|(_, h)| h.badge != 0 && h.stamp == b).map(|(i, _)| *i).collect();
            if minted.is_empty() && self.rng.pct(70) {
                let e = self.rng.pick(&endpoints)?;
                return sys(Syscall::Mint {
                    source: MintSource::Handle(e),
                    badge: self.rng.range(1, 9),
                    budget: budget_h(b),
                });
            }
            let mut hs = vec![budget_h(b)?];
            if self.rng.pct(50) || minted.is_empty() {
                hs.push(self.rng.pick(&endpoints)?);
            }
            hs.extend(minted);
            if self.rng.pct(40) {
                hs.push(self.rng.pick(&endpoints)?);
            }
            // Now and then a user process that holds a system-class budget: `system` (QUESTIONS 9
            // says it still cannot make system-class children), or the one init made, which may be
            // labelled (`budget_usage` must refuse it then).
            if k.budgets[&b].class == Class::User && self.rng.pct(30) {
                let made_system =
                    made.iter().copied().find(|x| *x != SYSTEM && k.budgets[x].class == Class::System);
                let target = if self.rng.pct(50) { made_system.unwrap_or(SYSTEM) } else { SYSTEM };
                hs.push(budget_h(target)?);
            }
            // WP-K5b: now and then DMA devices too, each on its own, so a child can hold DMA
            // memory, share a device with init or a sibling (the co-holder case), allocate through
            // one device while mapping another, and quarantine a device by dying.
            for (i, h) in &init.handles {
                if dma_device(k, h) && self.rng.pct(50) {
                    hs.push(*i);
                }
            }
            return sys(Syscall::ProcessStart {
                process: ph,
                entry: 0x1000,
                sp: 0x2000,
                arg: 0x1000_0000,
                handles: hs,
            });
        }
        None
    }

    /// The relay: a user process creates an endpoint and sends it to `init`, which starts
    /// processes in the other budgets it made (labelled ones among them) holding a receive right
    /// to it and naming it as their exit endpoint. So endpoints owned by user budgets are used
    /// across label sets (R1 against the owner, QUESTIONS 4; exit notices to a user owner) and
    /// received outside their owner's budget (R10: calls in flight when the owner is destroyed).
    fn relay_op(&mut self, k: &Kernel) -> Option<Op> {
        let init = k.processes.get(&INIT_PID)?;
        let itid = *init.threads.iter().find(|t| k.threads[t].wait.is_none())?;
        let sys = |pid, tid, call| Some(Op::Sys { pid, tid, call });
        // Choose the user process and let it create its endpoint.
        let (p0, x) = match self.relay_from {
            Some(r) if k.endpoints.contains_key(&r.1) && k.processes.contains_key(&r.0) => r,
            Some(_) => return None,
            None => {
                let (pid, tid) = k.runnable().into_iter().find(|(pid, _)| {
                    *pid != INIT_PID
                        && k.budget_of(*pid).is_some_and(|b| k.budgets[&b].class == Class::User)
                        && k.processes[pid]
                            .handles
                            .values()
                            .any(|h| matches!(h.object, Object::Endpoint(e) if k.endpoints[&e].owner == ROOT))
                })?;
                let b = k.budget_of(pid)?;
                if let Some(e) = k.endpoints.values().find(|e| e.owner == b) {
                    self.relay_from = Some((pid, e.id));
                    return self.relay_op(k);
                }
                return sys(pid, tid, Syscall::EndpointCreate);
            }
        };
        let hx = |pid: u64, badge0: bool| {
            k.processes
                .get(&pid)?
                .handles
                .iter()
                .find(|(_, h)| h.object == Object::Endpoint(x) && (!badge0 || h.badge == 0))
                .map(|(i, _)| *i)
        };
        // It sends its receive right to init, on an endpoint init owns.
        if !self.relay_sent {
            let tid = *k.processes[&p0].threads.iter().find(|t| k.threads[t].wait.is_none())?;
            let (via, _) = k.processes[&p0]
                .handles
                .iter()
                .find(|(_, h)| matches!(h.object, Object::Endpoint(e) if k.endpoints[&e].owner == ROOT))?;
            self.relay_sent = true;
            let call = Syscall::Send {
                h: *via,
                words: [7, 0, 0, 0],
                handles: vec![hx(p0, true)?],
                transfer: None,
                timeout: FOREVER,
            };
            return sys(p0, tid, call);
        }
        // Init receives it.
        let Some(hi) = hx(INIT_PID, true) else {
            let recv: Vec<u64> = init
                .handles
                .iter()
                .filter(|(_, h)| {
                    h.badge == 0 && matches!(h.object, Object::Endpoint(e) if k.endpoints[&e].owner == ROOT)
                })
                .map(|(i, _)| *i)
                .collect();
            let h = self.rng.pick(&recv)?;
            return sys(INIT_PID, itid, Syscall::Receive { h: Some(h), timeout: 0, max_transfer: 0 });
        };
        // Init starts a process holding it in another budget it made, created with it as the
        // exit endpoint; one budget at a time.
        let b0 = k.budget_of(p0)?;
        let budget_h =
            |b: u64| init.handles.iter().find(|(_, h)| h.object == Object::Budget(b)).map(|(i, _)| *i);
        for (i, h) in &init.handles {
            if let Object::Process(p) = h.object {
                if !k.processes[&p].started
                    && k.processes[&p].exit_endpoint.is_some_and(|e| e.object == Object::Endpoint(x))
                {
                    let b = k.processes[&p].budget;
                    return sys(
                        INIT_PID,
                        itid,
                        Syscall::ProcessStart {
                            process: *i,
                            entry: 0,
                            sp: 0,
                            arg: 0,
                            handles: vec![budget_h(b)?, hi],
                        },
                    );
                }
            }
        }
        let started: Vec<u64> = k
            .processes
            .values()
            .filter(|p| p.exit_endpoint.is_some_and(|e| e.object == Object::Endpoint(x)))
            .map(|p| p.budget)
            .collect();
        let target = k.budgets.values().find(|b| {
            b.id != b0
                && b.class == Class::User
                && b.weight > 0
                && b.processes_used < b.processes_limit
                && budget_h(b.id).is_some()
                && !started.contains(&b.id)
        })?;
        sys(INIT_PID, itid, Syscall::ProcessCreate { budget: budget_h(target.id)?, exit_endpoint: hi })
    }

    /// A thread holding a handle to the budget that owns the endpoint of a message in flight, or
    /// to the budget of its sender, destroys that budget (R10's "calls in flight fail"; R3's
    /// lender dying mid-call).
    fn destroy_in_flight(&mut self, k: &Kernel, runnable: &[(u64, u64)]) -> Option<Op> {
        let msgs: Vec<u64> = k.msgs.keys().copied().collect();
        let m = &k.msgs[&self.rng.pick(&msgs)?];
        let target = if self.rng.pct(50) { k.endpoints.get(&m.endpoint)?.owner } else { m.sender_budget };
        if target == ROOT {
            return None;
        }
        let holders: Vec<(u64, u64, u64)> = runnable
            .iter()
            .filter_map(|(pid, tid)| {
                let p = &k.processes[pid];
                let h =
                    p.handles.iter().find(|(_, h)| h.object == Object::Budget(target)).map(|(i, _)| *i)?;
                Some((*pid, *tid, h))
            })
            .collect();
        let (pid, tid, h) = self.rng.pick(&holders)?;
        Some(Op::Sys { pid, tid, call: Syscall::BudgetDestroy { h } })
    }

    // -------------------------------------------------------------------------------------------
    // Flood: many blocked senders from a process's threads onto one endpoint (R2).

    fn flood_op(&mut self, k: &Kernel, runnable: &[(u64, u64)]) -> Option<Op> {
        let target = match self.flood_endpoint {
            Some(e) if k.endpoints.contains_key(&e) => e,
            _ => {
                let eps: Vec<u64> = k.endpoints.keys().copied().collect();
                let e = self.rng.pick(&eps)?;
                self.flood_endpoint = Some(e);
                e
            }
        };
        let holders: Vec<(u64, u64)> = runnable
            .iter()
            .copied()
            .filter(|(pid, _)| {
                k.processes[pid].handles.values().any(|h| h.object == Object::Endpoint(target))
            })
            .collect();
        let (pid, tid) = self.rng.pick(&holders)?;
        let p = &k.processes[&pid];
        let h_any = p.handles.iter().find(|(_, h)| h.object == Object::Endpoint(target)).map(|(i, _)| *i)?;
        let h_recv = p
            .handles
            .iter()
            .find(|(_, h)| h.object == Object::Endpoint(target) && h.badge == 0)
            .map(|(i, _)| *i);
        if let Some(h) = h_recv {
            if self.rng.pct(if self.hoard { 45 } else { 20 }) {
                return Some(Op::Sys {
                    pid,
                    tid,
                    call: Syscall::Receive {
                        h: Some(h),
                        timeout: if self.hoard { FOREVER } else { 0 },
                        max_transfer: 1,
                    },
                });
            }
        }
        let call = k.threads[&tid].serving.first().and_then(|m| k.msgs.get(m)).map(|m| m.rid);
        if let Some(m) = call.filter(|_| !self.hoard) {
            return Some(Op::Sys {
                pid,
                tid,
                call: Syscall::Reply { msg_id: m, words: [0; WORDS], handles: vec![] },
            });
        }
        let runnable_here = runnable.iter().filter(|(p, _)| *p == pid).count();
        if runnable_here < 2 && (p.threads.len() as u64) < MAX_THREADS {
            return Some(Op::Sys { pid, tid, call: Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 } });
        }
        let timeout = if self.rng.pct(70) { FOREVER } else { self.rng.range(SLICE, 20 * SLICE) };
        let words = [self.rng.below(4), 0, 0, 0];
        let call = if self.rng.pct(50) {
            Syscall::Send { h: h_any, words, handles: vec![], transfer: None, timeout }
        } else {
            Syscall::Call { h: h_any, words, handles: vec![], lend: None, timeout }
        };
        Some(Op::Sys { pid, tid, call })
    }

    // -------------------------------------------------------------------------------------------
    // Random steps.

    /// A handle of the actor naming an object that satisfies `want`, or now and then a hostile
    /// index.
    fn handle(&mut self, k: &Kernel, pid: u64, want: impl Fn(&crate::kernel::Handle) -> bool) -> u64 {
        let p = &k.processes[&pid];
        let good: Vec<u64> = p.handles.iter().filter(|(_, h)| want(h)).map(|(i, _)| *i).collect();
        if good.is_empty() || self.rng.pct(8) {
            return match self.rng.below(5) {
                0 => NO_HANDLE,
                1 => U32_MAX + self.rng.below(3),
                2 => self.rng.range(1, 40),
                3 => U32_MAX - self.rng.below(2),
                _ => *p.handles.keys().next().unwrap_or(&1),
            };
        }
        self.rng.pick(&good).unwrap()
    }

    fn timeout(&mut self) -> u64 {
        match self.rng.below(10) {
            0..=2 => 0,
            3..=8 => self.rng.range(1, 4 * SLICE),
            _ => FOREVER,
        }
    }

    fn flags(&mut self) -> u64 {
        match self.rng.below(20) {
            0 => FLAG_W | FLAG_X | FLAG_R,
            1 => FLAG_W | FLAG_X,
            2 => 0,
            3 => FLAG_W,
            4 => 8,
            5..=7 => FLAG_R | FLAG_X,
            8 => FLAG_X,
            9 | 10 => FLAG_R,
            _ => FLAG_R | FLAG_W,
        }
    }

    /// A run of the actor's own accessible pages (`want` filters them), as (addr, pages); or
    /// garbage now and then.
    fn own_pages(&mut self, k: &Kernel, pid: u64, max: u64, want: impl Fn(u64) -> bool) -> (u64, u64) {
        let p = &k.processes[&pid];
        let pages: Vec<u64> = p
            .space
            .iter()
            .filter(|(_, m)| {
                m.state == MapState::Own && matches!(m.backing, Backing::Frame(_)) && want(m.flags)
            })
            .map(|(v, _)| *v)
            .collect();
        if pages.is_empty() || self.rng.pct(6) {
            return match self.rng.below(3) {
                0 => (0x1234, 1),
                1 => (KERNEL_CHOSEN_BASE_GUESS, self.rng.range(1, 3)),
                _ => (u64::MAX - PAGE_SIZE + 1, 2),
            };
        }
        let first = self.rng.pick(&pages).unwrap();
        let mut n = 1;
        while n < max
            && p.space.get(&(first + n)).is_some_and(|m| m.state == MapState::Own && want(m.flags))
            && self.rng.pct(60)
        {
            n += 1;
        }
        if self.rng.pct(4) {
            n += 1; // one page past the run
        }
        (first * PAGE_SIZE, n)
    }

    fn memory_op(&mut self, k: &Kernel, pid: u64, tid: u64) -> Op {
        let p = &k.processes[&pid];
        let mapped: Vec<u64> = p.space.keys().copied().collect();
        let addr = match self.rng.pick(&mapped) {
            Some(v) if self.rng.pct(92) => v * PAGE_SIZE + self.rng.below(PAGE_SIZE / 8) * 8,
            _ => self.rng.range(1, 1 << 40) & !7,
        };
        match self.rng.below(10) {
            0..=5 => Op::Write { pid, tid, addr, value: self.rng.range(1, 1 << 32) },
            6..=8 => Op::Read { pid, tid, addr },
            _ => Op::Exec { pid, tid, addr },
        }
    }

    fn syscall(&mut self, k: &Kernel, pid: u64, tid: u64) -> Syscall {
        let t = &k.threads[&tid];
        // The ids its process knows its open calls by.
        let open: Vec<u64> = t.serving.iter().filter_map(|m| k.msgs.get(m)).map(|m| m.rid).collect();
        // A thread holding calls usually answers one, or now and then turns to another (`serve`),
        // or answers an id that is not its open call (a stale or a send's id).
        if let Some(m) = self.rng.pick(&open) {
            let r = self.rng.below(100);
            if r < 40 {
                let handles = self.some_handles(k, pid, 2);
                return Syscall::Reply { msg_id: m, words: [self.rng.below(9); WORDS], handles };
            }
            if r < 50 {
                return Syscall::Serve { msg_id: m };
            }
        }
        if self.rng.pct(2) {
            let m = self.rng.range(0, 8);
            return if self.rng.pct(50) {
                Syscall::Reply { msg_id: m, words: [0; WORDS], handles: vec![] }
            } else {
                Syscall::Serve { msg_id: m }
            };
        }
        // WP-K5b: a child holding DMA devices maps them and allocates through them often, so that
        // co-holders (one maps a device another allocates through or maps too) and their deaths
        // come up in short sequences.
        if pid != INIT_PID && self.rng.pct(12) && k.processes[&pid].handles.values().any(|h| dma_device(k, h)) {
            let h = self.handle(k, pid, |h| dma_device(k, h));
            return if self.rng.pct(50) {
                Syscall::MapDevice { h }
            } else {
                Syscall::DmaAlloc { h, npages: self.rng.range(1, 3) }
            };
        }
        let is_endpoint = |h: &crate::kernel::Handle| matches!(h.object, Object::Endpoint(_));
        let is_budget = |h: &crate::kernel::Handle| matches!(h.object, Object::Budget(_));
        match self.rng.below(100) {
            0..=6 => {
                let n = if self.rng.pct(90) { self.rng.range(1, 4) } else { self.rng.range(0, 40) };
                let len = if self.rng.pct(95) { n * PAGE_SIZE } else { n * PAGE_SIZE + 12 };
                Syscall::MapAnon { len, flags: self.flags() }
            }
            7..=9 => {
                let (addr, n) = self.own_pages(k, pid, 3, |_| true);
                Syscall::Unmap { addr, len: n * PAGE_SIZE }
            }
            10..=11 => {
                let (addr, n) = self.own_pages(k, pid, 3, |_| true);
                Syscall::SetFlags { addr, len: n * PAGE_SIZE, flags: self.flags() }
            }
            12 => Syscall::MapDevice { h: self.handle(k, pid, |h| matches!(h.object, Object::Device(_))) },
            13 => Syscall::DmaAlloc {
                h: self.handle(k, pid, |h| matches!(h.object, Object::Device(_))),
                npages: self.rng.range(0, 3),
            },
            14..=17 => Syscall::ThreadCreate { entry: 0x1000, sp: 0x2000, arg: self.rng.below(9) },
            18 => {
                if self.rng.pct(50) {
                    Syscall::ThreadExit
                } else {
                    Syscall::TimeNow
                }
            }
            19 => {
                if pid != INIT_PID || self.rng.pct(5) {
                    Syscall::ProcessExit { code: if self.rng.pct(95) { self.rng.below(5) } else { 1 << 33 } }
                } else {
                    Syscall::TimeNow
                }
            }
            20..=23 => Syscall::ProcessCreate {
                budget: self.handle(k, pid, is_budget),
                exit_endpoint: self.handle(k, pid, is_endpoint),
            },
            24..=26 => {
                let process = self.handle(k, pid, |h| matches!(h.object, Object::Process(_)));
                let (src, n) = self.own_pages(k, pid, 2, |_| true);
                let dst = 0x1000_0000 + self.rng.below(4) * PAGE_SIZE;
                Syscall::ProcessMap { process, src, dst, len: n * PAGE_SIZE, flags: self.flags() }
            }
            27..=30 => {
                let process = self.handle(k, pid, |h| matches!(h.object, Object::Process(_)));
                let handles = self.some_handles(k, pid, 5);
                Syscall::ProcessStart {
                    process,
                    entry: 0x1000,
                    sp: 0x2000,
                    arg: self.rng.below(3) * PAGE_SIZE,
                    handles,
                }
            }
            31..=33 => Syscall::EndpointCreate,
            34..=38 => {
                // Now and then another thread's open call (mint takes only the caller's own).
                let others: Vec<u64> = k
                    .msgs
                    .values()
                    .filter(|m| m.server.is_some_and(|s| s.0 == pid && s.1 != tid))
                    .map(|m| m.rid)
                    .collect();
                let serving = if self.rng.pct(15) {
                    self.rng.pick(&others)
                } else {
                    self.rng.pick(&open).filter(|_| self.rng.pct(70))
                };
                let source = if let Some(m) = serving {
                    MintSource::Message(m)
                } else if self.rng.pct(5) {
                    MintSource::Message(self.rng.range(1, 50))
                } else {
                    MintSource::Handle(self.handle(k, pid, is_endpoint))
                };
                let badge = if self.rng.pct(92) { self.rng.range(1, 9) } else { 0 };
                let budget = if self.rng.pct(40) { Some(self.handle(k, pid, is_budget)) } else { None };
                Syscall::Mint { source, badge, budget }
            }
            39..=50 => {
                let h = self.handle(k, pid, is_endpoint);
                let max = if self.rng.pct(95) { 2 } else { 6 };
                let handles = self.some_handles(k, pid, max);
                let lend = if self.rng.pct(55) {
                    let (addr, n) = self.own_pages(k, pid, 3, |f| f & FLAG_W != 0);
                    Some(Buffer { addr, npages: if self.rng.pct(97) { n } else { MAX_LEND_PAGES + 1 } })
                } else {
                    None
                };
                Syscall::Call { h, words: [self.rng.below(9); WORDS], handles, lend, timeout: self.timeout() }
            }
            51..=59 => {
                let h = self.handle(k, pid, is_endpoint);
                let handles = self.some_handles(k, pid, 2);
                let transfer = if self.rng.pct(50) {
                    let (addr, npages) = self.own_pages(k, pid, 4, |_| true);
                    Some(Buffer { addr, npages })
                } else {
                    None
                };
                Syscall::Send {
                    h,
                    words: [self.rng.below(9); WORDS],
                    handles,
                    transfer,
                    timeout: self.timeout(),
                }
            }
            60..=74 => {
                let h = match self.rng.below(10) {
                    0 => None,
                    1 => Some(self.handle(k, pid, |h| {
                        matches!(h.object, Object::Device(d) if matches!(k.devices[&d].kind, DeviceKind::Irq { .. }))
                    })),
                    2 => Some(self.handle(k, pid, is_endpoint)),
                    _ => Some(self.handle(k, pid, |h| is_endpoint(h) && h.badge == 0)),
                };
                Syscall::Receive { h, timeout: self.timeout(), max_transfer: self.rng.below(4) }
            }
            75..=76 => Syscall::HandleClose { h: self.handle(k, pid, |_| true) },
            77..=86 => self.budget_create(k, pid),
            87..=89 => {
                // Destroying `root` ends the world; keep it rare.
                let h = self.handle(k, pid, |h| is_budget(h) && (h.object != Object::Budget(ROOT)));
                Syscall::BudgetDestroy { h }
            }
            90..=92 => Syscall::BudgetUsage { h: self.handle(k, pid, is_budget) },
            93..=94 => Syscall::Random,
            95 => {
                if self.rng.pct(3) {
                    Syscall::SystemReset {
                        h: self.handle(k, pid, |h| matches!(h.object, Object::Device(d) if k.devices[&d].kind == DeviceKind::Reset)),
                        kind: self.rng.range(0, 3),
                    }
                } else {
                    Syscall::SystemReset { h: self.handle(k, pid, is_endpoint), kind: 1 }
                }
            }
            96..=98 => {
                // A mix of a likely-free low address, address 0 (K5a-addr0: user space starts
                // at 0), an address near USER_TOP (where a lone-mapping process's alloc_va
                // fallback matters, P1-1), and now and then a page the process already owns
                // (own_pages), so the overlap refusal fires too.
                let addr = if self.rng.pct(20) {
                    self.own_pages(k, pid, 1, |_| true).0
                } else {
                    match self.rng.below(4) {
                        0 => 0,
                        1 => 0x1000_0000,
                        2 => USER_TOP - PAGE_SIZE,
                        _ => KERNEL_CHOSEN_BASE - PAGE_SIZE,
                    }
                };
                let n = if self.rng.pct(90) { self.rng.range(1, 4) } else { self.rng.range(0, 40) };
                Syscall::MapFixed { addr, len: n * PAGE_SIZE, flags: self.flags() }
            }
            _ => Syscall::TimeNow,
        }
    }

    fn some_handles(&mut self, k: &Kernel, pid: u64, max: u64) -> Vec<u64> {
        let n = self.rng.below(max + 1);
        (0..n).map(|_| self.handle(k, pid, |_| true)).collect()
    }

    fn budget_create(&mut self, k: &Kernel, pid: u64) -> Syscall {
        let parent = self.handle(k, pid, |h| matches!(h.object, Object::Budget(_)));
        let pb = match k.processes[&pid].handles.get(&parent).map(|h| h.object) {
            Some(Object::Budget(b)) => k.budgets.get(&b),
            _ => None,
        };
        let (free, free_proc, free_w, labels) = pb.map_or((10, 1, 10, vec![]), |b| {
            (
                b.pages_limit.saturating_sub(b.pages_used),
                b.processes_limit.saturating_sub(b.processes_used),
                b.weight.saturating_sub(b.weight_used),
                b.labels.clone(),
            )
        });
        let scope = self.rng.pct(15);
        let (pages, processes, weight) = if scope {
            (0, 0, 0)
        } else {
            let over = self.rng.pct(8);
            (
                if over { free + self.rng.range(1, 5) } else { self.rng.range(1, (free / 2).max(1)) },
                if over && self.rng.pct(50) { free_proc + 1 } else { self.rng.range(0, free_proc.min(2)) },
                if over && self.rng.pct(50) {
                    free_w + 1
                } else if self.rng.pct(5) {
                    // All of it: refused when the parent holds a process (R7/R12: free weight 0).
                    free_w
                } else {
                    self.rng.range(0, free_w.min(60))
                },
            )
        };
        let mut labels = labels;
        if self.rng.pct(20) {
            labels.push(self.rng.pick(&LABEL_POOL).unwrap());
        }
        if self.rng.pct(5) && !labels.is_empty() {
            labels.remove(0);
        }
        if self.rng.pct(3) {
            labels = (0..9).collect();
        }
        let deadline = match self.rng.below(10) {
            0 => k.now + self.rng.range(1, 30 * SLICE),
            1 => self.rng.below(k.now + 1),
            _ => FOREVER,
        };
        Syscall::BudgetCreate {
            parent,
            pages,
            processes,
            weight,
            labels,
            account: self.rng.pick(&ACCOUNT_POOL).unwrap(),
            deadline,
        }
    }
}

/// An address the model never hands out (below where the kernel places mappings), used for
/// hostile page ranges.
const KERNEL_CHOSEN_BASE_GUESS: u64 = 0x0800_0000;

#[cfg(test)]
mod subtree_regressions {
    use super::*;
    use crate::{invariants, kernel::Boot};

    fn actor(op: &Op) -> (u64, u64) {
        match *op {
            Op::Sys { pid, tid, .. }
            | Op::Write { pid, tid, .. }
            | Op::Read { pid, tid, .. }
            | Op::Exec { pid, tid, .. }
            | Op::Fault { pid, tid }
            | Op::Record { pid, tid, .. } => (pid, tid),
            _ => panic!("subtree sampling returned global event: {op:?}"),
        }
    }

    #[test]
    fn scoped_sampling_progresses_with_copy_fault_inside_and_outside() {
        for (outside_fault, inside_fault) in [(true, false), (false, true), (true, true)] {
            let mut initial = Kernel::boot(&Boot::default(), None).unwrap();
            let ep = initial.endpoint_create(INIT_PID).unwrap();
            let h = initial.process_create(INIT_PID, SYSTEM, ep).unwrap();
            let Object::Process(pid) = initial.processes[&INIT_PID].handles[&h].object else { panic!() };
            initial.process_start(INIT_PID, h, 0, 0, 0, &[]).unwrap();
            let tid = *initial.processes[&pid].threads.first().unwrap();
            for (p, t, faulty) in [(INIT_PID, 1, outside_fault), (pid, tid, inside_fault)] {
                if faulty {
                    initial.step(&Op::Record { pid: p, tid: t, record: Record::CopyFault }).unwrap();
                }
            }
            // Reproduce the old rejection sampler's trap without running an infinite loop:
            // global generation keeps returning the same unapplied outside repair.
            if outside_fault {
                let mut old = Gen::new(0);
                old.setup = u32::MAX;
                old.relay = 0;
                for _ in 0..8 {
                    assert_eq!(
                        old.next_op(&initial),
                        Op::Record { pid: INIT_PID, tid: 1, record: Record::Owned }
                    );
                }
            }
            for seed in 0..32 {
                let mut k = initial.clone();
                let mut gen = Gen::new(seed);
                let op = gen.next_op_in(&k, SYSTEM).unwrap();
                assert_eq!(actor(&op), (pid, tid));
                if inside_fault {
                    assert_eq!(op, Op::Record { pid, tid, record: Record::Owned });
                }
                assert!(!k.unsupported_receive_output(&op));
                k.step(&op).unwrap();
                invariants::check(&k).unwrap();
                if inside_fault {
                    let next = gen.next_op_in(&k, SYSTEM).unwrap();
                    assert_eq!(actor(&next), (pid, tid));
                    assert!(!matches!(next, Op::Record { .. }));
                    assert!(!k.unsupported_receive_output(&next));
                    k.step(&next).unwrap();
                    invariants::check(&k).unwrap();
                }
                assert_eq!(
                    k.threads[&1].record,
                    if outside_fault { Record::CopyFault } else { Record::Owned }
                );
            }
            assert!(Gen::new(0).next_op_in(&initial, USERS).is_none());
        }
    }
}
