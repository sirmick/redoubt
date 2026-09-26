// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{sepc, sstatus};

use crate::ptable::Thread;

extern "C" {
    fn _redoubt_resume_context(regs: *const usize) -> !;
}

fn set_supervisor(supervisor: bool) {
    // SAFETY: sets sstatus.SPP, which only chooses the privilege mode the next `sret`
    // returns to. It has no effect until `resume` issues that `sret`.
    unsafe {
        sstatus::set_spp(if supervisor { sstatus::SPP::Supervisor } else { sstatus::SPP::User });
    }
}

pub fn resume(supervisor: bool, thread: &Thread) -> ! {
    // Leaving the kernel: the scheduler's exit hook (`sched.rs`, accounting at the trap boundary).
    crate::sched::leave(crate::arch::current_pid());
    // SAFETY: sets sepc, the address `sret` will resume at. Harmless until the `sret` in
    // `_redoubt_resume_context`. (`unsafe` on the upstream `riscv` crate used for rv64, a
    // no-op wrapper on the vendored rv32 one.)
    #[allow(unused_unsafe)]
    unsafe {
        sepc::write(thread.sepc)
    };

    // Return to the appropriate CPU mode
    set_supervisor(supervisor);
    #[cfg(feature = "debug-print")]
    println!(
        "Switching to PID {}, SP: {:08x}, PC: {:08x}",
        crate::arch::current_pid(),
        thread.registers[1],
        thread.sepc,
    );
    // SAFETY: `_redoubt_resume_context` (asm) restores all registers from this thread's saved
    // register block and `sret`s. `thread.registers` is that block, and sepc/sstatus were
    // just set to match. It does not return.
    unsafe { _redoubt_resume_context(thread.registers.as_ptr()) };
}

/// `kmain`'s switch (`sched::switch_to`): an `ecall` from S-mode into the kernel's own trap
/// handler, with `a0..=a2` = `tag`, `pid`, `tid`. Returns the `a0` that `kmain` is resumed with.
///
/// Under SBI firmware an S-mode `ecall` is a call into that firmware and never reaches the kernel.
/// So this enters the trap handler directly, with the CSRs set up exactly as the hardware would
/// have left them for an `ecall` from S-mode.
pub fn switch_trap(tag: usize, pid: usize, tid: usize) -> usize {
    let mut a0 = tag;
    // SAFETY: this hand-crafts the CSR state of an `ecall`-from-S-mode trap and jumps to
    // the trap vector, so the kernel takes its own trap exactly as hardware would deliver
    // it. The handler saves every register of this context and restores it on the way back;
    // sepc points just past the block, so it resumes here.
    unsafe {
        core::arch::asm!(
            // The handler resumes at sepc + 4, as if stepping over a 4-byte `ecall`.
            "lla {tmp}, 2f - 4",
            "csrw sepc, {tmp}",
            "li {tmp}, 9",          // scause: environment call from S-mode
            "csrw scause, {tmp}",
            "li {tmp}, 0x22",       // sstatus.SIE = 0 now, and again after sret (SPIE = 0)
            "csrc sstatus, {tmp}",
            "li {tmp}, 0x100",      // sstatus.SPP = Supervisor
            "csrs sstatus, {tmp}",
            "j _start_trap",
            ".balign 4",
            "2:",
            tmp = out(reg) _,
            inlateout("a0") a0,
            inlateout("a1") pid => _,
            inlateout("a2") tid => _,
            lateout("a3") _,
            lateout("a4") _,
            lateout("a5") _,
            lateout("a6") _,
            lateout("a7") _,
        )
    };
    a0
}
