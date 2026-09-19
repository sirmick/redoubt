// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{satp, sie, sstatus};
use xous_kernel::PID;

mod asm;
pub mod exception;
pub mod irq;
pub mod mem;
mod mmu_flags;
pub mod panic;
pub mod process;
mod sv39;
pub mod syscall;

pub fn current_pid() -> PID { PID::new(mem::pid_from_satp(satp::read().bits()) as _).unwrap() }

pub fn init() {
    irq::init();

    println!("W^X verified: {} executable kernel pages, none writable under any alias", mem::verify_kernel_wx());

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
    // Park the hart until an interrupt is pending.
    // SAFETY: `wfi` has no memory effect. (`unsafe` on the vendored rv32 riscv crate,
    // a safe no-op wrapper on the rv64 one.)
    #[allow(unused_unsafe)]
    unsafe {
        riscv::asm::wfi()
    };

    // Briefly enable interrupts in Supervisor mode so any pending one drains into its
    // userspace handler; otherwise interrupts stay disabled while in Supervisor mode.
    // SAFETY: the kernel holds no borrow across this window.
    unsafe {
        sstatus::set_sie();
        sstatus::clear_sie();
    }
    true
}
