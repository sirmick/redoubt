// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{senvcfg, sie, sstatus};

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

    // SAFETY: enabling supervisor software and external interrupts. The kernel runs with
    // sstatus.SIE clear, so these are only actually taken once execution returns to
    // userspace, where the trap handler is ready for them.
    unsafe {
        sie::set_ssoft();
        sie::set_sext();
    }
}

/// Put the core to sleep until an interrupt hits. Returns `true` to indicate the kernel
/// should not exit.
pub fn idle() -> bool {
    // Park the hart until an interrupt is pending, without the kernel lock: another hart's wake
    // sends this one the reschedule interrupt while it is marked idle (`hart::wake_idle`).
    hart::set_idle(true);
    crate::cell::KERNEL_LOCK.release();
    // SAFETY: `wfi` has no memory effect.
    unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
    crate::cell::KERNEL_LOCK.acquire();
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
