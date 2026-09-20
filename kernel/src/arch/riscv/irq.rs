// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::sync::atomic::{AtomicBool, Ordering};

use riscv::register::{scause, sepc, sstatus, stval};
use xous_kernel::{PID, SysCall, TID};

use crate::arch::current_pid;
use crate::arch::exception::RiscvException;
use crate::arch::mem::MemoryMapping;
use crate::arch::process::{EXIT_THREAD, RETURN_FROM_ISR, Thread};
use crate::arch::process::{Process as ArchProcess, RETURN_FROM_EXCEPTION_HANDLER};
use crate::cell::KernelCell;
use crate::services::SystemServices;

extern "Rust" {
    fn _xous_syscall_return_result(args: &[usize; 8], context: &Thread) -> !;
}

/// Resume `context`, delivering `result` in its argument registers.
///
/// The result is serialized with `to_args()` rather than by reinterpreting the enum's
/// memory as eight registers. The two only coincide when every field is exactly one
/// register wide, which is not the case on rv64 (e.g. a `SID` is four `u32`s).
fn return_result(result: &xous_kernel::Result, context: &Thread) -> ! { return_registers(&result.to_args(), context) }

/// Resume `context` with `a0..=a7` = `args`.
fn return_registers(args: &[usize; 8], context: &Thread) -> ! {
    // SAFETY: `_xous_syscall_return_result` (asm) writes `args` into the return registers
    // and resumes `context` with `sret`. Both point at valid, kernel-owned data and it
    // does not return.
    unsafe { _xous_syscall_return_result(args, context) }
}

/// The interrupt controller backend. Every backend provides `enable_irq`, `disable_irq`,
/// `disable_all_irqs`, `enable_all_irqs`, `pending` and `mask`; add a new controller
/// (AIA, CLIC, ...) as another file and capability feature.
#[cfg_attr(feature = "plic", path = "intc_plic.rs")]
mod intc;

/// The hart timer backend, for platforms where the timer is a CPU resource rather than
/// a device that userspace can own. See `planning/redoubt/TIMER.md`.
#[cfg_attr(feature = "sbi", path = "timer_sbi.rs")]
pub mod timer;

pub fn init() {
    #[cfg(feature = "plic")]
    intc::init();
    #[cfg(feature = "sbi")]
    timer::init();
}

pub fn enable_irq(irq_no: usize) {
    // The timer is armed by setting a deadline, not by claiming its interrupt.
    if !timer::owns(irq_no) {
        intc::enable_irq(irq_no);
    }
}

pub fn disable_irq(irq_no: usize) {
    if timer::owns(irq_no) {
        timer::mask();
    } else {
        intc::disable_irq(irq_no);
    }
}

/// Hold off every interrupt source while a userspace handler runs; Xous does not nest them.
pub fn disable_all_irqs() {
    intc::disable_all_irqs();
    timer::mask();
}

pub fn enable_all_irqs() {
    intc::enable_all_irqs();
    timer::unmask();
}

// Indicate when we handle an IRQ
static HANDLING_IRQ: AtomicBool = AtomicBool::new(false);

/// The (PID, TID) to resume after an interrupt handler returns. Set when an interrupt
/// redirects into a userspace handler, cleared when it finishes.
static PREVIOUS_PAIR: KernelCell<Option<(PID, TID)>> = KernelCell::new(None);

/// Record who to resume after an interrupt handler returns.
///
/// # Safety
/// The operation is sound on its own; `unsafe` is a cross-architecture ABI marker (the
/// arm and hosted backends share the signature). Callers coordinate ISR return state and
/// must pair this with exactly one `take_isr_return_pair`.
pub unsafe fn set_isr_return_pair(pid: PID, tid: TID) { PREVIOUS_PAIR.with(|p| *p = Some((pid, tid))); }

/// Finish a pending ISR. Return `false` if there was none.
fn finish_isr() -> bool {
    if !HANDLING_IRQ.swap(false, Ordering::Relaxed) {
        return false;
    }

    // If we hit this address, then an ISR has just returned.  Since
    // we're in an interrupt context, it is safe to access this
    // global variable.
    let (previous_pid, previous_context) =
        PREVIOUS_PAIR.with(|p| p.take()).expect("got RETURN_FROM_ISR with no previous PID");
    // println!(
    //     "ISR: Resuming previous pair of ({}, {})",
    //     previous_pid, previous_context
    // );
    // Switch to the previous process' address space.
    SystemServices::with_mut(|ss| {
        ss.finish_callback_and_resume(previous_pid, previous_context).expect("unable to resume previous PID")
    });

    // Re-enable interrupts now that they're handled
    enable_all_irqs();

    true
}

/// Convert a RISC-V `Exception` into a Xous exception argument list.
fn generate_exception_args(ex: &RiscvException) -> Option<[usize; 3]> {
    match *ex {
        RiscvException::InstructionAddressMisaligned(epc, addr) => {
            Some([xous_kernel::ExceptionType::InstructionAddressMisaligned as usize, epc, addr])
        }
        RiscvException::InstructionAccessFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::InstructionAccessFault as usize, epc, addr])
        }
        RiscvException::IllegalInstruction(epc, instruction) => {
            Some([xous_kernel::ExceptionType::IllegalInstruction as usize, epc, instruction])
        }
        RiscvException::LoadAddressMisaligned(epc, addr) => {
            Some([xous_kernel::ExceptionType::LoadAddressMisaligned as usize, epc, addr])
        }
        RiscvException::LoadAccessFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::LoadAccessFault as usize, epc, addr])
        }
        RiscvException::StoreAddressMisaligned(epc, addr) => {
            Some([xous_kernel::ExceptionType::StoreAddressMisaligned as usize, epc, addr])
        }
        RiscvException::StoreAccessFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::StoreAccessFault as usize, epc, addr])
        }
        RiscvException::InstructionPageFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::InstructionPageFault as usize, epc, addr])
        }
        RiscvException::LoadPageFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::LoadPageFault as usize, epc, addr])
        }
        RiscvException::StorePageFault(epc, addr) => {
            Some([xous_kernel::ExceptionType::StorePageFault as usize, epc, addr])
        }
        _ => None,
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
    if cfg!(any(target_arch = "riscv32", target_arch = "riscv64")) && sstatus::read().spp() == sstatus::SPP::Supervisor && sc.bits() == 0xf
    {
        let pid = current_pid();
        let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
        MemoryMapping::current().print_map();
        panic!("KERNEL({}): RISC-V fault: {} - maybe ran out of kernel stack?", pid, ex);
    }

    let pid = current_pid();
    let epc = sepc::read();

    let ex = RiscvException::from_regs(sc.bits(), epc, stval::read());
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
        // Syscall
        RiscvException::CallFromSMode(_epc, _) | RiscvException::CallFromUMode(_epc, _) => {
            // We got here because of an `ecall` instruction, either from User mode (sc==8)
            // or from Supervisor mode (sc==9).  When we return, skip past the `ecall`
            // instruction.
            // If this is a call such as `SwitchTo`, then we will want to adjust the return
            // value of the current process prior to performing the switch in order to
            // avoid constantly executing the same instruction.
            let tid = ArchProcess::with_current_mut(|p| {
                p.current_thread_mut().sepc += 4;
                p.current_tid()
            });
            // A Redoubt call (redoubt-sys): its numbers start above every legacy one.
            if a0 >= redoubt_sys::NUMBER_BASE as usize {
                let regs = [a0, a1, a2, a3, a4, a5, a6, a7].map(|r| r as u64);
                let in_irq = PREVIOUS_PAIR.with(|p| p.is_some());
                match crate::redoubt::handle(pid, tid, in_irq, &regs) {
                    // Every result register holds at most 32 bits or one `usize` (redoubt-sys).
                    crate::redoubt::Outcome::Return(out) => ArchProcess::with_current_mut(|p| {
                        return_registers(&out.map(|r| r as usize), p.current_thread())
                    }),
                    crate::redoubt::Outcome::Resume => ArchProcess::with_current_mut(|p| {
                        crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
                    }),
                }
            }
            let call = SysCall::from_args(a0, a1, a2, a3, a4, a5, a6, a7).unwrap_or_else(|_| {
                ArchProcess::with_current_mut(|p| {
                    return_result(
                        &xous_kernel::Result::Error(xous_kernel::Error::UnhandledSyscall),
                        p.current_thread(),
                    )
                })
            });

            let response =
                crate::syscall::handle(pid, tid, PREVIOUS_PAIR.with(|p| p.is_some()), call)
                    .unwrap_or_else(xous_kernel::Result::Error);

            // println!("Syscall Result: {:?}", response);
            ArchProcess::with_current_mut(|p| {
                let thread = p.current_thread();
                // If we're resuming a process that was previously sleeping, restore the
                // thread context. Otherwise, keep the thread context the same and pass
                // the return values in 8 argument registers.
                if response == xous_kernel::Result::ResumeProcess {
                    crate::arch::syscall::resume(current_pid().get() == 1, thread);
                } else {
                    // println!("Returning to address {:08x}", thread.sepc);
                    return_result(&response, thread);
                }
            });
        }
        // Hardware interrupt
        RiscvException::UserExternalInterrupt(_)
        | RiscvException::SupervisorExternalInterrupt(_)
        | RiscvException::SupervisorTimerInterrupt(_) => {
            // The controller (or the timer) claims one interrupt; `None` is a spurious trap
            // with nothing pending, which we ignore and just resume from.
            #[cfg(feature = "sbi")]
            let pending = if let RiscvException::SupervisorTimerInterrupt(_) = ex {
                timer::on_interrupt();
                Some(timer::IRQ)
            } else {
                intc::pending()
            };
            #[cfg(not(feature = "sbi"))]
            let pending = intc::pending();

            if let Some(irq) = pending {
                // R5: an interrupt with a device object is the kernel's to record, not a
                // callback: it masks the source, sets `fired` and wakes whoever is in
                // `receive` on the handle. Nothing runs in userspace on the way, so there is
                // no ISR to return from and no pair to remember; completing the claim is all
                // that is left before resuming whatever was interrupted.
                if !crate::device::irq_fired(irq) {
                    // Remember who to resume once the userspace handler returns.
                    PREVIOUS_PAIR.with(|previous| {
                        if previous.is_none() {
                            *previous = Some((pid, crate::arch::process::current_tid()));
                        }
                    });
                    HANDLING_IRQ.store(true, Ordering::Relaxed);
                    crate::irq::handle(irq).expect("Couldn't handle IRQ");
                }
            }
            ArchProcess::with_current_mut(|process| {
                crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
            })
        }

        // See if it's a known exception, such as writing to a demand-paged area
        // or returning from a handler or thread. If so, handle the exception
        // and return right away.
        RiscvException::StorePageFault(_pc, addr) | RiscvException::LoadPageFault(_pc, addr) => {
            #[cfg(all(feature = "debug-print", feature = "print-panics"))]
            println!("KERNEL({}): RISC-V fault: {} @ {:08x}, addr {:08x} - ", pid, ex, _pc, addr);
            crate::mem::MemoryManager::with_mut(|mm| crate::arch::mem::ensure_page_exists_inner(mm, addr))
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

        RiscvException::InstructionPageFault(RETURN_FROM_EXCEPTION_HANDLER, _offset) => {
            // A process can branch here on purpose without ever having entered an
            // exception handler. Only perform the resume dance if the process was
            // genuinely in an Exception state; otherwise fall through to the
            // unhandled-fault path, which terminates just this process.
            if SystemServices::with_mut(|ss| ss.finish_exception_handler_and_resume(pid)).is_ok() {
                ArchProcess::with_current_mut(|p| {
                    let pc_adjust = a0 as isize;
                    if pc_adjust < 0 {
                        p.current_thread_mut().sepc -= pc_adjust.abs() as usize;
                    } else {
                        p.current_thread_mut().sepc += pc_adjust.abs() as usize;
                    }
                    crate::arch::syscall::resume(pid.get() == 1, p.current_thread());
                });
            }
            // On Err: do nothing here, let control reach the bottom-of-handler
            // containment that calls terminate_process(pid).
        }

        RiscvException::InstructionPageFault(EXIT_THREAD, _offset) => {
            let tid = ArchProcess::with_current(|process| process.current_tid());

            // This address indicates a thread has exited. Destroy the thread.
            // This activates another thread within this process.
            if SystemServices::with_mut(|ss| ss.destroy_thread(pid, tid)).unwrap() {
                crate::syscall::reset_switchto_caller();
            }

            // Now that the thread is destroyed, switch to a different process if
            // we're in an interrupt handler.
            finish_isr();

            // Resume the new thread within the same process.
            ArchProcess::with_current_mut(|p| {
                crate::arch::syscall::resume(current_pid().get() == 1, p.current_thread())
            });
        }

        RiscvException::InstructionPageFault(RETURN_FROM_ISR, _offset) => {
            finish_isr();
            ArchProcess::with_current_mut(|process| {
                crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
            });
        }

        // Handle faulted instruction pages, because we can now actually have instruction pages that are
        // swapped out.

        _ => {
            println!("!!! Unrecognized exception: {:x?}", ex);
        }
    }

    // This exception is not due to something we're aware of. In this case,
    // determine if there is an exception handler in this particular program
    // and call that handler if so.
    if let Some(args) = generate_exception_args(&ex) {
        if let Some(handler) = SystemServices::with_mut(|ss| ss.begin_exception_handler(pid)) {
            klog!("Exception handler for process exists ({:x?})", handler);
            // If this is the sort of exception that may be able to be handled by
            // the userspace program, generate a list of arguments to pass to
            // the handler.
            // Invoke the handler in userspace and exit this exception handler.
            klog!(
                "At start of exception, current thread was: {}",
                SystemServices::with(|ss| ss.get_process(pid).unwrap().current_thread)
            );
            ArchProcess::with_current_mut(|process| {
                crate::arch::syscall::invoke(
                    process.thread_mut(crate::arch::process::EXCEPTION_TID),
                    current_pid().get() == 1,
                    handler.pc,
                    handler.sp,
                    RETURN_FROM_EXCEPTION_HANDLER,
                    &args,
                );
                crate::arch::syscall::resume(
                    current_pid().get() == 1,
                    process.thread(crate::arch::process::EXCEPTION_TID),
                )
            });
        }
    }

    let is_kernel_failure = sstatus::read().spp() == sstatus::SPP::Supervisor;
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

    finish_isr();

    // If it's not a failure in the kernel, terminate or debug the current process.
    SystemServices::with_mut(|ss| {
        ss.terminate_process(pid).expect("couldn't terminate current process");
        crate::syscall::reset_switchto_caller();
    });

    // Resume the parent process.
    ArchProcess::with_current_mut(|process| {
        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
    })
}
