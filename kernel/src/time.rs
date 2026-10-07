// SPDX-License-Identifier: MIT OR Apache-2.0

//! The kernel-owned timer (kernel/timer.md; I13 and R12).
//!
//! One hardware timer, always armed for the earliest of what is due: the running thread's slice
//! end (R12), a blocking call's timeout (I13) or a budget's deadline. Nothing in userspace programs it and
//! there is no timer interrupt for userspace; user mode reads the counter directly (`rdtime`).
//!
//! # Expiry
//! [`expire_due`] handles everything due at or before now, earliest first: a timeout returns
//! `Timeout` (`message.rs`); a deadline destroys its budget (`budget::destroy_subtree`, R10). At an
//! equal instant timeouts come first, then budgets by id; timeouts among themselves in (pid, tid)
//! order. The order matters: a caller whose call its server took, and whose timeout falls with
//! the server budget's deadline, gets `Timeout` with its lend consumed, not `Dead` with it
//! returned.
//!
//! It runs at every kernel entry but `kmain`'s switch (`sched::switch_to`), first, before
//! anything reads the entering process, so a deadline that has passed beats any operation that
//! enters later.
//! `kmain` expires, then picks, then switches, with no kernel entry in between.
//!
//! # Hints
//! Finding what is due means reading the timed waits of each process whose cached earliest
//! timeout has come (`message::collect_due`) and walking the list of budgets with a deadline
//! (`budget.rs`). Both are skipped while the cached earliest deadlines are still in the future. A
//! hint is only ever early: a thread that blocks with a timeout or a new budget lowers it, and a
//! wait that ends before its timeout or a destroyed budget leaves it where it was, which costs at
//! most one early interrupt and a walk that recomputes it. So nothing is missed. A call answered at
//! once, or whose timeout has already passed, never blocks and lowers nothing. The walk that finds
//! an ended wait is its thread's budget's, as is the rest of an entry that found nothing else; a
//! destroyed budget's early walk of the deadline list is nobody's.
//!
//! Microseconds are the ABI's unit (kernel/timer.md, "Time"): deadlines are kept in them, and
//! converted to timer ticks rounding up, so an interrupt never comes before its deadline.

use crate::arch::hart::{self, MAX_HARTS};
use crate::arch::irq::timer;
use crate::cell::KernelCell;
use crate::handle::BudgetRef;
use crate::mem::MemoryManager;
use crate::ptable::ProcessTable;

/// A time that never comes (`FOREVER`).
pub const NEVER: u64 = u64::MAX;

struct Timer {
    /// No thread's timeout is earlier than this.
    threads: u64,
    /// No budget's deadline is earlier than this.
    budgets: u64,
    /// When each hart's running thread's slice ends (`sched.rs`), by boot index; `NEVER` while
    /// `kmain` runs there, and from a pick until the thread returns to user mode.
    slice: [u64; MAX_HARTS],
    /// What each hart's timer is armed for, in microseconds.
    armed: [u64; MAX_HARTS],
}

static TIMER: KernelCell<Timer> = KernelCell::new(Timer {
    threads: NEVER,
    budgets: NEVER,
    slice: [NEVER; MAX_HARTS],
    armed: [NEVER; MAX_HARTS],
});

/// Monotonic microseconds since boot.
pub fn now_us() -> u64 { timer::now_us() }

/// A call blocked until `deadline`: make sure the timer comes by then.
pub fn note_timeout(deadline: u64) {
    TIMER.with(|t| t.threads = t.threads.min(deadline));
    rearm();
}

/// A budget was created with `deadline`: make sure the timer comes by then (at once, for one
/// already past).
pub fn note_budget_deadline(deadline: u64) {
    TIMER.with(|t| t.budgets = t.budgets.min(deadline));
    rearm();
}

/// This hart's running thread's slice ends at `at` (`NEVER` for none).
pub fn set_slice_end(at: u64) {
    TIMER.with(|t| t.slice[hart::index()] = at);
    rearm();
}

/// When this hart's running thread's slice ends.
pub fn slice_end() -> u64 { TIMER.with(|t| t.slice[hart::index()]) }

/// Arm this hart's timer for the earliest thing due, if that changed: its own slice's end, or the
/// earliest timeout or deadline, which every hart's timer comes by.
pub fn rearm() {
    let h = hart::index();
    TIMER.with(|t| {
        let target = t.threads.min(t.budgets).min(t.slice[h]);
        if target != t.armed[h] {
            t.armed[h] = target;
            timer::set(if target == NEVER { u64::MAX } else { timer::us_to_ticks(target) });
        }
    });
}

/// The budget deadline due first at `now`: (deadline, id, frame).
fn due_budget(now: u64) -> Option<(u64, u64, u32)> {
    MemoryManager::with(|mm| mm.deadlines().filter(|(d, _, _)| *d <= now).min())
}

/// The process a destruction happens under: the one running (whose call or run this entry
/// interrupted), if it is not the kernel.
fn running() -> Option<redoubt_layout::Pid> {
    let pid = crate::arch::current_pid();
    (pid.get() != 1).then_some(pid)
}

/// What an expiry did.
pub struct Expired {
    /// A budget deadline fired (a preemption point, R12).
    pub destroyed: bool,
    /// The budget billed for the item expired last, or with none expired, for the wait found to
    /// have ended before its timeout, if any: the rest of the entry is its (kernel/timer.md, "R12
    /// (scheduling) for timer work").
    pub last: Option<BudgetRef>,
}

/// Handle everything due (module docs), then re-arm for the next thing. Takes the scheduler only:
/// destroying a budget borrows the memory manager in phases (`process.rs`, Locks).
///
/// One walk finds every wait due and orders it (`message::collect_due`), so ending R waits costs
/// one walk and R steps, not a walk each. Each item's handling is billed (`sched::bill`): a
/// timeout to its thread's budget, and a deadline, the whole destruction, to the dying budget's
/// parent once its carve is back, or the nearest ancestor with free weight (`destroy_subtree`,
/// R10). The walk and its ordering are billed in equal shares to the waits it found, the
/// remainder to the first, each with its own ending, so a budget pays its share of a shared
/// instant, not a neighbour's (R12); a wait found ended meanwhile is billed its share too. The
/// re-arm, and any share left over (a wait whose thread a deadline's destruction ended), go to
/// the budget billed last. With no wait due, the walk goes with the first deadline, or to the
/// budget of a wait the walk found ended before its timeout (it left the timer early), or with
/// neither to nobody. The intervals are contiguous: each bill closes at a tick that opens the
/// next, so a bill's own handling is in the interval after it, and the last bill opens the budget
/// billed last's billing (`sched::bill_from`), which the entry's next payer closes. No part of
/// the expiry falls between two intervals, to nobody.
pub fn expire_due(ss: &mut ProcessTable) -> Expired {
    let now = now_us();
    if TIMER.with(|t| t.threads > now && t.budgets > now) {
        return Expired { destroyed: false, last: None };
    }
    let mut started = crate::sched::now_ticks();
    // The `EXPIRY` walk is the timer's own: the collect walk and its sort. Each ending, and the
    // pump it makes, is recorded after it, as its own.
    let walk = {
        #[cfg(feature = "walk-trace")]
        let _walk = crate::sched::trace::walk(crate::sched::trace::EXPIRY);
        MemoryManager::with_mut(|mm| crate::message::collect_due(mm, now))
    };
    let (mut pool, share, mut first) = match walk.count {
        0 => (0, 0, 0),
        n => {
            let pool = crate::sched::now_ticks().saturating_sub(started);
            started = crate::sched::now_ticks();
            (pool, pool / n, pool % n)
        }
    };
    let mut destroyed = false;
    let mut expired = false;
    let mut last = None;
    loop {
        let timeout = MemoryManager::with(crate::message::first_due);
        let budget = due_budget(now);
        // Earliest first; at an equal instant, the timeout.
        let timeout_first = match (timeout, budget) {
            (None, None) => break,
            (Some((td, _, _)), Some((bd, _, _))) => td <= bd,
            (t, _) => t.is_some(),
        };
        expired = true;
        if let (true, Some((_, pid, tid))) = (timeout_first, timeout) {
            let billed = MemoryManager::with_mut(|mm| {
                if crate::message::pop_due(mm, pid, tid) {
                    crate::message::time_out(ss, mm, pid, tid);
                }
                let take = (share + core::mem::take(&mut first)).min(pool);
                pool -= take;
                let frame = mm.budget_of(pid)?;
                let b = BudgetRef { frame, id: mm.budget(frame).id };
                let at = crate::sched::now_ticks();
                crate::sched::bill(mm, b, take + at.saturating_sub(started));
                Some((b, at))
            });
            last = billed.map(|(b, _)| b);
            // The next interval opens where this bill closed, so the bill's own handling is in it.
            started = billed.map_or_else(crate::sched::now_ticks, |(_, at)| at);
        } else if let Some((_, _, frame)) = budget {
            // The payer `destroy_subtree` names, asked as it asks: after `mark_dying`.
            last = MemoryManager::with_mut(|mm| {
                mm.mark_dying(frame);
                mm.destruction_payer(frame)
            });
            crate::budget::destroy_subtree(ss, frame, running(), Some(started));
            destroyed = true;
            started = crate::sched::now_ticks();
        }
    }
    // Nothing expired: the walk found a wait that ended before its timeout.
    let stale = !expired && walk.stale.is_some();
    if stale {
        last = MemoryManager::with(|mm| {
            let frame = mm.budget_of(walk.stale?)?;
            Some(BudgetRef { frame, id: mm.budget(frame).id })
        });
    }
    #[cfg(feature = "sched-trace")]
    crate::sched::trace::expired(last, stale);
    let next_budget = MemoryManager::with(|mm| {
        mm.deadlines().map(|(d, _, _)| d).filter(|d| *d > now).min().unwrap_or(NEVER)
    });
    TIMER.with(|t| {
        t.threads = walk.next;
        t.budgets = next_budget;
        // This hart's timer fired (or will, for what just passed); arm afresh.
        t.armed[hart::index()] = 0;
    });
    rearm();
    let at = crate::sched::now_ticks();
    if let Some(b) = last {
        MemoryManager::with_mut(|mm| crate::sched::bill(mm, b, pool + at.saturating_sub(started)));
    }
    // From the last bill on, the rest of the entry is the budget billed last's (none with nothing
    // expired), the bill's own handling included; an audit below is charged to no one.
    crate::sched::bill_from(last, at);
    MemoryManager::with(crate::message::audit);
    // The deadlines' destructions audit once, here: inside the loop, a wait due later than one
    // was still on the due list.
    #[cfg(debug_assertions)]
    if destroyed {
        crate::budget::audit_destruction();
    }
    Expired { destroyed, last }
}

/// A timer interrupt arrived. The trap handler has already expired what is due at its entry;
/// all that is left is to arm for the next thing (a stale early hint lands here too).
pub fn on_interrupt() {
    TIMER.with(|t| t.armed[hart::index()] = 0);
    rearm();
}

/// Expire at a kernel entry.
pub fn expire_at_entry() -> Expired { ProcessTable::with_mut(expire_due) }
