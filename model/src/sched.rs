//! R12 (answers 103, 166 and the K5 owner decisions): one flat weighted stride queue over every
//! runnable budget; round-robin threads within each budget.
//!
//! The kernel model tells the scheduler when a thread becomes runnable or stops being runnable
//! ([`Scheduler::thread_runnable`], [`Scheduler::thread_blocked`]), when a budget is created,
//! carves or is destroyed, and when time passes for the running thread ([`Scheduler::run`]). It
//! asks what to run ([`Scheduler::pick`]) and ends slices ([`Scheduler::slice_end`]) or preempts
//! at a budget deadline ([`Scheduler::preempt`]).
//!
//! The rules, as the owner approved them for WP-K5 (KERNEL-SPEC.md, R7/R12):
//! - **Stride weight is free weight**: a budget's weight limit less what it carved to children, so carving
//!   moves share and never duplicates it.
//! - **Charging** happens at a *fold*: pending runtime is added as `t = rem + runtime·STRIDE; pass += t / w;
//!   rem = t % w`, an exact remainder. Runtime is folded at every deschedule and before every weight change,
//!   so it is charged at the weight it ran at.
//! - **Floor**: a monotone lower bound, raised to the queue's minimum pass whenever that rises; it survives
//!   an empty queue. A waking budget's pass is `max(own, floor)`.
//! - **Rank at an equal pass**: wakers ahead of requeued budgets; a later reconcile's wake ahead of an
//!   earlier one's; within one reconcile the lower budget id ahead; requeues FIFO. Encoded as a tie key:
//!   wakes take `front -= 1` (same-reconcile wakes processed in descending id), every deschedule of a
//!   still-runnable budget takes `back += 1`; both reset when the queue empties.
//! - **Preemption** only at slice end or a budget deadline; a wake never preempts.
//! - **Deschedule**: a budget taken off the CPU is charged what it ran, and at least [`MIN_CHARGE`] (owner
//!   decision 5: the kernel's clock is a timebase tick, and a run too short for it to see is not free).
//! - **Pass inheritance**: a child enters at `e = max(floor, parent.pass)`, kept as its `entry`; at its
//!   destruction its own unpaid work since entry, `W = (pass − max(entry, floor))⁺·w + rem`, is added to the
//!   parent's lead: `parent = max(parent.pass, floor) + W / w_parent` (remainder carried), `w_parent` taken
//!   after the child's weight returns.
//!
//! Time is whatever unit the caller uses (the model's microseconds); the kernel charges timebase
//! ticks with the same arithmetic.

use alloc::collections::{BTreeMap, BTreeSet};

use crate::mutation::Mutation;
use crate::spec::{SLICE, STRIDE};

/// The most runtime one fold charges: `RUNTIME_CAP · STRIDE` stays below 2^60, so a charge and a
/// remainder below 2^32 fit in 64 bits.
pub const RUNTIME_CAP: u64 = 1 << 40;

/// The least a deschedule charges (one of the caller's time units; the kernel's timebase tick).
pub const MIN_CHARGE: u64 = 1;

/// A thread: (pid, tid). Round-robin within a budget follows this order.
pub type ThreadId = (u64, u64);

#[derive(Clone, Debug)]
pub struct Entry {
    pub parent: Option<u64>,
    /// The budget's weight limit and what it carved to children (R7).
    pub limit: u64,
    pub carved: u64,
    pub pass: u128,
    /// Where it entered the queue's virtual time at creation.
    pub entry: u128,
    /// The exact remainder of charging, below the stride weight.
    pub rem: u64,
    /// Rank among equal passes (module docs).
    pub tie: i64,
    /// In the queue: it has a runnable thread (or is running), as of the last reconcile.
    pub queued: bool,
    pub runnable: BTreeSet<ThreadId>,
    /// The thread it ran last, for round-robin.
    pub cursor: Option<ThreadId>,
    /// Its carve is back with its parent already ([`Scheduler::return_carve`]: the top of a
    /// destruction, at its start).
    pub returned: bool,
}

/// The thread on the CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Current {
    pub budget: u64,
    pub thread: ThreadId,
    /// Runtime not yet folded into the budget's pass.
    pub pending: u64,
    /// What is left of its slice.
    pub slice_left: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Scheduler {
    pub mutation: Option<Mutation>,
    pub budgets: BTreeMap<u64, Entry>,
    pub floor: u128,
    pub front: i64,
    pub back: i64,
    pub current: Option<Current>,
}

impl Scheduler {
    fn broken(&self, m: Mutation) -> bool { self.mutation == Some(m) }

    /// A budget's stride weight: its free weight (or, broken, its limit).
    pub fn weight(&self, b: u64) -> u64 {
        self.budgets.get(&b).map_or(0, |e| {
            if self.broken(Mutation::R12StrideWeightIsLimit) {
                e.limit
            } else {
                e.limit.saturating_sub(e.carved)
            }
        })
    }

    fn queued(&self) -> impl Iterator<Item = (&u64, &Entry)> { self.budgets.iter().filter(|(_, e)| e.queued) }

    /// Raise the floor to the queue's minimum pass (it never falls, and holds while the queue is
    /// empty).
    fn raise_floor(&mut self) {
        if let Some(min) = self.queued().map(|(_, e)| e.pass).min() {
            self.floor = self.floor.max(min);
        }
    }

    /// Charge `runtime` to `b` at its current stride weight, with the exact remainder.
    fn charge(&mut self, b: u64, runtime: u64) {
        let w = if self.broken(Mutation::R12IgnoreWeight) { 1 } else { self.weight(b) };
        let short_free = self.broken(Mutation::R12ShortRunsFree) && runtime < SLICE;
        let drop_rem = self.broken(Mutation::R12DropRemainder);
        let Some(e) = self.budgets.get_mut(&b) else { return };
        if w == 0 || short_free {
            return;
        }
        let rem = if drop_rem { 0 } else { e.rem };
        // rem < w < 2^32 and runtime·STRIDE < 2^60: no overflow.
        let t = rem + runtime.min(RUNTIME_CAP) * STRIDE;
        e.pass += u128::from(t / w);
        e.rem = if drop_rem { 0 } else { t % w };
    }

    /// Fold the running thread's pending runtime into its budget.
    pub fn fold(&mut self) {
        if let Some(c) = self.current.as_mut() {
            let (b, run) = (c.budget, c.pending);
            c.pending = 0;
            self.charge(b, run);
            self.raise_floor();
        }
    }

    /// Fold if `b` is running (a weight change or a creation under it is about to read or change
    /// what its runtime is worth).
    fn fold_if_running(&mut self, b: u64) {
        if self.current.is_some_and(|c| c.budget == b) {
            self.fold();
        }
    }

    /// `b`'s stride weight is about to change to `new` (a carve, or a returned carve): fold at the
    /// old weight first, then rescale the remainder (error below one pass unit).
    fn reweigh(&mut self, b: u64, old: u64, new: u64) {
        let rescale = |rem: u64| {
            if old == 0 { 0 } else { ((u128::from(rem) * u128::from(new)) / u128::from(old)) as u64 }
        };
        if let Some(e) = self.budgets.get_mut(&b) {
            e.rem = rescale(e.rem).min(new.saturating_sub(1));
        }
    }

    /// A new budget `id` with weight limit `limit` is carved from `parent` (none for a root).
    pub fn add_budget(&mut self, id: u64, parent: Option<u64>, limit: u64) {
        let parent_pass = parent.and_then(|p| {
            if !self.broken(Mutation::R12FoldAtNewWeight) {
                self.fold_if_running(p);
            }
            self.budgets.get(&p).map(|e| e.pass)
        });
        let e = if self.broken(Mutation::R12CreateAtFloorOnly) {
            self.floor
        } else {
            parent_pass.map_or(self.floor, |p| p.max(self.floor))
        };
        if let Some(p) = parent {
            let old = self.weight(p);
            if let Some(x) = self.budgets.get_mut(&p) {
                x.carved = x.carved.saturating_add(limit);
            }
            let new = self.weight(p);
            self.reweigh(p, old, new);
        }
        self.budgets.insert(
            id,
            Entry {
                parent,
                limit,
                carved: 0,
                pass: e,
                entry: e,
                rem: 0,
                tie: 0,
                queued: false,
                runnable: BTreeSet::new(),
                cursor: None,
                returned: false,
            },
        );
    }

    /// The first step of destroying `b` and everything below it: `b`'s carve comes back to its
    /// parent (fold at the old weight first, then rescale), before any of the destruction's own
    /// work is charged, so the parent pays for it at the weight it has once `b` is gone
    /// (K5-code-review-4 D1). [`Scheduler::destroy_budget`] then returns nothing for `b`.
    pub fn return_carve(&mut self, b: u64) {
        let Some(child) = self.budgets.get(&b).cloned() else { return };
        let Some(p) = child.parent.filter(|p| self.budgets.contains_key(p)) else { return };
        if child.returned {
            return;
        }
        if !self.broken(Mutation::R12FoldAtNewWeight) {
            self.fold_if_running(p);
        }
        let old = self.weight(p);
        if let Some(x) = self.budgets.get_mut(&p) {
            x.carved = x.carved.saturating_sub(child.limit);
        }
        let new = self.weight(p);
        self.reweigh(p, old, new);
        self.budgets.get_mut(&b).unwrap().returned = true;
    }

    /// Budget `b` is destroyed; its descendants already were (bottom-up, R10 order), and every
    /// thread in it has already been blocked or removed. Its work since entry moves to its parent,
    /// and its carved weight returns there.
    pub fn destroy_budget(&mut self, b: u64) {
        if self.current.is_some_and(|c| c.budget == b) {
            // Its threads are gone; whatever ran is charged here before it moves up.
            self.fold();
            self.current = None;
        }
        let Some(child) = self.budgets.get(&b).cloned() else { return };
        let w_child = self.weight(b);
        if let Some(p) = child.parent.filter(|p| self.budgets.contains_key(p)) {
            if !self.broken(Mutation::R12FoldAtNewWeight) {
                self.fold_if_running(p);
            }
            let old = self.weight(p);
            if !child.returned {
                if let Some(x) = self.budgets.get_mut(&p) {
                    x.carved = x.carved.saturating_sub(child.limit);
                }
            }
            let w_parent = self.weight(p);
            self.reweigh(p, old, w_parent);
            // The floor after both folds.
            let f = self.floor;
            self.lift(p, &child, w_child, w_parent, f);
        }
        self.budgets.remove(&b);
        if !self.budgets.values().any(|e| e.queued) {
            self.front = 0;
            self.back = 0;
        }
        self.raise_floor();
    }

    /// Move a destroyed child's unpaid work to its parent (module docs).
    fn lift(&mut self, p: u64, child: &Entry, w_child: u64, w_parent: u64, f: u128) {
        if self.broken(Mutation::R12DestroyDropsDebt) {
            return;
        }
        let unnormalized = self.broken(Mutation::R12UnnormalizedLift);
        let by_max = self.broken(Mutation::R12LiftByMax);
        let from = if self.broken(Mutation::R12LiftCountsEntryWait) { f } else { child.entry.max(f) };
        let Some(x) = self.budgets.get_mut(&p) else { return };
        if unnormalized {
            x.pass = x.pass.max(child.pass);
            return;
        }
        if w_parent == 0 {
            return;
        }
        let work = child.pass.saturating_sub(from) * u128::from(w_child) + u128::from(child.rem);
        let (q, r) = (work / u128::from(w_parent), (work % u128::from(w_parent)) as u64);
        if by_max {
            if f + q > x.pass || (f + q == x.pass && r > x.rem) {
                x.pass = f + q;
                x.rem = r;
            }
            return;
        }
        let (base, base_rem) = if x.pass >= f { (x.pass, x.rem) } else { (f, 0) };
        let sum = u128::from(base_rem) + u128::from(r);
        let carry = sum / u128::from(w_parent);
        x.pass = base + q + carry;
        x.rem = (sum % u128::from(w_parent)) as u64;
    }

    /// Thread `t` of budget `b` became runnable. The budget wakes at the next reconcile if it had
    /// none.
    pub fn thread_runnable(&mut self, b: u64, t: ThreadId) {
        if let Some(e) = self.budgets.get_mut(&b) {
            e.runnable.insert(t);
        }
    }

    /// Thread `t` of budget `b` ended (exited, faulted or was killed). Ending on the CPU is a
    /// deschedule like any other: its partial slice is charged.
    pub fn thread_exited(&mut self, b: u64, t: ThreadId) {
        if self.broken(Mutation::R12ExitRunsFree) {
            if let Some(c) = self.current.as_mut().filter(|c| c.budget == b && c.thread == t) {
                c.pending = 0;
            }
        }
        self.thread_blocked(b, t);
    }

    /// Thread `t` of budget `b` is no longer runnable (blocked or gone). If it was running, its
    /// budget is descheduled.
    pub fn thread_blocked(&mut self, b: u64, t: ThreadId) {
        if let Some(e) = self.budgets.get_mut(&b) {
            e.runnable.remove(&t);
        }
        if self.current.is_some_and(|c| c.budget == b && c.thread == t) {
            self.deschedule();
        }
    }

    /// Take the running thread off the CPU: fold, and requeue its budget if it still has a
    /// runnable thread, or take it out of the queue.
    fn deschedule(&mut self) {
        if !self.broken(Mutation::R12NoMinimumCharge) {
            if let Some(c) = self.current.as_mut() {
                c.pending = c.pending.max(MIN_CHARGE);
            }
        }
        self.fold();
        let Some(c) = self.current.take() else { return };
        let still = self.budgets.get(&c.budget).is_some_and(|e| !e.runnable.is_empty());
        if still {
            // Requeued behind its equals.
            let tie = if self.broken(Mutation::R12RequeueAhead) {
                self.front -= 1;
                self.front
            } else {
                self.back = self.back.saturating_add(1);
                if self.broken(Mutation::R12RequeueLifo) { i64::MAX - self.back } else { self.back }
            };
            self.budgets.get_mut(&c.budget).unwrap().tie = tie;
        } else if let Some(e) = self.budgets.get_mut(&c.budget) {
            e.queued = false;
        }
        self.raise_floor();
        if !self.budgets.values().any(|e| e.queued) {
            self.front = 0;
            self.back = 0;
        }
    }

    /// End of one kernel entry (a model op, or one timer instant inside a tick): budgets that lost
    /// their last runnable thread leave the queue, and budgets that gained one wake, in
    /// descending id so that the lowest id ranks first.
    pub fn reconcile(&mut self) {
        let running = self.current.map(|c| c.budget);
        for (id, e) in self.budgets.iter_mut() {
            if e.queued && e.runnable.is_empty() && running != Some(*id) {
                e.queued = false;
            }
        }
        self.raise_floor();
        let was_empty = !self.budgets.values().any(|e| e.queued);
        if was_empty {
            self.front = 0;
            self.back = 0;
        }
        let waking: alloc::vec::Vec<u64> = self
            .budgets
            .iter()
            .filter(|(_, e)| !e.queued && !e.runnable.is_empty())
            .map(|(id, _)| *id)
            .collect();
        // Broken: a budget waking into an empty queue keeps its own pass.
        let banks = self.broken(Mutation::R12WakeBanksCredit)
            || (was_empty && self.broken(Mutation::R12NoFloorWhenIdle));
        let queued_first = self.broken(Mutation::R12TieQueuedFirst);
        let floor = self.floor;
        for id in waking.into_iter().rev() {
            let tie = if queued_first {
                self.back = self.back.saturating_add(1);
                self.back
            } else {
                self.front = self.front.saturating_sub(1);
                self.front
            };
            let e = self.budgets.get_mut(&id).unwrap();
            if !banks {
                e.pass = e.pass.max(floor);
            }
            e.tie = tie;
            e.queued = true;
        }
        self.raise_floor();
        if self.broken(Mutation::R12PreemptOnWake) {
            if let Some(c) = self.current {
                let cur = self.budgets.get(&c.budget).map(|e| (e.pass, e.tie, c.budget));
                let best = self.queued().map(|(id, e)| (e.pass, e.tie, *id)).min();
                if best.is_some_and(|b| Some(b) < cur) {
                    self.deschedule();
                }
            }
        }
    }

    /// The rank key a pick minimizes.
    fn key(&self, id: u64, e: &Entry) -> (u64, u128, i64, u64) {
        let tier = if self.broken(Mutation::R12PriorityById) { id } else { 0 };
        (tier, e.pass, e.tie, id)
    }

    /// What runs: the current thread, or (after a reconcile) the lowest-ranked queued budget's next
    /// thread after its cursor. A new pick starts a fresh slice.
    pub fn pick(&mut self) -> Option<Current> {
        self.reconcile();
        if self.current.is_some() {
            return self.current;
        }
        let (id, _) = self.queued().map(|(id, e)| (self.key(*id, e), *id)).min().map(|(k, id)| (id, k))?;
        let e = self.budgets.get_mut(&id).unwrap();
        let next = match e.cursor {
            Some(c) => e.runnable.range((core::ops::Bound::Excluded(c), core::ops::Bound::Unbounded)).next(),
            None => None,
        }
        .or_else(|| e.runnable.iter().next())
        .copied()?;
        e.cursor = Some(next);
        self.current = Some(Current { budget: id, thread: next, pending: 0, slice_left: SLICE });
        self.current
    }

    /// The running thread ran for `dt` (at most what is left of its slice).
    pub fn run(&mut self, dt: u64) {
        if let Some(c) = self.current.as_mut() {
            let dt = dt.min(c.slice_left);
            c.pending += dt;
            c.slice_left -= dt;
        }
    }

    /// The running thread's slice ended: deschedule it (it is requeued if still runnable).
    pub fn slice_end(&mut self) {
        if self.current.is_some_and(|c| c.slice_left == 0) {
            self.deschedule();
        }
    }

    /// A budget deadline fired: the running thread is preempted (re-pick).
    pub fn preempt(&mut self) {
        if self.current.is_some() {
            self.deschedule();
        }
    }

    /// With exactly one budget queued and running a fresh slice, `n` whole slices go to it: charge
    /// them at once and advance its round-robin `n` threads (the same passes and order as slice by
    /// slice).
    pub fn run_slices(&mut self, n: u64) {
        let Some(c) = self.current else { return };
        if n == 0 {
            return;
        }
        // The first slice is the current one (charged at the deschedule below); the rest rotate
        // through its threads.
        self.run(SLICE);
        let b = c.budget;
        // n − 1 further slices: charged together (the remainder makes this exact), or one by one
        // where a mutation makes a split charge differ from a whole one.
        if n > 1 {
            if self.broken(Mutation::R12DropRemainder) {
                for _ in 1..n {
                    self.charge(b, SLICE);
                }
            } else {
                let mut left = (n - 1).saturating_mul(SLICE);
                while left > 0 {
                    let chunk = left.min(RUNTIME_CAP);
                    self.charge(b, chunk);
                    left -= chunk;
                }
            }
            // The n − 1 requeues the batch skips (the last is `deschedule` below).
            if self.broken(Mutation::R12RequeueAhead) {
                self.front = self.front.saturating_sub(n as i64 - 1);
            } else {
                self.back = self.back.saturating_add(n as i64 - 1);
            }
        }
        let e = self.budgets.get_mut(&b).unwrap();
        let threads: alloc::vec::Vec<ThreadId> = e.runnable.iter().copied().collect();
        let at = threads.iter().position(|t| Some(*t) == e.cursor).unwrap_or(0);
        let last = threads[(at + (n as usize - 1) % threads.len()) % threads.len()];
        e.cursor = Some(last);
        self.current = Some(Current { budget: b, thread: last, pending: SLICE, slice_left: 0 });
        self.deschedule();
        self.raise_floor();
    }

    /// Budgets that are queued (runnable, or running).
    pub fn runnable_budgets(&self) -> usize { self.queued().count() }
}
