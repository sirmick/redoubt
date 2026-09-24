// SPDX-License-Identifier: MIT OR Apache-2.0

//! The scheduler: one stride queue over every runnable budget (KERNEL-SPEC.md R7, R12; RESOURCES.md,
//! Scheduling; the WP-K5 owner decisions). The rules themselves (charging with an exact remainder,
//! the floor, ranks, inheritance) and their wiring to a CPU (when runtime is folded, what a
//! deschedule, a pick, a weight change and a destruction do, in what order) are `redoubt-stride`'s
//! ([`Cpu`]), checked there against the executable model; this module keeps their state in the
//! budgets' frames and drives them from the trap boundary.
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
//!   and it is requeued behind its equals if it still has a runnable thread. It then reconciles the queue
//!   (one reconcile per kernel entry: budgets that gained a runnable thread wake, those that lost their last
//!   one leave).
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
use redoubt_stride::{Budgets, Cpu, State};

use crate::arch::process::{MAX_PROCESS_COUNT, MAX_THREAD};
use crate::budget::BudgetFrame;
use crate::cell::KernelCell;
use crate::handle::BudgetRef;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// Time slice, in microseconds (KERNEL-SPEC.md, Constants).
pub const SLICE_US: u64 = 10_000;

struct Sched {
    /// The queue, the budget whose runtime is accruing (on the CPU, or in the kernel on its
    /// behalf) and the ticks it has run and not yet been charged.
    cpu: Cpu<BudgetRef, MAX_PROCESS_COUNT>,
    /// When `cur` last went to user mode, in ticks.
    user_since: Option<u64>,
    /// Kernel time since this tick is billed to this budget.
    billing: Option<(u64, BudgetRef)>,
    /// Billing paused while `kmain` expires deadlines (the walk is nobody's), to resume after.
    paused: Option<BudgetRef>,
}

static SCHED: KernelCell<Sched> =
    KernelCell::new(Sched { cpu: Cpu::new(), user_since: None, billing: None, paused: None });

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
}

fn budget_ref(mm: &MemoryManager, frame: BudgetFrame) -> BudgetRef {
    BudgetRef { frame, id: mm.budget_id(frame) }
}

impl Sched {
    /// Close the kernel-time billing interval at `now`: `cur`'s joins its pending runtime, anyone
    /// else's is charged at once.
    fn close_billing(&mut self, mm: &mut MemoryManager, now: u64) {
        if let Some((since, b)) = self.billing.take() {
            self.cpu.bill(mm, b, now.saturating_sub(since));
        }
    }

    /// If kernel time is being billed to `b`, close the interval and reopen it: what `b` is
    /// worth is about to change (a weight change, a creation under it, its destruction).
    fn settle_billing(&mut self, mm: &mut MemoryManager, b: BudgetRef) {
        if self.billing.is_some_and(|(_, payer)| payer == b) {
            let now = ticks();
            self.close_billing(mm, now);
            self.billing = Some((now, b));
        }
    }

    /// Budgets with a thread waiting for the CPU, each once.
    fn runnable(ss: &SystemServices, mm: &MemoryManager) -> ([BudgetRef; MAX_PROCESS_COUNT], usize) {
        let mut out = [BudgetRef { frame: 0, id: 0 }; MAX_PROCESS_COUNT];
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
            if !out[..n].contains(&b) && n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
        (out, n)
    }

    fn reconcile(&mut self, mm: &mut MemoryManager, runnable: &[BudgetRef]) {
        #[cfg(feature = "sched-trace")]
        trace::entry();
        self.cpu.reconcile(mm, runnable);
    }
}

/// A trap from user mode: the user time since the last return is `cur`'s.
pub fn from_user() {
    let now = ticks();
    SCHED.with(|s| {
        if let Some(since) = s.user_since.take() {
            s.cpu.accrue(now.saturating_sub(since));
        }
    });
}

/// The entry's expiry is done: from here, kernel time is `cur`'s (a system call's is its
/// caller's).
pub fn begin_billing() {
    let now = ticks();
    SCHED.with(|s| s.billing = s.cpu.cur.map(|b| (now, b)));
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
pub fn bill(mm: &mut MemoryManager, b: BudgetRef, ticks: u64) { SCHED.with(|s| s.cpu.bill(mm, b, ticks)); }

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
                let (list, n) = Sched::runnable(ss, mm);
                let runnable = &list[..n];
                if next != s.cpu.cur {
                    let left = s.cpu.switch(mm, next, |_, b| runnable.contains(&b));
                    s.user_since = None;
                    // `kmain`'s pick and switch after a deschedule are the descheduled budget's
                    // work (it blocked, exited or was preempted): billed to it, as a deschedule's
                    // cost, until the next budget runs.
                    if next.is_none() {
                        s.billing = left.filter(|b| mm.is_live_budget(*b)).map(|b| (now, b));
                    }
                }
                s.reconcile(mm, runnable);
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
        let (list, n) = Sched::runnable(ss, mm);
        s.reconcile(mm, &list[..n]);
        s.cpu.pick(mm, |mm, b| next_thread(ss, mm, b))
    });
    let (b, (pid, tid)) = chosen?;
    #[cfg(feature = "sched-trace")]
    trace::record(trace::PICK, b.id, mm.sched_state(b.frame).pass);
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
/// at the old weight first, then its remainder is rescaled.
pub fn change_weight(mm: &mut MemoryManager, b: BudgetFrame, change: impl FnOnce(&mut MemoryManager)) {
    let b = budget_ref(mm, b);
    SCHED.with(|s| {
        s.settle_billing(mm, b);
        s.cpu.change_weight(mm, b, change);
    });
}

/// `frame`, dying, is being destroyed (its descendants already were, bottom-up): what it ran is
/// charged, its carve returns to its parent, its work since entry moves there (added to the
/// parent's lead, normalized by the parent's weight now), and it leaves the queue. Its frame is
/// freed after this.
pub fn destroy(mm: &mut MemoryManager, frame: BudgetFrame) {
    let child = budget_ref(mm, frame);
    let parent_frame = mm.budget(frame).parent;
    let parent = parent_frame.map(|p| budget_ref(mm, p));
    let limit = mm.budget(frame).weight_limit;
    SCHED.with(|s| {
        s.settle_billing(mm, child);
        if s.billing.is_some_and(|(_, b)| b == child) {
            s.billing = None;
        }
        if let Some(p) = parent {
            s.settle_billing(mm, p);
        }
        if s.cpu.cur == Some(child) {
            s.user_since = None;
        }
        s.cpu.destroy(mm, child, parent, |mm| {
            if let Some(p) = parent_frame {
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
pub fn preempt(ss: &mut SystemServices, tid: TID) {
    crate::syscall::reset_switchto_caller();
    let kernel = PID::new(1).expect("PID 1");
    ss.activate_process_thread(tid, kernel, 0, true, crate::services::PostActivateOp::None)
        .expect("the kernel can always run");
    crate::syscall::restore_last_thread(ss);
}

/// The queue's raw events, for the bench's independent rank oracle (`tools/testbench`,
/// `sched_oracle`), in test builds only (feature `sched-trace`; a default build compiles none of
/// it: a per-pick record of every budget is a cross-principal channel no production kernel may
/// have). A bounded ring in the kernel's own RAM region (so its records are compact), printed at
/// `system_reset`. It records what the queue did, never why: no tie key. Each record: its
/// sequence number, the kernel entry (reconcile) it belongs to, the event, the budget's id, and
/// the low 64 bits of its pass after the event (a pass reaches 2^64 only after centuries of
/// slices).
#[cfg(feature = "sched-trace")]
pub mod trace {
    use crate::cell::KernelCell;

    /// A budget woke into the queue, was requeued behind its equals, left the queue, had its pass
    /// changed, or was picked.
    pub const WAKE: u8 = b'W';
    pub const REQUEUE: u8 = b'R';
    pub const LEFT: u8 = b'D';
    pub const PASS: u8 = b'P';
    pub const PICK: u8 = b'K';

    /// 160 KiB of the kernel's 512 KiB RAM region, in a test build only.
    const CAP: usize = 10240;

    #[derive(Clone, Copy)]
    struct Rec {
        pass: u64,
        entry: u32,
        /// The budget's id (below 2^24 in any test run) and the event, in its low byte.
        id_kind: u32,
    }

    struct Ring {
        recs: [Rec; CAP],
        n: usize,
        dropped: u64,
        entry: u32,
    }

    static RING: KernelCell<Ring> = KernelCell::new(Ring {
        recs: [Rec { pass: 0, entry: 0, id_kind: 0 }; CAP],
        n: 0,
        dropped: 0,
        entry: 0,
    });

    /// A reconcile begins: the records that follow belong to a new kernel entry.
    pub fn entry() { RING.with(|r| r.entry = r.entry.wrapping_add(1)); }

    pub fn record(kind: u8, id: u64, pass: u128) {
        RING.with(|r| {
            if r.n < CAP {
                let entry = r.entry;
                r.recs[r.n] = Rec { pass: pass as u64, entry, id_kind: (id as u32) << 8 | u32::from(kind) };
                r.n += 1;
            } else {
                r.dropped += 1;
            }
        });
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
            for (seq, rec) in r.recs[..r.n].iter().enumerate() {
                let (id, kind) = (rec.id_kind >> 8, rec.id_kind as u8 as char);
                println!("SCHED-TRACE {} {} {} {} {:x}", seq, rec.entry, kind, id, rec.pass);
            }
            println!("SCHED-TRACE-END {} dropped {}", r.n, r.dropped);
        });
    }
}
