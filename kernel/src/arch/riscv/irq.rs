// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use riscv::register::{scause, sepc, sstatus, stval};

use crate::arch::current_pid;
use crate::arch::exception::RiscvException;
use crate::arch::mem::MemoryMapping;
use crate::arch::process::{EXIT_THREAD, Thread};
use crate::arch::process::Process as ArchProcess;
use crate::services::SystemServices;

extern "Rust" {
    fn _redoubt_syscall_return_result(args: &[usize; 8], context: &Thread) -> !;
}

/// Resume `context` with `a0..=a7` = `args`.
fn return_registers(args: &[usize; 8], context: &Thread) -> ! {
    // Leaving the kernel: the scheduler's exit hook (`sched.rs`, accounting at the trap boundary).
    crate::sched::leave(current_pid());
    // SAFETY: `_redoubt_syscall_return_result` (asm) writes `args` into the return registers
    // and resumes `context` with `sret`. Both point at valid, kernel-owned data and it
    // does not return.
    unsafe { _redoubt_syscall_return_result(args, context) }
}

/// The interrupt controller backend. Every backend provides `enable_irq`, `disable_irq`,
/// `complete`, `pending` and `mask`; add a new controller
/// (AIA, CLIC, ...) as another file and capability feature.
#[cfg_attr(feature = "plic", path = "intc_plic.rs")]
mod intc;

/// The hart timer backend. The timer is the kernel's (`crate::time`), never a device userspace
/// owns.
#[path = "timer_sbi.rs"]
pub mod timer;

pub fn init() {
    #[cfg(feature = "plic")]
    intc::init();
    timer::init();
}

pub fn enable_irq(irq_no: usize) { intc::enable_irq(irq_no); }

pub fn disable_irq(irq_no: usize) { intc::disable_irq(irq_no); }

/// Complete the interrupt the trap handler claimed (`intc::pending`).
pub fn complete_irq() { intc::complete(); }

/// Resume whatever is current now: after the entering thread's process died at this entry, or
/// once a trap is fully handled.
fn resume_current() -> ! {
    ArchProcess::with_current_mut(|p| {
        crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
    })
}

/// Preempt the running thread (R12: its slice ended, or a budget deadline fired): it stays ready
/// and `kmain` picks again. A trap it had not yet been handled for is taken again when it next
/// runs: its `sepc` is untouched (an `ecall` is stepped over only when handled).
fn preempt() -> ! {
    let tid = ArchProcess::with_current(|p| p.current_tid());
    SystemServices::with_mut(|ss| crate::sched::preempt(ss, tid));
    resume_current()
}

/// A system call: every user-mode `ecall`, and the only decoder, `redoubt::handle` (redoubt-sys).
/// It never returns to the trap handler: the caller resumes past its `ecall` with the result,
/// or whatever is current runs.
fn system_call(pid: redoubt_layout::Pid, regs: [usize; 8]) -> ! {
    let tid = ArchProcess::with_current_mut(|p| {
        p.current_thread_mut().sepc += 4;
        p.current_tid()
    });
    match crate::redoubt::handle(pid, tid, &regs.map(|r| r as u64)) {
        // Every result register holds at most 32 bits or one `usize` (redoubt-sys).
        crate::redoubt::Outcome::Return(out) => ArchProcess::with_current_mut(|p| {
            return_registers(&out.map(|r| r as usize), p.current_thread())
        }),
        crate::redoubt::Outcome::Resume => resume_current(),
    }
}

/// Trap entry point rust (_start_trap_rust)
///
/// scause is read to determine the cause of the trap. The top bit indicates if
/// it's an interrupt or an exception. The result is converted to an element of
/// the Interrupt or Exception enum and passed to handle_interrupt or
/// handle_exception.
#[export_name = "_start_trap_rust"]
#[allow(unreachable_code)] // panic handler will terminate execution
pub extern "C" fn trap_handler(
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> ! {
    let sc = scause::read();

    // If we were previously in Supervisor mode and we've just tried to write to
    // invalid memory, then we likely blew out the stack.
    if cfg!(any(target_arch = "riscv32", target_arch = "riscv64"))
        && sstatus::read().spp() == sstatus::SPP::Supervisor
        && sc.bits() == 0xf
    {
        let pid = current_pid();
        let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
        MemoryMapping::current().print_map();
        panic!("KERNEL({}): RISC-V fault: {} - maybe ran out of kernel stack?", pid, ex);
    }

    let pid = current_pid();
    let epc = sepc::read();

    let ex = RiscvException::from_regs(sc.bits(), epc, stval::read());

    // The user time since the last return is the running budget's (`sched.rs`).
    let from_user = sstatus::read().spp() == sstatus::SPP::User;
    if from_user {
        crate::sched::from_user();
    }
    // Every entry but `kmain`'s switch answers the deadlines that have passed first
    // (`time.rs`), so a deadline beats anything that enters after it. If that ended the entering
    // process (a budget deadline), there is nothing of it left to handle: run what is current.
    // A budget deadline is a preemption point (R12): the entering thread yields the CPU before
    // anything else, and its trap is taken again when it next runs.
    if !matches!(ex, RiscvException::CallFromSMode(..)) {
        let destroyed = crate::time::expire_at_entry();
        if from_user
            && (current_pid() != pid
                || SystemServices::with(|ss| ss.get_process(pid).map_or(true, |p| p.free())))
        {
            resume_current();
        }
        if from_user && destroyed {
            preempt();
        }
    }
    // From here, kernel time is the running budget's: a system call's is its caller's.
    if from_user {
        crate::sched::begin_billing();
    }
    #[cfg(any(feature = "debug-print"))] // , feature = "debug-swap-verbose"
    {
        let pid = current_pid();
        let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
        let tid = ArchProcess::with_current(|p| p.current_tid());
        println!(
            "IRQ ({}.{}): {} sepc {:x} sim {:x}", //  reg {:08x?}
            pid,
            tid,
            ex,
            sepc::read(),
            intc::mask(),
            // ArchProcess::with_current(|p| p.current_thread().registers)
        );
    }
    match ex {
        // `kmain`'s switch (`sched::switch_to`), the one S-mode `ecall`: resumed past it.
        RiscvException::CallFromSMode(..) => {
            ArchProcess::with_current_mut(|p| p.current_thread_mut().sepc += 4);
            SystemServices::with_mut(|ss| crate::sched::switch(ss, a0, a1, a2));
            resume_current()
        }
        RiscvException::CallFromUMode(..) => system_call(pid, [a0, a1, a2, a3, a4, a5, a6, a7]),
        // The kernel's timer: what was due was answered at this entry; arm for what is next.
        RiscvException::SupervisorTimerInterrupt(_) => {
            crate::time::on_interrupt();
            // The running thread's slice is over: preempt it (R12).
            if from_user && crate::sched::slice_over() {
                preempt();
            }
            resume_current();
        }
        // Hardware interrupt
        RiscvException::UserExternalInterrupt(_) | RiscvException::SupervisorExternalInterrupt(_) => {
            // The controller claims one interrupt; `None` is a spurious trap with nothing
            // pending, which we ignore and just resume from. Handling it is its device owner's
            // work, not the interrupted budget's (`sched::bill_irq`).
            let started = crate::sched::now_ticks();
            let pending = intc::pending();

            if let Some(irq) = pending {
                // R5: an interrupt with a device object is the kernel's to record: it masks the
                // source, sets `fired` and wakes whoever is in `receive` on the handle. One with no
                // device object has nobody to tell: complete the claim, then mask the source so it
                // cannot storm.
                if !crate::device::irq_fired(irq) {
                    klog!("[!] Masked IRQ #{}, which no device object owns", irq);
                    complete_irq();
                    disable_irq(irq);
                }
                crate::sched::bill_irq(irq, started);
            }
            resume_current()
        }

        // See if it's a known exception, such as writing to a demand-paged area
        // or returning from a thread. If so, handle the exception
        // and return right away.
        RiscvException::StorePageFault(_pc, addr) | RiscvException::LoadPageFault(_pc, addr) => {
            #[cfg(all(feature = "debug-print", feature = "print-panics"))]
            println!("KERNEL({}): RISC-V fault: {} @ {:08x}, addr {:08x} - ", pid, ex, _pc, addr);
            crate::mem::MemoryManager::with_mut(|mm| {
                // A valid mapping faulted on permissions: retrying cannot make progress.
                if crate::arch::mem::is_mapped(addr) {
                    return Err(crate::mem::PageError::InUse);
                }
                crate::arch::mem::ensure_page_exists_inner(mm, addr)
            })
            .map(|_new_page| {
                ArchProcess::with_current_mut(|process| {
                    #[cfg(all(feature = "debug-print", feature = "print-panics"))]
                    println!(
                        "SPF Handing page {:08x} to pid {} tid {} sepc {:x}",
                        _new_page,
                        process.pid().get(),
                        process.current_tid(),
                        process.current_thread().sepc,
                    );
                    crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                });
            })
            .ok(); // If this fails, fall through.
        }

        RiscvException::InstructionPageFault(EXIT_THREAD, _offset)
            if ArchProcess::with_current(|process| process.current_tid())
                >= crate::arch::process::INITIAL_TID =>
        {
            let tid = ArchProcess::with_current(|process| process.current_tid());
            // Ordinary thread returns use the same lifecycle policy as explicit thread_exit:
            // the final return snapshots open calls/blame before process_exit(0) cleanup (170).
            SystemServices::with_mut(|ss| crate::process::thread_exit(ss, pid, tid));

            // Teardown selected a surviving sibling or another process.
            ArchProcess::with_current_mut(|p| {
                crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
            });
        }

        // Handle faulted instruction pages, because we can now actually have instruction pages that are
        // swapped out.
        _ => {
            println!("!!! Unrecognized exception: {:x?}", ex);
        }
    }

    // Read at entry, before expiry (which never changes it, but nothing here depends on that).
    let is_kernel_failure = !from_user;
    // The exception was not handled. We should terminate the program here.
    // For now, let's halt the whole system instead so that it becomes
    // immediately obvious that we screwed up. On hardware this will trigger
    // a watchdog reset.
    println!(
        "{}: CPU Exception on PID {}: {}",
        if is_kernel_failure { "!!! KERNEL FAILURE !!!" } else { "PROGRAM HALT" },
        pid,
        ex
    );
    ArchProcess::with_current(|process| {
        println!("Current thread {}:", process.current_tid());
        process.print_current_thread();
    });

    // If this is a failure in the kernel, go into an infinite loop
    MemoryMapping::current().print_map();
    if is_kernel_failure {
        #[allow(clippy::empty_loop)]
        loop {}
    }

    // If it's not a failure in the kernel, the process faults: it is torn down and its exit
    // notice, cause `faulted`, blames the sender of the faulting thread's current call
    // (KERNEL-SPEC.md, Messages; `process.rs`). The code is the RISC-V exception cause.
    crate::process::faulted(pid, (sc.bits() & 0xff) as u32);

    // Resume the parent process.
    ArchProcess::with_current_mut(|process| {
        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
    })
}
