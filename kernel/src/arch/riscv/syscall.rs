// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{sepc, sstatus};

use crate::services::Thread;

extern "C" {
    fn _xous_resume_context(regs: *const usize) -> !;
}

pub fn invoke(thread: &mut Thread, supervisor: bool, pc: usize, sp: usize, ret_addr: usize, args: &[usize]) {
    set_supervisor(supervisor);
    thread.registers[0] = ret_addr;
    thread.registers[1] = sp;
    assert!(args.len() <= 8, "too many arguments to invoke()");
    for (idx, arg) in args.iter().enumerate() {
        thread.registers[9 + idx] = *arg;
    }
    thread.sepc = pc;
}

fn set_supervisor(supervisor: bool) {
    // SAFETY: sets sstatus.SPP, which only chooses the privilege mode the next `sret`
    // returns to. It has no effect until `resume` issues that `sret`.
    unsafe {
        sstatus::set_spp(if supervisor { sstatus::SPP::Supervisor } else { sstatus::SPP::User });
    }
}

pub fn resume(supervisor: bool, thread: &Thread) -> ! {
    // SAFETY: sets sepc, the address `sret` will resume at. Harmless until the `sret` in
    // `_xous_resume_context`. (`unsafe` on the upstream `riscv` crate used for rv64, a
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
    // SAFETY: `_xous_resume_context` (asm) restores all registers from this thread's saved
    // register block and `sret`s. `thread.registers` is that block, and sepc/sstatus were
    // just set to match. It does not return.
    unsafe { _xous_resume_context(thread.registers.as_ptr()) };
}

/// Make a syscall from inside the kernel (PID 1).
///
/// Without SBI firmware, the loader delegates S-mode `ecall` back to S-mode, so the
/// kernel can simply `ecall` into its own trap handler.
#[cfg(not(feature = "sbi"))]
pub fn kernel_syscall(call: xous_kernel::SysCall) -> xous_kernel::SysCallResult { xous_kernel::rsyscall(call) }

/// Make a syscall from inside the kernel (PID 1).
///
/// Under SBI firmware an S-mode `ecall` is a call into that firmware and never reaches us. Instead, enter the trap handler directly, with the
/// CSRs set up exactly as the hardware would have left them for an `ecall` from S-mode.
#[cfg(feature = "sbi")]
pub fn kernel_syscall(call: xous_kernel::SysCall) -> xous_kernel::SysCallResult {
    let mut args = call.as_args();
    // SAFETY: this hand-crafts the CSR state of an `ecall`-from-S-mode trap and jumps to
    // the trap vector, so the kernel takes its own syscall exactly as hardware would
    // deliver it. sepc points just past the block, so the handler resumes here.
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
            inlateout("a0") args[0],
            inlateout("a1") args[1],
            inlateout("a2") args[2],
            inlateout("a3") args[3],
            inlateout("a4") args[4],
            inlateout("a5") args[5],
            inlateout("a6") args[6],
            inlateout("a7") args[7],
        )
    };
    match xous_kernel::Result::from_args(args) {
        xous_kernel::Result::Error(e) => Err(e),
        other => Ok(other),
    }
}
