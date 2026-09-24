// SPDX-License-Identifier: MIT OR Apache-2.0

//! The kernel-owned timer (RESOURCES.md, The timer; KERNEL-SPEC.md, I13 and R12).
//!
//! One hardware timer, always armed for the earliest of what is due: a blocking call's timeout
//! (I13). Nothing in userspace programs it and there is no timer interrupt for userspace; user
//! mode reads the counter directly (`rdtime`).
//!
//! # Expiry
//! [`expire_due`] answers every timeout that has passed, earliest first (at an equal instant,
//! in (pid, tid) order), with `Timeout`. It runs at every kernel entry from user mode and at
//! every timer or interrupt trap, before anything else looks at the entering process, so a
//! deadline that has passed beats any operation that enters later (a reply after a caller's
//! timeout finds the call already abandoned). The kernel's own `SwitchTo` entry never expires:
//! `kmain` expires, then picks, then switches, with no kernel entry in between.
//!
//! # Hints
//! Finding what is due means walking every thread (`message.rs`). The walk is skipped while the
//! cached earliest deadline is still in the future. The hint is only ever early: `mark` lowers it
//! as a call blocks, and a cancelled wait (a reply, a death) leaves it where it was, which costs
//! at most one early interrupt and a walk that recomputes it. So no deadline is missed.
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
    /// What the hardware is armed for, in microseconds.
    armed: u64,
}

static TIMER: KernelCell<Timer> = KernelCell::new(Timer { threads: NEVER, armed: NEVER });

/// Monotonic microseconds since boot.
pub fn now_us() -> u64 { timer::now_us() }

/// A call blocked until `deadline`: make sure the timer comes by then.
pub fn note_timeout(deadline: u64) {
    TIMER.with(|t| t.threads = t.threads.min(deadline));
    rearm();
}

/// Arm the timer for the earliest thing due, if that changed.
pub fn rearm() {
    TIMER.with(|t| {
        let target = t.threads;
        if target != t.armed {
            t.armed = target;
            timer::set(if target == NEVER { u64::MAX } else { timer::us_to_ticks(target) });
        }
    });
}

/// Answer every timeout that has passed (I13), earliest first, then re-arm for the next one.
pub fn expire_due(ss: &mut SystemServices, mm: &mut MemoryManager) {
    let now = now_us();
    if TIMER.with(|t| t.threads > now) {
        return;
    }
    let next = crate::message::expire_due(ss, mm, now);
    TIMER.with(|t| {
        t.threads = next;
        // The hardware fired (or will, for what just passed); arm afresh.
        t.armed = 0;
    });
    rearm();
}

/// A timer interrupt arrived. The trap handler has already expired what is due at its entry;
/// all that is left is to arm for the next thing (a stale early hint lands here too).
pub fn on_interrupt() {
    TIMER.with(|t| t.armed = 0);
    rearm();
}

/// Expire at a kernel entry: both managers, borrowed in the kernel's order.
pub fn expire_at_entry() { SystemServices::with_mut(|ss| MemoryManager::with_mut(|mm| expire_due(ss, mm))); }
