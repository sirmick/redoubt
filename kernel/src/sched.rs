// SPDX-License-Identifier: MIT OR Apache-2.0

//! The scheduler: one stride queue over every runnable budget (KERNEL-SPEC.md R7, R12; RESOURCES.md,
//! Scheduling; the WP-K5 owner decisions). The rules themselves (charging with an exact remainder,
//! the floor, ranks, inheritance) are `redoubt-stride`'s, checked there against the executable
//! model; this module keeps their state in the budgets' frames and wires them to the CPU.
//!
//! # Accounting at the trap boundary
//! There are exactly two ways into user mode (`arch::syscall::resume` and the syscall return) and
//! one out (the trap handler). Runtime is counted at those, so no path can run user code
//! unaccounted, whatever ends it (a block, an exit, a fault, a kill, a callback):
//! - [`from_user`] (first thing on a trap from user mode) adds the user time since the last return to the
//!   pending runtime of the budget that ran, `cur`.
//! - [`begin_billing`] (after the entry's expiry) starts billing kernel time to `cur`: a system call's time
//!   is its caller's. Expiry work is not: each expired item is billed to its own budget as it is handled
//!   (`time.rs`), and the shared walk to nobody (the kernel).
//! - [`leave`] (on every return, to user mode or to `kmain`) closes the billing and, if the budget that runs
//!   next is not `cur`, deschedules `cur`: its pending runtime is folded into its pass (at least one tick)
//!   and it is requeued behind its equals. It then reconciles the queue (one reconcile per kernel entry:
//!   budgets that gained a runnable thread wake, those that lost their last one leave).
//!
//! A pass changes only at a fold: a deschedule, a weight change (a carve, or a carve returned,
//! folds first so earlier runtime is charged at the weight it ran at), a budget's destruction,
//! and a bill for expiry work.
//!
//! # Picking and preemption
//! `kmain` picks ([`pick`]): the queue's lowest rank, then the next thread of that budget after its
//! round-robin cursor, in (pid, tid) order; the pick starts a slice. The running thread keeps the
//! CPU until its slice ends, it blocks or exits, or a budget deadline fires; a wake never preempts
//! (`time.rs` arms the timer for the slice's end). A legacy direct switch (a borrowed quantum)
//! runs another budget with no pick: [`leave`] deschedules the old one and the slice continues
//! (INTERIM, until WP-K6).

use redoubt_abi::{PID, TID};
use redoubt_stride::{Budgets, Queue, State};

use crate::arch::process::{MAX_PROCESS_COUNT, MAX_THREAD};
use crate::budget::BudgetFrame;
use crate::cell::KernelCell;
use crate::handle::BudgetRef;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// Time slice, in microseconds (KERNEL-SPEC.md, Constants).
pub const SLICE_US: u64 = 10_000;

struct Sched {
    q: Queue<BudgetRef, MAX_PROCESS_COUNT>,
    /// The budget whose runtime is accruing: on the CPU, or in the kernel on its behalf.
    cur: Option<BudgetRef>,
    /// Ticks `cur` has run and not yet been charged.
    pending: u64,
    /// When `cur` last went to user mode, in ticks.
    user_since: Option<u64>,
    /// Kernel time since this tick is billed to this budget.
    billing: Option<(u64, BudgetRef)>,
    /// Billing paused while `kmain` expires deadlines (the walk is nobody's), to resume after.
    paused: Option<BudgetRef>,
}

static SCHED: KernelCell<Sched> = KernelCell::new(Sched {
    q: Queue::new(),
    cur: None,
    pending: 0,
    user_since: None,
    billing: None,
    paused: None,
});

fn ticks() -> u64 { crate::arch::irq::timer::now_ticks() }

impl Budgets<BudgetRef> for MemoryManager {
    fn state(&self, b: BudgetRef) -> State { self.sched_state(b.frame) }

    fn set_state(&mut self, b: BudgetRef, s: State) { self.set_sched_state(b.frame, &s) }

    fn id(&self, b: BudgetRef) -> u64 { b.id }

    /// Stride weight is free weight: what the budget has not carved to children (R7).
    fn weight(&self, b: BudgetRef) -> u64 { self.free_weight_of(b.frame) }
}

fn free_weight(mm: &MemoryManager, frame: BudgetFrame) -> u64 { mm.free_weight_of(frame) }

fn budget_ref(mm: &MemoryManager, frame: BudgetFrame) -> BudgetRef {
    BudgetRef { frame, id: mm.budget_id(frame) }
}

impl Sched {
    /// Close the kernel-time billing interval at `now`: `cur`'s joins its pending runtime, anyone
    /// else's is charged at once.
    fn close_billing(&mut self, mm: &mut MemoryManager, now: u64) {
        if let Some((since, b)) = self.billing.take() {
            let dt = now.saturating_sub(since);
            if Some(b) == self.cur {
                self.pending = self.pending.saturating_add(dt);
            } else if mm.is_live_budget(b) {
                self.q.fold(mm, b, dt);
            }
        }
    }

    /// Fold `cur`'s pending runtime into its pass.
    fn fold_cur(&mut self, mm: &mut MemoryManager) {
        if let Some(c) = self.cur {
            let run = core::mem::take(&mut self.pending);
            if mm.is_live_budget(c) {
                self.q.fold(mm, c, run);
            }
        }
    }

    /// If `b` is accruing runtime, charge what it has so far (at its present weight) and reopen
    /// its billing interval: a weight change or a creation under it follows.
    fn settle(&mut self, mm: &mut MemoryManager, b: BudgetRef) {
        let now = ticks();
        if self.billing.is_some_and(|(_, payer)| payer == b) {
            self.close_billing(mm, now);
            self.billing = Some((now, b));
        }
        if self.cur == Some(b) {
            self.fold_cur(mm);
        }
    }

    /// Take `cur` off the CPU: fold at least a tick, and requeue it behind its equals (the next
    /// reconcile takes it out if it has no runnable thread left).
    fn deschedule(&mut self, mm: &mut MemoryManager) {
        let Some(c) = self.cur.take() else { return };
        let run = core::mem::take(&mut self.pending).max(1);
        if mm.is_live_budget(c) {
            self.q.fold(mm, c, run);
            self.q.deschedule(mm, c, true);
        }
        self.user_since = None;
    }

    /// Budgets with a thread waiting for the CPU, each once.
    fn runnable(ss: &SystemServices, mm: &MemoryManager) -> ([Option<BudgetRef>; MAX_PROCESS_COUNT], usize) {
        let mut out = [None; MAX_PROCESS_COUNT];
        let mut n = 0;
        for p in ss.processes.iter() {
            if p.free() || p.pid.get() == 1 {
                continue;
            }
            let ready = p.ready_threads().is_none_or(|x| x != 0);
            if !ready {
                continue;
            }
            let Some(frame) = mm.budget_of(p.pid) else { continue };
            let b = budget_ref(mm, frame);
            if !out[..n].contains(&Some(b)) && n < out.len() {
                out[n] = Some(b);
                n += 1;
            }
        }
        (out, n)
    }

    fn reconcile(&mut self, ss: &SystemServices, mm: &mut MemoryManager) {
        let (list, n) = Self::runnable(ss, mm);
        let mut budgets = [BudgetRef { frame: 0, id: 0 }; MAX_PROCESS_COUNT];
        for (dst, src) in budgets.iter_mut().zip(list[..n].iter()) {
            *dst = src.expect("filled");
        }
        let running = self.cur.filter(|c| mm.is_live_budget(*c));
        self.q.reconcile(mm, running, &budgets[..n]);
    }
}

/// A trap from user mode: the user time since the last return is `cur`'s.
pub fn from_user() {
    let now = ticks();
    SCHED.with(|s| {
        if let Some(since) = s.user_since.take() {
            s.pending = s.pending.saturating_add(now.saturating_sub(since));
        }
    });
}

/// The entry's expiry is done: from here, kernel time is `cur`'s (a system call's is its
/// caller's).
pub fn begin_billing() {
    let now = ticks();
    SCHED.with(|s| s.billing = s.cur.map(|b| (now, b)));
}

/// Kernel time since billing began goes to nobody: it was spent for someone else (an interrupt
/// handled for its device's owner, [`bill`]).
pub fn restart_billing() {
    let now = ticks();
    SCHED.with(|s| {
        if let Some((_, b)) = s.billing {
            s.billing = Some((now, b));
        }
    });
}

/// `kmain` is about to expire deadlines: its walk is nobody's work, and each expired item bills
/// its own budget (`time.rs`). Billing resumes after ([`resume_billing`]).
pub fn pause_billing() {
    let now = ticks();
    MemoryManager::with_mut(|mm| {
        SCHED.with(|s| {
            s.paused = s.billing.map(|(_, b)| b);
            s.close_billing(mm, now);
        })
    });
}

pub fn resume_billing() {
    let now = ticks();
    SCHED.with(|s| s.billing = s.paused.take().map(|b| (now, b)));
}

/// `kmain` idles: nobody's work.
pub fn stop_billing() {
    let now = ticks();
    MemoryManager::with_mut(|mm| SCHED.with(|s| s.close_billing(mm, now)));
}

/// Charge `ticks` of kernel work done for `b` (an expired timeout of one of its threads, its
/// destruction, an interrupt of its device) to it.
pub fn bill(mm: &mut MemoryManager, b: BudgetRef, ticks: u64) {
    SCHED.with(|s| {
        if s.cur == Some(b) {
            s.pending = s.pending.saturating_add(ticks);
        } else if mm.is_live_budget(b) {
            s.q.fold(mm, b, ticks);
        }
    });
}

/// An interrupt was handled from tick `started`: bill that to the owner of its device object
/// (R5), if it has one, and restart billing the interrupted budget from now.
pub fn bill_irq(irq: usize, started: u64) {
    let dt = ticks().saturating_sub(started);
    MemoryManager::with_mut(|mm| {
        if let Some(frame) = mm.irq_device(irq) {
            let owner = mm.device(frame).owner;
            bill(mm, owner, dt);
        }
    });
    restart_billing();
}

/// The ticks now, for measuring a piece of kernel work to [`bill`].
pub fn now_ticks() -> u64 { ticks() }

/// Leaving the kernel for `pid` (the kernel itself for PID 1): close the billing, deschedule the
/// budget that ran if another runs now, reconcile, and start counting user time.
pub fn leave(pid: PID) {
    let now = ticks();
    SystemServices::with(|ss| {
        MemoryManager::with_mut(|mm| {
            SCHED.with(|s| {
                s.close_billing(mm, now);
                let next = if pid.get() == 1 { None } else { mm.budget_of(pid).map(|f| budget_ref(mm, f)) };
                if next != s.cur {
                    let left = s.cur;
                    s.deschedule(mm);
                    s.cur = next;
                    // `kmain`'s pick and switch after a deschedule are the descheduled budget's
                    // work (it blocked, exited or was preempted): billed to it, as a deschedule's
                    // cost, until the next budget runs.
                    if next.is_none() {
                        s.billing = left.filter(|b| mm.is_live_budget(*b)).map(|b| (now, b));
                    }
                }
                s.reconcile(ss, mm);
                s.user_since = next.map(|_| now);
            })
        })
    });
    if pid.get() == 1 {
        crate::time::set_slice_end(crate::time::NEVER);
    }
    crate::time::rearm();
}

/// What `kmain` runs next: the lowest-ranked queued budget's next thread after its cursor. Starts
/// a slice. `None` when nothing is runnable.
pub fn pick(ss: &SystemServices, mm: &mut MemoryManager) -> Option<(PID, TID)> {
    let chosen = SCHED.with(|s| {
        s.reconcile(ss, mm);
        loop {
            let b = s.q.pick(mm)?;
            if let Some(t) = next_thread(ss, mm, b) {
                return Some((b, t));
            }
            // Queued with nothing to run: out it goes (never expected; reconcile keeps them in step).
            s.q.deschedule(mm, b, false);
        }
    });
    let (b, (pid, tid)) = chosen?;
    let mut x = mm.budget(b.frame);
    x.cursor = Some((pid.get(), tid as u8));
    mm.store(b.frame, &x);
    crate::time::set_slice_end(crate::time::now_us().saturating_add(SLICE_US));
    Some((pid, tid))
}

/// The next runnable thread of budget `b` after its cursor, in (pid, tid) order, wrapping; a tid
/// of 0 leaves the choice to `activate_process_thread` (a process being set up or handling an
/// exception).
fn next_thread(ss: &SystemServices, mm: &MemoryManager, b: BudgetRef) -> Option<(PID, TID)> {
    let cursor = mm.budget(b.frame).cursor.map(|(p, t)| (p, t as usize));
    let mut first: Option<(PID, TID)> = None;
    let mut after: Option<(PID, TID)> = None;
    for p in ss.processes.iter() {
        if p.free() || p.running() || p.pid.get() == 1 || mm.budget_of(p.pid) != Some(b.frame) {
            continue;
        }
        let tids: [bool; MAX_THREAD + 1] = match p.ready_threads() {
            Some(mask) => core::array::from_fn(|t| mask & (1 << t) != 0),
            None => core::array::from_fn(|t| t == 0),
        };
        for (tid, _) in tids.iter().enumerate().filter(|(_, r)| **r) {
            let key = (p.pid.get(), tid);
            first.get_or_insert((p.pid, tid));
            if after.is_none() && cursor.is_some_and(|c| key > c) {
                after = Some((p.pid, tid));
            }
        }
    }
    after.or(first)
}

/// A budget is created under `parent`: fold a running parent's runtime, then the child enters at
/// `max(floor, parent's pass)`. The carve and [`reweigh`] follow.
pub fn create(mm: &mut MemoryManager, child: BudgetFrame, parent: Option<BudgetFrame>) {
    let child = budget_ref(mm, child);
    let parent = parent.map(|p| budget_ref(mm, p));
    SCHED.with(|s| {
        if let Some(p) = parent {
            s.settle(mm, p);
        }
        s.q.create(mm, child, parent);
    });
}

/// `b`'s stride weight is about to change: charge what it ran at the old weight (callers change
/// the carve after this and then call [`reweigh`]).
pub fn before_weight_change(mm: &mut MemoryManager, b: BudgetFrame) {
    let b = budget_ref(mm, b);
    SCHED.with(|s| s.settle(mm, b));
}

/// `b`'s stride weight changed from `old` to `new`: rescale its remainder.
pub fn reweigh(mm: &mut MemoryManager, b: BudgetFrame, old: u64, new: u64) {
    let b = budget_ref(mm, b);
    SCHED.with(|s| s.q.reweigh(mm, b, old, new));
}

/// `b`, dying, is being destroyed (its descendants already were, bottom-up): charge what it ran,
/// return its carve to its parent (unless it came back already: the subtree's top returns its
/// weight first), move its work since entry there (added to the parent's lead, normalized by the
/// parent's weight now), and take it out of the queue. Its frame is freed after this.
pub fn destroy(mm: &mut MemoryManager, frame: BudgetFrame, weight_returned: bool) {
    let child = budget_ref(mm, frame);
    let parent = mm.budget(frame).parent;
    SCHED.with(|s| {
        s.settle(mm, child);
        if s.cur == Some(child) {
            s.cur = None;
            s.user_since = None;
        }
        if s.billing.is_some_and(|(_, b)| b == child) {
            s.billing = None;
        }
        let w_child = free_weight(mm, frame);
        let limit = mm.budget(frame).weight_limit;
        let (parent, w_parent) = match parent {
            Some(p) => {
                let pref = budget_ref(mm, p);
                if !weight_returned {
                    s.settle(mm, pref);
                    let old = free_weight(mm, p);
                    let mut pb = mm.budget(p);
                    pb.weight_carved = pb.weight_carved.checked_sub(limit).expect("I5: carve underflow");
                    mm.store(p, &pb);
                    let new = free_weight(mm, p);
                    s.q.reweigh(mm, pref, old, new);
                }
                (Some(pref), free_weight(mm, p))
            }
            None => (None, 0),
        };
        s.q.destroy(mm, child, parent, w_child, w_parent);
    });
}

/// Whether the running thread's slice is over (`time.rs` arms for its end).
pub fn slice_over() -> bool { crate::time::slice_end() <= crate::time::now_us() }

/// The running thread is preempted (its slice ended, or a budget deadline fired): it stays ready,
/// and the CPU goes to `kmain`, which picks again. Its budget is descheduled as the kernel leaves
/// for `kmain` ([`leave`]).
pub fn preempt(ss: &mut SystemServices, tid: TID) {
    crate::syscall::reset_switchto_caller();
    let kernel = PID::new(1).expect("PID 1");
    ss.activate_process_thread(tid, kernel, 0, true, crate::services::PostActivateOp::None)
        .expect("the kernel can always run");
    crate::syscall::restore_last_thread(ss);
}
