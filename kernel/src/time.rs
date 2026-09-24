// SPDX-License-Identifier: MIT OR Apache-2.0

//! The kernel-owned timer (RESOURCES.md, The timer; KERNEL-SPEC.md, I13 and R12).
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
//! It runs at every kernel entry but the kernel's own `SwitchTo`, first, before anything reads
//! the entering process, so a deadline that has passed beats any operation that enters later.
//! `kmain` expires, then picks, then switches, with no kernel entry in between.
//!
//! While a legacy interrupt callback runs, budget deadlines wait (timeouts still expire: they
//! only wake): the process it interrupted must be there to go back to. They are handled as the
//! callback returns (INTERIM, until WP-K6 removes callbacks).
//!
//! # Hints
//! Finding what is due means walking every thread (`message.rs`) and the list of budgets with a
//! deadline (`budget.rs`). Both are skipped while the cached earliest deadlines are still in the
//! future. A hint is only ever early: a blocking call or a new budget lowers it, and a cancelled
//! wait or a destroyed budget leaves it where it was, which costs at most one early interrupt and
//! a walk that recomputes it. So nothing is missed.
//!
//! Microseconds are the ABI's unit (KERNEL-SPEC.md, Constants): deadlines are kept in them, and
//! converted to timer ticks rounding up, so an interrupt never comes before its deadline.

use crate::arch::irq::timer;
use crate::cell::KernelCell;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

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

/// Arm the timer for the earliest thing due, if that changed. Budget deadlines wait while a
/// legacy callback runs (module docs).
pub fn rearm() {
    let callback = crate::arch::irq::in_callback();
    TIMER.with(|t| {
        let target = if callback { t.threads } else { t.threads.min(t.budgets).min(t.slice) };
        if target != t.armed {
            t.armed = target;
            timer::set(if target == NEVER { u64::MAX } else { timer::us_to_ticks(target) });
        }
    });
}

/// The budget deadline due first at `now`, if budget deadlines may be handled: (deadline, id,
/// frame).
fn due_budget(now: u64) -> Option<(u64, u64, u32)> {
    MemoryManager::with(|mm| mm.deadlines().filter(|(d, _, _)| *d <= now).min())
}

/// The process a destruction happens under: the one running (whose call or run this entry
/// interrupted), if it is not the kernel.
fn running() -> Option<redoubt_abi::PID> {
    let pid = crate::arch::current_pid();
    (pid.get() != 1).then_some(pid)
}

/// Handle everything due (module docs), then re-arm for the next thing. Returns whether a budget
/// deadline fired (a preemption point, R12). Takes the scheduler only: destroying a budget
/// borrows the memory manager in phases (`process.rs`, Locks).
///
/// Each item's handling is billed to its own budget (`sched::bill`): a timeout to its thread's,
/// a deadline to the dying budget (whose debt then moves up). The walks that find them are the
/// kernel's.
pub fn expire_due(ss: &mut SystemServices) -> bool {
    let now = now_us();
    let callback = crate::arch::irq::in_callback();
    if TIMER.with(|t| t.threads > now && (callback || t.budgets > now)) {
        return false;
    }
    let mut destroyed = false;
    let mut next_timeout;
    loop {
        let (timeout, next) = MemoryManager::with_mut(|mm| crate::message::next_timeout(mm, now));
        next_timeout = next;
        let budget = if callback { None } else { due_budget(now) };
        // Earliest first; at an equal instant, the timeout.
        let timeout_first = match (timeout, budget) {
            (None, None) => break,
            (Some((td, _, _)), Some((bd, _, _))) => td <= bd,
            (t, _) => t.is_some(),
        };
        if let (true, Some((_, pid, tid))) = (timeout_first, timeout) {
            let started = crate::sched::now_ticks();
            MemoryManager::with_mut(|mm| {
                crate::message::time_out(ss, mm, pid, tid);
                if let Some(frame) = mm.budget_of(pid) {
                    let b = crate::handle::BudgetRef { frame, id: mm.budget(frame).id };
                    crate::sched::bill(mm, b, crate::sched::now_ticks().saturating_sub(started));
                }
            });
        } else if let Some((_, _, frame)) = budget {
            MemoryManager::with_mut(|mm| mm.mark_dying(frame));
            crate::budget::destroy_subtree(ss, frame, running(), true);
            destroyed = true;
        }
    }
    let next_budget = MemoryManager::with(|mm| {
        mm.deadlines().map(|(d, _, _)| d).filter(|d| callback || *d > now).min().unwrap_or(NEVER)
    });
    TIMER.with(|t| {
        t.threads = next_timeout;
        t.budgets = next_budget;
        // The hardware fired (or will, for what just passed); arm afresh.
        t.armed = 0;
    });
    rearm();
    destroyed
}

/// A timer interrupt arrived. The trap handler has already expired what is due at its entry;
/// all that is left is to arm for the next thing (a stale early hint lands here too).
pub fn on_interrupt() {
    TIMER.with(|t| t.armed = 0);
    rearm();
}

/// Expire at a kernel entry; whether a budget deadline fired.
pub fn expire_at_entry() -> bool { SystemServices::with_mut(expire_due) }
