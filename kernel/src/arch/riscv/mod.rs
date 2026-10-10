// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{senvcfg, sstatus};

mod asm;
pub mod exception;
pub mod hart;
pub mod irq;
pub mod mem;
mod mmu_flags;
pub mod panic;
mod physmap;
pub mod process;
pub mod syscall;

/// The running PID: the kernel's own record of it (kernel/memory-layout.md, "`satp`").
pub use process::current_pid;

pub fn init() {
    // R24: `_start` cleared both, and nothing sets them again.
    let status = sstatus::read();
    assert!(!status.sum() && !status.mxr(), "sstatus.SUM or MXR is set at boot (R24)");
    // R11: `_start` wrote senvcfg 0, so no cache-block operation runs in user mode.
    assert_eq!(senvcfg::read().bits(), 0, "senvcfg is not 0 at boot (R11)");
    irq::init();

    println!(
        "W^X verified: {} executable kernel pages, none writable under any alias",
        mem::verify_kernel_wx()
    );
}

/// Halt the hart (`wfi`) until an interrupt enabled in `sie` is pending, taken or not: with
/// `sstatus.SIE` clear it is not, and the hart goes on after the halt.
pub fn halt() { halt_masking(0) }

/// [`halt`] until the reschedule interrupt alone is pending: a wait (for the kernel lock, or for a
/// shootdown's acknowledgement) that a pending timer or device interrupt would end at once, every
/// turn, since a hart without the lock cannot take it. Such a wait would spin, not halt (R78).
pub fn halt_for_ipi() { halt_masking(STIE | SEIE) }

/// `sie`'s timer and external interrupt enables.
const STIE: usize = 1 << 5;
const SEIE: usize = 1 << 9;

/// `wfi` with the `sie` bits in `mask` cleared, and `sie` restored after; no `wfi` at all if an
/// interrupt it waits for is pending already. The architecture lets `wfi` return at once then, but
/// QEMU halts the hart regardless and, under `-icount`, gives the host thread to every other hart's
/// turn before it looks again: up to a kernel section each (`sched-lock-contention-4`).
fn halt_masking(mask: usize) {
    if riscv::register::sip::read().bits() & riscv::register::sie::read().bits() & !mask != 0 {
        #[cfg(debug_assertions)]
        SKIPPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        return;
    }
    // SAFETY: `sie` only chooses which pending interrupts end the halt (and, with `sstatus.SIE`
    // set, which are taken; it is clear in the kernel); it is restored before this returns, and
    // `wfi` has no memory effect.
    unsafe {
        core::arch::asm!(
            "csrrc {saved}, sie, {mask}",
            "wfi",
            "csrw sie, {saved}",
            mask = in(reg) mask,
            saved = out(reg) _,
            options(nomem, nostack)
        )
    };
}

/// A checked build's count of the halts skipped because an awaited interrupt was pending, which
/// `sched-lock-contention` requires above 0 (`hart::report`).
#[cfg(debug_assertions)]
pub static SKIPPED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Put the core to sleep until an interrupt hits. Returns `true` to indicate the kernel
/// should not exit.
pub fn idle() -> bool {
    // Park the hart until an interrupt is pending, without the kernel lock: another hart's wake
    // sends this one the reschedule interrupt while it is marked idle (`hart::wake_idle`).
    hart::set_idle(true);
    crate::reclaim::end_section();
    // With every hart idle, this one zeroes a frame it freed before it halts (R81), unless another
    // hart is draining; with more left it does not halt, but comes back here through `kmain`'s
    // interrupt-enabled top for the next. Only then: under `icount` the harts share one virtual
    // clock, and a hart that zeroes while another works slows that one's time.
    let (frame, more, wake) = crate::reclaim::take_one(hart::all_idle());
    crate::cell::KERNEL_LOCK.release();
    hart::wake_halted(wake);
    if frame != 0 {
        crate::reclaim::zero_taken(frame);
    }
    if frame == 0 || !more {
        halt();
    }
    crate::cell::KERNEL_LOCK.acquire();
    crate::mem::entered();
    crate::sched::zeroed_idle();
    hart::set_idle(false);

    // Briefly enable interrupts in Supervisor mode so any pending one drains into its
    // userspace handler; otherwise interrupts stay disabled while in Supervisor mode.
    // SAFETY: the kernel holds no borrow across this window.
    unsafe {
        sstatus::set_sie();
        sstatus::clear_sie();
    }
    true
}
