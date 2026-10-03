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
//! Finding what is due means walking every thread (`message.rs`) and the list of budgets with a
//! deadline (`budget.rs`). Both are skipped while the cached earliest deadlines are still in the
//! future. A hint is only ever early: a thread that blocks with a timeout or a new budget lowers
//! it, and a wait that ends before its timeout or a destroyed budget leaves it where it was, which
//! costs at most one early interrupt and a walk that recomputes it. So nothing is missed. A call
//! answered at once, or whose timeout has already passed, never blocks and lowers nothing. The
//! walk that finds an ended wait is its thread's budget's, as is the rest of an entry that found
//! nothing else; a destroyed budget's early walk of the deadline list is nobody's.
//!
//! Microseconds are the ABI's unit (kernel/timer.md, "Time"): deadlines are kept in them, and
//! converted to timer ticks rounding up, so an interrupt never comes before its deadline.

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
    /// When the running thread's slice ends (`sched.rs`); `NEVER` while `kmain` runs.
    slice: u64,
    /// What the hardware is armed for, in microseconds.
    armed: u64,
}

static TIMER: KernelCell<Timer> =
    KernelCell::new(Timer { threads: NEVER, budgets: NEVER, slice: NEVER, armed: NEVER });

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

/// The running thread's slice ends at `at` (`NEVER` for none).
pub fn set_slice_end(at: u64) {
    TIMER.with(|t| t.slice = at);
    rearm();
}

/// When the running thread's slice ends.
pub fn slice_end() -> u64 { TIMER.with(|t| t.slice) }

/// Arm the timer for the earliest thing due, if that changed.
pub fn rearm() {
    TIMER.with(|t| {
        let target = t.threads.min(t.budgets).min(t.slice);
        if target != t.armed {
            t.armed = target;
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
/// Each item's handling is billed (`sched::bill`): a timeout to its thread's budget, and a
/// deadline, the whole destruction, to the dying budget's parent once its carve is back, or the
/// nearest ancestor with free weight (`destroy_subtree`, R10). So is the walk that found it: a
/// budget with many timeouts due at once pays for the walk each one costs. The last walk, which
/// finds nothing more, and the re-arm are billed to the budget billed last. With nothing expired,
/// they are the budget of a wait the walk found ended before its timeout (it left the timer
/// early), and with none, nobody's.
pub fn expire_due(ss: &mut ProcessTable) -> Expired {
    let now = now_us();
    if TIMER.with(|t| t.threads > now && t.budgets > now) {
        return Expired { destroyed: false, last: None };
    }
    #[cfg(feature = "walk-trace")]
    let _walk = crate::sched::trace::walk(crate::sched::trace::EXPIRY);
    let mut destroyed = false;
    let mut expired = false;
    let mut last = None;
    let mut stale_pid;
    let mut next_timeout;
    let mut started;
    loop {
        started = crate::sched::now_ticks();
        let walk = MemoryManager::with_mut(|mm| crate::message::next_timeout(mm, now));
        let timeout = walk.due;
        next_timeout = walk.next;
        stale_pid = walk.stale;
        let budget = due_budget(now);
        // Earliest first; at an equal instant, the timeout.
        let timeout_first = match (timeout, budget) {
            (None, None) => break,
            (Some((td, _, _)), Some((bd, _, _))) => td <= bd,
            (t, _) => t.is_some(),
        };
        expired = true;
        if let (true, Some((_, pid, tid))) = (timeout_first, timeout) {
            last = MemoryManager::with_mut(|mm| {
                crate::message::time_out(ss, mm, pid, tid);
                let frame = mm.budget_of(pid)?;
                let b = BudgetRef { frame, id: mm.budget(frame).id };
                crate::sched::bill(mm, b, crate::sched::now_ticks().saturating_sub(started));
                Some(b)
            });
        } else if let Some((_, _, frame)) = budget {
            // The payer `destroy_subtree` names, asked as it asks: after `mark_dying`.
            last = MemoryManager::with_mut(|mm| {
                mm.mark_dying(frame);
                mm.destruction_payer(frame)
            });
            crate::budget::destroy_subtree(ss, frame, running(), Some(started));
            destroyed = true;
        }
    }
    // Nothing expired: the walk found a wait that ended before its timeout (an item expired
    // leaves its own process's cache behind, which the walks after it find).
    let stale = !expired && stale_pid.is_some();
    if stale {
        last = MemoryManager::with(|mm| {
            let frame = mm.budget_of(stale_pid?)?;
            Some(BudgetRef { frame, id: mm.budget(frame).id })
        });
    }
    #[cfg(feature = "sched-trace")]
    crate::sched::trace::expired(last, stale);
    let next_budget = MemoryManager::with(|mm| {
        mm.deadlines().map(|(d, _, _)| d).filter(|d| *d > now).min().unwrap_or(NEVER)
    });
    TIMER.with(|t| {
        t.threads = next_timeout;
        t.budgets = next_budget;
        // The hardware fired (or will, for what just passed); arm afresh.
        t.armed = 0;
    });
    rearm();
    if let Some(b) = last {
        MemoryManager::with_mut(|mm| {
            crate::sched::bill(mm, b, crate::sched::now_ticks().saturating_sub(started))
        });
    }
    Expired { destroyed, last }
}

/// A timer interrupt arrived. The trap handler has already expired what is due at its entry;
/// all that is left is to arm for the next thing (a stale early hint lands here too).
pub fn on_interrupt() {
    TIMER.with(|t| t.armed = 0);
    rearm();
}

/// Expire at a kernel entry.
pub fn expire_at_entry() -> Expired { ProcessTable::with_mut(expire_due) }
