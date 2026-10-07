// SPDX-License-Identifier: MIT OR Apache-2.0

//! The scheduler: one stride queue over every runnable budget (kernel/scheduling.md; R7, R12).
//! The rules themselves (charging with an exact remainder, the floor, ranks, inheritance) and
//! their wiring to a CPU (when runtime is folded, what a deschedule, a pick, a weight change and a
//! destruction do, in what order) are `redoubt-stride`'s ([`Harts`], a runner per hart), checked there
//! against the executable model; this module keeps their state in the budgets' frames and drives them from
//! the trap boundary.
//!
//! # Accounting at the trap boundary
//! There are exactly two ways into user mode (`arch::syscall::resume` and the syscall return) and
//! one out (the trap handler). Runtime is counted at those, so no path can run user code
//! unaccounted, whatever ends it (a block, an exit, a fault or a kill):
//! - [`from_user`] (first thing on a trap from user mode) adds the user time since the last return to the
//!   pending runtime of the budget that ran, `cur`.
//! - Expiry work comes first: each expired item is billed to its own budget as it is handled, a walk that
//!   finds only a wait ended before its timeout to that wait's budget, and the last walk, which finds nothing
//!   more, to the budget billed last (`time.rs`).
//! - [`begin_billing`] (after the entry's expiry) starts billing kernel time to `cur`: a system call's time
//!   is its caller's. A timer interrupt's is not: the rest of an entry that found something is the budget's
//!   billed last ([`bill_from_now`]), and of one that found nothing, `cur`'s if it ends its slice and
//!   nobody's otherwise (`arch::irq`).
//! - [`leave`] (on every return, to user mode or to `kmain`) closes the billing and, if the budget that runs
//!   next is not `cur`, deschedules `cur`: its pending runtime is folded into its pass (at least one tick)
//!   and it is requeued behind its equals if it still has a runnable thread. It then reconciles the queue
//!   (one reconcile per kernel entry: budgets that gained a runnable thread wake, those that lost their last
//!   one leave). The entry's payer pays for all of that, up to the return; user time starts there.
//!
//! A reconcile visits only the budgets whose runnable state changed: every change of a process's
//! state that changes how many of its threads are ready marks it (`ptable.rs`), and the marks move
//! its count to its budget, so whether a budget has a runnable thread is one read ([`Marks`]).
//!
//! A pass changes only at a fold: a deschedule, a weight change (a carve, or a carve returned,
//! folds first so earlier runtime is charged at the weight it ran at), a budget's destruction,
//! and a bill for expiry work.
//!
//! # Picking and preemption
//! `kmain` picks ([`pick`]): the queue's lowest rank, then the next thread of that budget after its
//! round-robin cursor, in (pid, tid) order; its slice starts when it returns to user mode
//! ([`leave`]). The running thread keeps the CPU until its slice ends, it blocks or exits, or a
//! budget deadline fires; a wake never preempts (`time.rs` arms the timer for the slice's end).
//!
//! # `kmain`'s switch
//! `kmain` runs its pick with [`switch_to`], a private S-mode `ecall` into the kernel's own trap
//! handler, which saves `kmain`'s context as it saves a thread's and resumes the picked thread
//! ([`switch`]). It is not a system call: its tag is outside the call table, and a user-mode
//! `ecall` with it is an unknown number (`InvalidArgument`) like any other.

use redoubt_layout::{KERNEL_PID, Pid};
use redoubt_stride::{Budgets, Harts, Marks, Ready, State};

use crate::arch::hart::MAX_HARTS;
use crate::arch::process::MAX_PROCESS_COUNT;
use crate::arch::process::{TID, TidMask};
use crate::budget::BudgetFrame;
use crate::cell::KernelCell;
use crate::handle::BudgetRef;
use crate::mem::MemoryManager;
use crate::ptable::{ArchProcess, ProcessTable};

/// Time slice, in microseconds (kernel/timer.md, "The hart timer"). The test-only `slice-10ms`
/// keeps the old 10 ms, for the cluster's known-bad control (kernel/scheduling.md,
/// "Responsiveness").
pub const SLICE_US: u64 = if cfg!(feature = "slice-10ms") { 10_000 } else { 1_000 };

struct Sched {
    /// The queue and, for each hart, the budget whose runtime is accruing there (on the hart, or
    /// in the kernel on its behalf) and the ticks it has run and not yet been charged.
    cpu: Harts<BudgetRef, MAX_PROCESS_COUNT, MAX_HARTS>,
    /// Each hart's billing, by boot index.
    harts: [Billing; MAX_HARTS],
    /// The processes whose ready threads changed since the last reconcile, and what each was
    /// counted as: a reconcile visits only the budgets they moved.
    marks: Marks<BudgetRef, MAX_PROCESS_COUNT>,
}

/// One hart's side of the accounting at the trap boundary.
#[derive(Clone, Copy)]
struct Billing {
    /// When the hart's `cur` last went to user mode, in ticks.
    user_since: Option<u64>,
    /// Kernel time since this tick is billed to this payer.
    billing: Option<(u64, Payer)>,
    /// Billing paused while `kmain` expires deadlines (the walk is nobody's), to resume after.
    paused: Option<Payer>,
    /// Kernel time owed by the budget `kmain` picks next ([`Payer::Next`]), not yet charged.
    owed: u64,
}

/// Who kernel time is billed to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Payer {
    Budget(BudgetRef),
    /// The budget `kmain` picks next, once it is known: the pick and switch into it.
    Next,
}

static SCHED: KernelCell<Sched> = KernelCell::new(Sched {
    cpu: Harts::new(),
    harts: [Billing { user_since: None, billing: None, paused: None, owed: 0 }; MAX_HARTS],
    marks: Marks::new(BudgetRef { frame: 0, id: 0 }),
});

/// This hart's boot index: its runner in the queue's wiring, and its [`Billing`].
fn here() -> usize { crate::arch::hart::index() }

fn ticks() -> u64 { crate::arch::irq::timer::now_ticks() }

impl Budgets<BudgetRef> for MemoryManager {
    fn state(&self, b: BudgetRef) -> State { self.sched_state(b.frame) }

    fn set_state(&mut self, b: BudgetRef, s: State) {
        #[cfg(feature = "sched-inject-tie-fault")]
        let s = trace::tie_fault(s);
        // A pass that changes while the budget is queued (one that is not ranks nowhere; its
        // pass is recorded when it wakes).
        #[cfg(feature = "sched-trace")]
        if s.queued && self.sched_state(b.frame).pass != s.pass {
            trace::record(trace::PASS, b.id, s.pass);
        }
        self.set_sched_state(b.frame, &s)
    }

    fn id(&self, b: BudgetRef) -> u64 { b.id }

    /// Stride weight is free weight: what the budget has not carved to children (R7).
    fn weight(&self, b: BudgetRef) -> u64 { self.free_weight_of(b.frame) }

    fn live(&self, b: BudgetRef) -> bool { self.is_live_budget(b) }

    #[cfg(feature = "sched-trace")]
    fn woke(&mut self, b: BudgetRef) { trace::record(trace::WAKE, b.id, self.sched_state(b.frame).pass) }

    #[cfg(feature = "sched-trace")]
    fn requeued(&mut self, b: BudgetRef) {
        trace::record(trace::REQUEUE, b.id, self.sched_state(b.frame).pass)
    }

    #[cfg(feature = "sched-trace")]
    fn left(&mut self, b: BudgetRef) { trace::record(trace::LEFT, b.id, self.sched_state(b.frame).pass) }

    #[cfg(feature = "sched-trace")]
    fn lifted(&mut self, parent: BudgetRef, child: BudgetRef, l: &redoubt_stride::Lift) {
        trace::lift(parent.id, child.id, l)
    }

    #[cfg(feature = "sched-trace")]
    fn reweighed(&mut self, b: BudgetRef, r: &redoubt_stride::Reweigh) { trace::reweigh(b.id, r) }
}

impl Ready<BudgetRef> for MemoryManager {
    fn ready(&self, b: BudgetRef) -> u32 { self.sched_ready(b.frame) }

    fn set_ready(&mut self, b: BudgetRef, n: u32) { self.set_sched_ready(b.frame, n) }
}

fn budget_ref(mm: &MemoryManager, frame: BudgetFrame) -> BudgetRef {
    BudgetRef { frame, id: mm.budget_id(frame) }
}

impl Sched {
    /// This hart's billing.
    fn b(&mut self) -> &mut Billing { &mut self.harts[here()] }

    /// Close the kernel-time billing interval at `now`: `cur`'s joins its pending runtime, anyone
    /// else's is charged at once, and the next budget's is owed until it is picked.
    fn close_billing(&mut self, mm: &mut MemoryManager, now: u64) {
        match self.b().billing.take() {
            Some((since, Payer::Budget(b))) => self.bill(mm, b, now.saturating_sub(since)),
            Some((since, Payer::Next)) => self.b().owed += now.saturating_sub(since),
            None => {}
        }
    }

    /// Charge `ticks` of kernel time to `b` (`Harts::bill`), and say so in the trace.
    fn bill(&mut self, mm: &mut MemoryManager, b: BudgetRef, ticks: u64) {
        #[cfg(feature = "sched-trace")]
        trace::charge(b.id, ticks);
        self.cpu.bill(mm, b, ticks);
    }

    /// If kernel time is being billed to `b`, close the interval and reopen it: what `b` is
    /// worth is about to change (a weight change, a creation under it, its destruction).
    fn settle_billing(&mut self, mm: &mut MemoryManager, b: BudgetRef) {
        if self.b().billing.is_some_and(|(_, payer)| payer == Payer::Budget(b)) {
            let now = ticks();
            self.close_billing(mm, now);
            self.b().billing = Some((now, Payer::Budget(b)));
        }
    }

    /// The marked processes' counts move to their budgets, so whether a budget has a ready thread
    /// is one read ([`Ready`]); before a deschedule asks it, and before the reconcile.
    fn settle(&mut self, ss: &ProcessTable, mm: &mut MemoryManager) {
        self.marks.settle(mm, |mm, i| ready_now(ss, mm, i));
    }

    /// The end of a kernel entry: the queue takes in the budgets the settle moved.
    fn reconcile(&mut self, mm: &mut MemoryManager) {
        #[cfg(feature = "sched-trace")]
        trace::entry();
        let (lost, gained) = self.marks.changed();
        self.cpu.reconcile(mm, lost, gained, |mm, b| mm.ready(b) > 0);
        #[cfg(debug_assertions)]
        {
            let cpu = &self.cpu;
            if let Err(e) = self.marks.check_visited(mm, |b| cpu.running(b)) {
                panic!("the scheduler's marks: {:?}", e);
            }
        }
        self.marks.clear();
    }
}

/// Process `pid`'s ready threads changed (`ptable.rs`, at every change of its state that changes
/// them): its budget is visited at the next reconcile.
pub fn mark(pid: Pid) { SCHED.with(|s| s.marks.mark(usize::from(pid.get()) - 1)) }

/// Process slot `i`'s ready threads and its budget now: what the marks keep each slot counted as.
fn ready_now(ss: &ProcessTable, mm: &MemoryManager, i: usize) -> (u32, Option<BudgetRef>) {
    let p = &ss.processes[i];
    if p.free() {
        return (0, None);
    }
    (p.ready_count(), mm.budget_of(p.pid()).map(|f| budget_ref(mm, f)))
}

/// The checked build's full walk, at most once a slice after a reconcile that visited a budget,
/// and before the hart idles ([`Marks::audit_due`]): every process's ready threads are counted as a walk of
/// the process table finds them, each budget's count is the sum of its processes', and the queue holds
/// exactly the budgets with a ready thread, and the running one if it is queued.
#[cfg(debug_assertions)]
fn audit_marks(ss: &ProcessTable, mm: &MemoryManager) {
    audit(AUDIT_MARKS, || {
        SCHED.with(|s| {
            // A process with no account is in no budget, so has nothing counted.
            let live = mm.live_pids().map(|pid| usize::from(pid.get()) - 1);
            let Sched { cpu, marks, .. } = s;
            if let Err(e) = marks.audit(mm, &cpu.q, |b| cpu.running(b), live, |mm, i| ready_now(ss, mm, i)) {
                panic!("the scheduler's marks: {:?}", e);
            }
            // The ranks the queue keeps beside its slots against the frames, the authority.
            if let Err(b) = cpu.q.audit(mm) {
                panic!("the scheduler's ranks: budget {} is out of step with its frame", b.id);
            }
        })
    });
}

/// A trap from user mode: the user time since the last return is `cur`'s.
pub fn from_user() {
    let now = ticks();
    SCHED.with(|s| {
        if let Some(since) = s.b().user_since.take() {
            s.cpu.accrue(here(), now.saturating_sub(since));
        }
    });
}

/// The entry's expiry is done: from here, kernel time is `cur`'s (a system call's is its
/// caller's).
pub fn begin_billing() { bill_from_now(SCHED.with(|s| s.cpu.cur(here()))); }

/// From here, kernel time is `payer`'s (nobody's for `None`): the rest of an entry whose expiry
/// billed `payer` last (`time::Expired`). Kernel time being billed is charged first: the expiry's
/// tail is its last budget's until here ([`bill_from`]).
pub fn bill_from_now(payer: Option<BudgetRef>) { bill_from(payer, ticks()) }

/// From tick `since`, kernel time is `payer`'s (nobody's for `None`), once what was billed until
/// then is charged: the expiry's last bill opens the rest of the entry for the budget it billed
/// (`time::expire_due`), so its own handling is not left to nobody.
pub fn bill_from(payer: Option<BudgetRef>, since: u64) {
    MemoryManager::with_mut(|mm| {
        SCHED.with(|s| {
            s.close_billing(mm, since);
            s.b().billing = payer.map(|b| (since, Payer::Budget(b)));
        })
    });
}

/// Kernel time since billing began goes to nobody: it was spent for someone else (an interrupt
/// handled for its device's owner, [`bill`]).
pub fn restart_billing() {
    let now = ticks();
    SCHED.with(|s| {
        if let Some((_, b)) = s.b().billing {
            s.b().billing = Some((now, b));
        }
    });
}

/// `kmain` is about to expire deadlines: its walk is nobody's work, and each expired item bills
/// its own budget (`time.rs`). Billing resumes after ([`resume_billing`]).
pub fn pause_billing() {
    let now = ticks();
    MemoryManager::with_mut(|mm| {
        SCHED.with(|s| {
            s.b().paused = s.b().billing.map(|(_, p)| p);
            s.close_billing(mm, now);
        })
    });
}

/// `kmain`'s expiry is done: what it left billed (its last bill's handling, [`bill_from`]) is
/// charged, and billing resumes for whom it was paused.
pub fn resume_billing() {
    let now = ticks();
    MemoryManager::with_mut(|mm| {
        SCHED.with(|s| {
            s.close_billing(mm, now);
            s.b().billing = s.b().paused.take().map(|b| (now, b));
        })
    });
}

/// `kmain` idles: nobody's work, and so is a pick that found nothing: what the next budget owed
/// goes with it.
pub fn stop_billing() {
    let now = ticks();
    MemoryManager::with_mut(|mm| {
        SCHED.with(|s| {
            s.close_billing(mm, now);
            s.b().owed = 0;
        })
    });
}

/// Charge `ticks` of kernel work done for `b` (an expired timeout of one of its threads, its
/// destruction, an interrupt of its device) to it.
pub fn bill(mm: &mut MemoryManager, b: BudgetRef, ticks: u64) { SCHED.with(|s| s.bill(mm, b, ticks)); }

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

/// The checked build's audit after a destruction's `Y` (`budget::destroy_subtree`), or a
/// deadline's expiry (`time::expire_due`).
#[cfg(debug_assertions)]
pub const AUDIT_DESTRUCTION: u64 = 1;
/// The PID index's audit at a process object's change (`MemoryManager::index_process`).
#[cfg(debug_assertions)]
pub const AUDIT_PROCESS_INDEX: u64 = 2;
/// The IPC lists' audit at the end of an entry that changed one (`message::audit`).
#[cfg(debug_assertions)]
pub const AUDIT_IPC_LISTS: u64 = 3;
/// The scheduler's marks' audit after a reconcile ([`audit_marks`]).
#[cfg(debug_assertions)]
pub const AUDIT_MARKS: u64 = 4;

/// A checked build runs the audit `which`: `check`, which a release build does not have. Its time
/// is charged to no budget, and the running slice's end and the start of the kernel time being
/// billed both move forward by its length, so the thread that ran it is picked and preempted as
/// in a release build. A traced build also stamps it, so the latency targets leave it out of
/// every window (kernel/scheduling.md, "Responsiveness").
#[cfg(debug_assertions)]
pub fn audit(which: u64, check: impl FnOnce()) {
    #[cfg(feature = "sched-trace")]
    let _stamp = trace::audit(which);
    #[cfg(not(feature = "sched-trace"))]
    let _ = which;
    // Debug only, never in a bench build but one recorded negative run (feature `audit-billed`):
    // the audit's time stays billed to the budget that ran it and counts against its slice.
    #[cfg(not(feature = "audit-billed"))]
    let started = ticks();
    check();
    #[cfg(not(feature = "audit-billed"))]
    {
        let ended = ticks();
        SCHED.with(|s| {
            if let Some((since, b)) = s.b().billing {
                s.b().billing = Some((since.saturating_add(ended.saturating_sub(started)), b));
            }
        });
        let length =
            crate::arch::irq::timer::ticks_to_us(ended) - crate::arch::irq::timer::ticks_to_us(started);
        crate::time::set_slice_end(crate::time::slice_end().saturating_add(length));
    }
}

/// Leaving the kernel for `pid` (the kernel itself for PID 1): close the billing, deschedule the
/// budget that ran if another runs now, reconcile, start a picked thread's slice, and start counting
/// user time. Leaving for user mode, the entry's payer pays for this too, up to the return.
pub fn leave(pid: Pid) {
    let now = ticks();
    let to_user = ProcessTable::with(|ss| {
        MemoryManager::with_mut(|mm| {
            SCHED.with(|s| {
                let payer = s.b().billing.map(|(_, p)| p);
                s.close_billing(mm, now);
                let next = if pid.get() == 1 { None } else { mm.budget_of(pid).map(|f| budget_ref(mm, f)) };
                #[cfg(feature = "walk-trace")]
                let _walk = trace::walk(trace::RECONCILE);
                s.settle(ss, mm);
                if next != s.cpu.cur(here()) {
                    s.cpu.switch(here(), mm, next, |mm, b| mm.ready(b) > 0);
                    s.b().user_since = None;
                    // The budget picked pays what it owes for getting here (below).
                    if let Some(b) = next {
                        let owed = core::mem::take(&mut s.b().owed);
                        if owed > 0 {
                            s.bill(mm, b, owed);
                        }
                    }
                }
                // Debug only, never in a bench build but one recorded negative run (feature
                // `timer-tail-billed`): user time starts here, so the rest is the next budget's.
                if cfg!(feature = "timer-tail-billed") {
                    s.b().user_since = next.map(|_| now);
                } else {
                    s.b().user_since = None;
                    // Billing closed above, so the deschedule folds what `cur` ran; it reopens for
                    // the entry's payer, who pays for the rest, and closes again at the return.
                    if let Some(b) = next {
                        let payer = payer.map(|p| if p == Payer::Next { Payer::Budget(b) } else { p });
                        s.b().billing = payer.map(|p| (now, p));
                    } else {
                        // Back to `kmain`: its pick and switch are paid by the budget it picks,
                        // whatever ended the run before; idle time and a pick of nothing are nobody's.
                        s.b().billing = Some((now, Payer::Next));
                    }
                }
                s.reconcile(mm);
                // A budget no hart runs is waiting: an idle hart picks it (the one this hart leaves
                // for `kmain` to pick does not count).
                let waiting = s.cpu.q.queued().filter(|b| !s.cpu.running(*b)).count();
                if waiting > usize::from(next.is_none()) {
                    crate::arch::hart::wake_idle();
                }
                next.is_some()
            })
        })
    });
    #[cfg(debug_assertions)]
    if SCHED.with(|s| s.marks.audit_due(crate::time::now_us(), SLICE_US, false)) {
        ProcessTable::with(|ss| MemoryManager::with(|mm| audit_marks(ss, mm)));
    }
    if pid.get() == 1 {
        crate::time::set_slice_end(crate::time::NEVER);
    } else if to_user && crate::time::slice_end() == crate::time::NEVER {
        // A picked thread's slice starts here, at its return to user mode: the slice is user time,
        // so the exit work since the pick never uses it up; only a deadline due now preempts (R12).
        crate::time::set_slice_end(crate::time::now_us().saturating_add(SLICE_US));
    }
    crate::time::rearm();
    if to_user && !cfg!(feature = "timer-tail-billed") {
        let back = ticks();
        MemoryManager::with_mut(|mm| {
            SCHED.with(|s| {
                s.close_billing(mm, back);
                s.b().user_since = Some(back);
            })
        });
    }
    #[cfg(feature = "sched-trace")]
    trace::returned(to_user);
}

/// What `kmain` runs next: the lowest-ranked queued budget's next thread after its cursor. Its
/// slice starts when it returns to user mode ([`leave`]). `None` when nothing is runnable.
pub fn pick(ss: &ProcessTable, mm: &mut MemoryManager) -> Option<(Pid, TID)> {
    SCHED.with(|s| {
        #[cfg(feature = "walk-trace")]
        let _walk = trace::walk(trace::RECONCILE);
        s.settle(ss, mm);
        s.reconcile(mm);
    });
    let chosen = SCHED.with(|s| s.cpu.pick(here(), mm, |mm, b| next_thread(ss, mm, b)));
    #[cfg(debug_assertions)]
    if SCHED.with(|s| s.marks.audit_due(crate::time::now_us(), SLICE_US, chosen.is_none())) {
        audit_marks(ss, mm);
    }
    let (b, (pid, tid)) = chosen?;
    #[cfg(feature = "sched-trace")]
    trace::record(trace::PICK, b.id, mm.sched_state(b.frame).pass);
    let mut x = mm.budget(b.frame);
    x.cursor = Some((pid, tid));
    mm.store(b.frame, &x);
    Some((pid, tid))
}

/// The next runnable thread of budget `b` after its cursor, in (pid, tid) order, wrapping; a tid
/// of 0 leaves the choice to `activate_process_thread` (a process being set up or handling an
/// exception).
fn next_thread(ss: &ProcessTable, mm: &MemoryManager, b: BudgetRef) -> Option<(Pid, TID)> {
    let cursor = mm.budget(b.frame).cursor;
    let mut first: Option<(Pid, TID)> = None;
    let mut after: Option<(Pid, TID)> = None;
    for pid in mm.live_pids() {
        let p = &ss.processes[usize::from(pid.get()) - 1];
        if p.free() || p.running() || mm.budget_of(pid) != Some(b.frame) {
            continue;
        }
        let tids = p.ready_threads().unwrap_or(TidMask::of(0));
        for tid in tids.iter() {
            let key = (p.pid(), tid);
            first.get_or_insert((p.pid(), tid));
            if after.is_none() && cursor.is_some_and(|c| key > c) {
                after = Some((p.pid(), tid));
            }
        }
    }
    after.or(first)
}

/// A budget is created under `parent`: a running parent is charged first, then the child enters
/// at `max(floor, parent's pass)`. The carve ([`change_weight`]) follows.
pub fn create(mm: &mut MemoryManager, child: BudgetFrame, parent: Option<BudgetFrame>) {
    let child = budget_ref(mm, child);
    let parent = parent.map(|p| budget_ref(mm, p));
    SCHED.with(|s| {
        if let Some(p) = parent {
            s.settle_billing(mm, p);
        }
        s.cpu.create(mm, child, parent);
    });
}

/// `b`'s stride weight changes by `change` (a carve, or a carve returned): what it ran is charged
/// at the old weight first, then its lead and remainder are converted to the new weight
/// (kernel/scheduling.md, "The lead follows the weight").
pub fn change_weight(mm: &mut MemoryManager, b: BudgetFrame, change: impl FnOnce(&mut MemoryManager)) {
    let b = budget_ref(mm, b);
    SCHED.with(|s| {
        s.settle_billing(mm, b);
        s.cpu.change_weight(mm, b, change);
    });
}

/// `frame`, dying, is being destroyed (its descendants already were, bottom-up): what it ran is
/// charged, its carve returns to its parent (unless it came back already: the top's does at mark
/// time), its work since entry moves there (added to the parent's lead, normalized by the
/// parent's weight now), and it leaves the queue. Its frame is freed after this.
pub fn destroy(mm: &mut MemoryManager, frame: BudgetFrame, weight_returned: bool) {
    let child = budget_ref(mm, frame);
    let parent_frame = mm.budget(frame).parent;
    let parent = parent_frame.map(|p| budget_ref(mm, p));
    let limit = mm.budget(frame).weight_limit;
    SCHED.with(|s| {
        s.settle_billing(mm, child);
        for h in s.harts.iter_mut().filter(|h| h.billing.is_some_and(|(_, p)| p == Payer::Budget(child))) {
            h.billing = None;
        }
        if let Some(p) = parent {
            s.settle_billing(mm, p);
        }
        // Whichever hart runs it counts no more user time for it.
        for (i, h) in s.harts.iter_mut().enumerate() {
            if s.cpu.cur(i) == Some(child) {
                h.user_since = None;
            }
        }
        s.cpu.destroy(mm, child, parent, |mm| {
            if let (false, Some(p)) = (weight_returned, parent_frame) {
                let mut pb = mm.budget(p);
                pb.weight_carved = pb.weight_carved.checked_sub(limit).expect("I5: carve underflow");
                mm.store(p, &pb);
            }
        });
    });
}

/// Whether the running thread's slice is over (`time.rs` arms for its end).
pub fn slice_over() -> bool { crate::time::slice_end() <= crate::time::now_us() }

/// The running thread is preempted (its slice ended, or a budget deadline fired): it stays ready,
/// and the CPU goes to `kmain`, which picks again. Its budget is descheduled as the kernel leaves
/// for `kmain` ([`leave`]).
pub fn preempt(ss: &mut ProcessTable, tid: TID) {
    ss.activate_process_thread(tid, KERNEL_PID, 0, true).expect("the kernel can always run");
}

/// `a0` of `kmain`'s switch: outside the call table (redoubt-sys numbers run from `NUMBER_BASE`
/// + 1), so a user-mode `ecall` with it is refused as an unknown number.
const SWITCH_TAG: usize = 0x5357_4954;
/// The switch's answer in `kmain`'s `a0`: the thread ran, and the CPU is back with `kmain`.
const RAN: u64 = 0;
/// The switch's answer in `kmain`'s `a0`: the thread cannot be switched to, and nothing ran.
const NOT_RUNNABLE: u64 = 1;

/// The picked thread cannot run: it died since it was picked.
pub struct NotRunnable;

/// `kmain` runs `(pid, tid)` until the CPU comes back to it (the thread blocked, exited or was
/// preempted). The `ecall`'s trap is [`switch`].
pub fn switch_to(pid: Pid, tid: TID) -> Result<(), NotRunnable> {
    match crate::arch::syscall::switch_trap(SWITCH_TAG, pid.get() as usize, tid) as u64 {
        RAN => Ok(()),
        _ => Err(NotRunnable),
    }
}

/// The trap of [`switch_to`] (an `ecall` from S-mode): make `(pid, tid)` current, or leave
/// `kmain` current with [`NOT_RUNNABLE`]. Only `kmain` switches, and only with [`SWITCH_TAG`];
/// anything else is a kernel bug. The caller resumes whatever is current.
pub fn switch(ss: &mut ProcessTable, tag: usize, pid: usize, tid: TID) {
    assert!(
        ss.current_pid() == KERNEL_PID && tag == SWITCH_TAG,
        "an S-mode ecall that is not kmain's switch: pid {}, a0 {:#x}",
        ss.current_pid(),
        tag
    );
    let pid = crate::budget::pid_from(pid).expect("kmain switches to a process's PID");
    let kmain = ArchProcess::with_current(|p| p.current_tid());
    // `kmain` reads `a0` when it next runs: once the CPU comes back to it.
    ss.set_redoubt_result(KERNEL_PID, kmain, &[RAN, 0, 0, 0, 0, 0, 0, 0]).expect("kmain exists");
    if ss.activate_process_thread(kmain, pid, tid, true).is_err() {
        ss.set_redoubt_result(KERNEL_PID, kmain, &[NOT_RUNNABLE, 0, 0, 0, 0, 0, 0, 0]).expect("kmain exists");
    } else {
        // Another hart may have written code this one is about to fetch (an image moved in, a
        // page made executable): fence before running a process (kernel/memory.md).
        crate::arch::mem::sync_icache();
    }
}

/// The queue's raw events, for the bench's independent rank oracle (`tools/testbench`,
/// `sched_oracle`), in test builds only (feature `sched-trace`; a default build compiles none of
/// it: a per-pick record of every budget is a cross-principal channel no production kernel may
/// have). A bounded ring in frames the kernel takes for itself at boot, before the budget tree
/// counts what is left ([`trace::init`]), printed at `system_reset`. It records what the queue
/// did, never why: no tie key. Each record: its sequence number, the kernel entry (reconcile) it
/// belongs to, the event, the budget's id, and the low 64 bits of its pass after the event (a pass
/// reaches 2^64 only after centuries of slices).
#[cfg(feature = "sched-trace")]
pub mod trace {
    use crate::cell::KernelCell;
    use crate::mem::MemoryManager;

    /// A budget woke into the queue, was requeued behind its equals, left the queue, had its pass
    /// changed, or was picked.
    pub const WAKE: u8 = b'W';
    pub const REQUEUE: u8 = b'R';
    pub const LEFT: u8 = b'D';
    pub const PASS: u8 = b'P';
    pub const PICK: u8 = b'K';
    /// A destruction (R10) began and ended: the top's id, and the time in µs in the pass field.
    pub const R10_BEGIN: u8 = b'X';
    pub const R10_END: u8 = b'Y';

    /// Frames the ring takes (64 MiB, 192 MiB with `sched-trace-large`), and the records they hold.
    const PAGES: usize = if cfg!(feature = "sched-trace-large") { 49152 } else { 16384 };
    const PER_PAGE: usize = redoubt_sys::PAGE_SIZE / 32;
    const CAP: usize = PAGES * PER_PAGE;

    struct Ring {
        pages: [usize; PAGES],
        n: usize,
        dropped: u64,
        entry: u64,
        /// Inside a timer interrupt from user mode, until it returns: its charges are recorded.
        timer: bool,
        /// The walk measured now (`walk-trace`), 0 for none.
        walk: u64,
    }

    static RING: KernelCell<Ring> =
        KernelCell::new(Ring { pages: [0; PAGES], n: 0, dropped: 0, entry: 0, timer: false, walk: 0 });

    /// Take the ring's frames, zeroed (at boot, before `boot_budgets` counts what the kernel
    /// keeps).
    pub fn init(mm: &mut MemoryManager) {
        RING.with(|r| {
            for page in r.pages.iter_mut() {
                *page = mm.kernel_frame().expect("sched-trace: no RAM for the trace ring");
                crate::kframe::zero(*page);
            }
        });
    }

    /// A reconcile begins: the records that follow belong to a new kernel entry.
    pub fn entry() { RING.with(|r| r.entry += 1); }

    /// Record an event. The ring has one writer at a time, the hart holding the kernel lock; each
    /// record's kind word carries that hart's boot index above the kind's byte (0 on one hart, so
    /// a one-hart trace is as it was).
    pub fn record(kind: u8, id: u64, pass: u128) {
        let kind = u64::from(kind) | (crate::arch::hart::index() as u64) << 8;
        RING.with(|r| {
            if r.n < CAP && r.pages[0] != 0 {
                let (page, at) = (r.pages[r.n / PER_PAGE], (r.n % PER_PAGE) * 32);
                for (k, word) in [pass as u64, r.entry, id, kind].iter().enumerate() {
                    crate::kframe::write(page, at + k * 8, *word);
                }
                r.n += 1;
            } else {
                r.dropped += 1;
            }
        });
    }

    /// A timer interrupt from user mode began (`I`): the budget it interrupted (id 0 for none; no
    /// object has id 0), and the time in µs in the pass field. Until it returns (`O`: 1 in the pass
    /// field to user mode, 0 to `kmain`), each charge is recorded (`B`: the payer and the ticks),
    /// and the end of its expiry (`E`: the budget billed last, and in the pass field 1 for an
    /// expired item's, 2 for a wait's that ended before its timeout, 0 for none). The oracle
    /// checks that the budget it interrupted pays only for its own items, or for its slice's end
    /// (kernel/scheduling.md, "Charging").
    pub const TIMER_ENTRY: u8 = b'I';
    pub const CHARGE: u8 = b'B';
    pub const EXPIRED: u8 = b'E';
    pub const RETURN: u8 = b'O';

    /// A timer interrupt from user mode began (after its user time was accrued).
    pub fn timer_entry() {
        let cur = super::SCHED.with(|s| s.cpu.cur(super::here()));
        RING.with(|r| r.timer = true);
        record(TIMER_ENTRY, cur.map_or(0, |b| b.id), u128::from(crate::time::now_us()));
    }

    /// `ticks` of kernel time were charged to budget `id`.
    pub fn charge(id: u64, ticks: u64) {
        if RING.with(|r| r.timer) {
            record(CHARGE, id, u128::from(ticks));
        }
    }

    /// The entry's expiry is done; `last` was billed last, for a wait that ended before its
    /// timeout if `stale`.
    pub fn expired(last: Option<crate::handle::BudgetRef>, stale: bool) {
        if RING.with(|r| r.timer) {
            let what = match (last, stale) {
                (None, _) => 0,
                (Some(_), false) => 1,
                (Some(_), true) => 2,
            };
            record(EXPIRED, last.map_or(0, |b| b.id), what);
        }
    }

    /// The kernel returns, to user mode or to `kmain`.
    pub fn returned(to_user: bool) {
        if RING.with(|r| core::mem::take(&mut r.timer)) {
            record(RETURN, 0, u128::from(to_user));
        }
    }

    /// A destruction begins or ends (`budget::destroy_subtree`).
    pub fn r10(kind: u8, top: u64) { record(kind, top, u128::from(crate::time::now_us())); }

    /// The object frames a destruction walks (after its `X`).
    pub const R10_FRAMES: u8 = b'Z';

    /// A checked build's audit began and ended: which audit, and the time in µs in the pass field.
    /// A release build runs none, so the bench subtracts their time from every window a latency
    /// target judges (kernel/scheduling.md, "Responsiveness").
    pub const AUDIT_BEGIN: u8 = b'U';
    pub const AUDIT_END: u8 = b'V';

    /// An audit running: its begin is recorded, and its end when this drops (none if `None`).
    pub struct Audit(Option<u64>);

    /// Stamp the audit `which` that is about to run, until the returned guard drops
    /// ([`super::audit`]).
    pub fn audit(which: u64) -> Audit {
        // Debug only, never in a bench build but one recorded negative run (feature
        // `audit-unstamped`): the audit after a destruction runs unstamped, to show that the
        // oracle subtracts only what the trace shows it.
        #[cfg(feature = "audit-unstamped")]
        if which == super::AUDIT_DESTRUCTION {
            return Audit(None);
        }
        record(AUDIT_BEGIN, which, u128::from(crate::time::now_us()));
        Audit(Some(which))
    }

    impl Drop for Audit {
        fn drop(&mut self) {
            if let Some(which) = self.0 {
                record(AUDIT_END, which, u128::from(crate::time::now_us()));
            }
        }
    }

    /// A process's threads began and finished ending, the pumps after them included
    /// (`message::process_ending`): the time in µs in the pass field. The bench reports each
    /// destruction's time in them beside R10's (kernel/budgets.md, "Residual risks").
    pub const THREADS_BEGIN: u8 = b'T';
    pub const THREADS_END: u8 = b't';

    /// The threads span running: its begin is recorded, and its end when this drops.
    pub struct Threads;

    /// Stamp a process's threads ending, until the returned guard drops.
    pub fn threads() -> Threads {
        record(THREADS_BEGIN, 0, u128::from(crate::time::now_us()));
        Threads
    }

    impl Drop for Threads {
        fn drop(&mut self) { record(THREADS_END, 0, u128::from(crate::time::now_us())); }
    }

    /// A walk began and ended (`walk-trace`): which walk, and the time in µs in the pass field.
    /// The worst-walk case reads them (kernel/ipc.md and kernel/timer.md, "Residual risks").
    pub const WALK_BEGIN: u8 = b'M';
    pub const WALK_END: u8 = b'm';
    /// The walks: a receive's pump (`message::pump`), a timer interrupt's expiry
    /// (`time::expire_due`), and the end of a kernel entry (`reconcile`, its wakes included).
    pub const PUMP: u64 = 1;
    pub const EXPIRY: u64 = 2;
    pub const RECONCILE: u64 = 3;

    /// A walk running: its begin is recorded, and its end when this drops. A walk inside another
    /// is the outer one's (none recorded).
    pub struct Walk(Option<u64>);

    /// Stamp the walk `which` that is about to run, until the returned guard drops.
    pub fn walk(which: u64) -> Walk {
        let inside = RING.with(|r| {
            let open = r.walk != 0;
            if !open {
                r.walk = which;
            }
            open
        });
        if inside {
            return Walk(None);
        }
        record(WALK_BEGIN, which, u128::from(crate::time::now_us()));
        Walk(Some(which))
    }

    impl Drop for Walk {
        fn drop(&mut self) {
            if let Some(which) = self.0 {
                record(WALK_END, which, u128::from(crate::time::now_us()));
                RING.with(|r| r.walk = 0);
            }
        }
    }

    /// A destroyed child's work moved to its parent: every operand of the rule and its result,
    /// as a group of nine records the oracle recomputes (`L` parent pass before, `l` child pass,
    /// `e` child entry, `f` floor, `r` child remainder, `q` parent remainder before, `w` the child's
    /// and the parent's weights (high and low 32 bits), `A` parent pass after, `a` parent
    /// remainder after).
    pub fn lift(parent: u64, child: u64, l: &redoubt_stride::Lift) {
        let weights = (l.w_child << 32 | l.w_parent & 0xffff_ffff) as u128;
        for (kind, id, value) in [
            (b'L', parent, l.parent.pass),
            (b'l', child, l.child.pass),
            (b'e', child, l.child.entry),
            (b'f', 0, l.floor),
            (b'r', child, u128::from(l.child.rem)),
            (b'q', parent, u128::from(l.parent.rem)),
            (b'w', 0, weights),
            (b'A', parent, l.after.pass),
            (b'a', parent, u128::from(l.after.rem)),
        ] {
            record(kind, id, value);
        }
    }

    /// A budget's weight changed and its lead and remainder were converted: every operand and
    /// the result, as a group of six records the oracle recomputes, ahead of the pass it may
    /// lower (`G` pass before, `g` remainder before, `v` the old and the new weight (high and low
    /// 32 bits), `f` floor, `N` pass after, `n` remainder after).
    pub fn reweigh(b: u64, r: &redoubt_stride::Reweigh) {
        let weights = (r.old << 32 | r.new & 0xffff_ffff) as u128;
        for (kind, value) in [
            (b'G', r.before.pass),
            (b'g', u128::from(r.before.rem)),
            (b'v', weights),
            (b'f', r.floor),
            (b'N', r.after.pass),
            (b'n', u128::from(r.after.rem)),
        ] {
            record(kind, b, value);
        }
    }

    /// Debug only, never in a bench build but one recorded negative run (feature
    /// `sched-inject-tie-fault`): wakers rank behind queued budgets of equal pass, and behind each
    /// other in reverse (the model's `R12TieQueuedFirst`), to show the oracle catches it.
    #[cfg(feature = "sched-inject-tie-fault")]
    pub fn tie_fault(mut s: redoubt_stride::State) -> redoubt_stride::State {
        if s.tie < 0 {
            s.tie = i64::MAX / 2 - s.tie;
        }
        s
    }

    /// Print the ring (at `system_reset`, before the machine goes). A drop fails the oracle.
    pub fn dump() {
        RING.with(|r| {
            for seq in 0..r.n {
                let (page, at) = (r.pages[seq / PER_PAGE], (seq % PER_PAGE) * 32);
                let w = |k: usize| crate::kframe::read(page, at + k * 8);
                println!("SCHED-TRACE {} {} {} {} {:x}", seq, w(1), w(3) as u8 as char, w(2), w(0));
            }
            println!("SCHED-TRACE-END {} dropped {}", r.n, r.dropped);
        });
    }
}
