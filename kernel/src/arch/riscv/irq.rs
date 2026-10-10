// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use redoubt_layout::KERNEL_PID;
use riscv::register::{scause, sepc, sstatus, stval};

use crate::arch::current_pid;
use crate::arch::exception::RiscvException;
use crate::arch::mem::MemoryMapping;
use crate::arch::process::Process as ArchProcess;
use crate::arch::process::{EXIT_THREAD, Thread};
use crate::ptable::ProcessTable;

extern "Rust" {
    fn _redoubt_syscall_return_result(args: &[usize; 8], context: &Thread) -> !;
}

/// Resume `context` with `a0..=a7` = `args`.
fn return_registers(args: &[usize; 8], context: &Thread) -> ! {
    // Leaving the kernel: the scheduler's exit hook (`sched.rs`, accounting at the trap boundary).
    crate::sched::leave(current_pid());
    #[cfg(debug_assertions)]
    crate::arch::mem::audit::returning();
    #[cfg(debug_assertions)]
    assert!(!crate::arch::hart::shot_down(), "a hart shot down returns to user mode");
    // A system call returns to user mode: the last kernel work of this entry.
    crate::reclaim::end_section();
    crate::cell::KERNEL_LOCK.release();
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

/// A hart started after boot takes device interrupts on its PLIC context from now on (the
/// supervisor external interrupt is on in its `sie` since `timer::init_hart`).
pub fn online() {
    // Debug only, its negative case's: the other harts' contexts stay off.
    #[cfg(all(feature = "plic", not(feature = "irq-boot-hart-only")))]
    intc::online();
}

/// A checked build's account of the claims at `system_reset` (`hart::report`).
#[cfg(debug_assertions)]
pub fn report() {
    #[cfg(feature = "plic")]
    intc::report();
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
    ProcessTable::with_mut(|ss| crate::sched::preempt(ss, tid));
    resume_current()
}

/// A system call: every user-mode `ecall`, and the only decoder, `redoubt::handle` (redoubt-sys).
/// It never returns to the trap handler: the caller resumes past its `ecall` with the result,
/// or whatever is current runs.
fn system_call(pid: redoubt_layout::Pid, regs: [usize; 8]) -> ! {
    #[cfg(feature = "sum-probe")]
    sum_probe(sepc::read());
    let tid = ArchProcess::with_current_mut(|p| {
        p.current_thread_mut().sepc += 4;
        p.current_tid()
    });
    match crate::redoubt::handle(pid, tid, &regs.map(|r| r as u64)) {
        // Every result register holds at most 32 bits or one `usize` (redoubt-sys).
        crate::redoubt::Outcome::Return(out) => {
            ArchProcess::with_current_mut(|p| return_registers(&out.map(|r| r as usize), p.current_thread()))
        }
        crate::redoubt::Outcome::Resume => resume_current(),
    }
}

/// Test builds only: at the first system call, load the caller's `ecall` at `epc` straight
/// through its user mapping. With `sstatus.SUM` clear (R24) the load faults and the kernel stops
/// with a kernel failure; with it set, the load succeeds and the probe says so.
#[cfg(feature = "sum-probe")]
fn sum_probe(epc: usize) {
    use core::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    println!("sum-probe: loading {:#x} through the caller's user mapping", epc);
    // SAFETY: a test build's deliberate stray access. `epc` is the caller's `ecall`, in a live,
    // readable user mapping, 2-byte aligned, and nothing writes it meanwhile, so if the load
    // does not fault it reads an initialised `u16`.
    let half = unsafe { core::ptr::read_volatile(epc as *const u16) };
    println!("sum-probe: read {:#x} through a user mapping", half);
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
    // A trap from user mode takes the kernel lock, its registers saved; one from S-mode (`kmain`'s
    // switch, or an interrupt in its idle window) holds it already (cell.rs, `KERNEL_LOCK`).
    let from_user = sstatus::read().spp() == sstatus::SPP::User;
    if from_user {
        // The wait for the lock is the trace's (`Q`): from here, if another hart held it. A checked
        // build marks it, so that another hart's audit it waits through is not billed to it
        // (`sched::audit`).
        #[cfg(debug_assertions)]
        let came = riscv::register::time::read64();
        #[cfg(debug_assertions)]
        crate::sched::waiting(came);
        crate::arch::hart::serve();
        let (held, ticket, ahead) = crate::cell::KERNEL_LOCK.acquire_ticket();
        #[cfg(debug_assertions)]
        crate::sched::waited(held, came);
        #[cfg(feature = "sched-trace")]
        if held {
            crate::sched::trace::lock_wait(came, ticket, ahead);
        }
        #[cfg(not(feature = "sched-trace"))]
        let _ = (held, ticket, ahead);
        // The frames harts have zeroed go back to the bitmap (R81).
        crate::mem::entered();
        // Shot down while it ran here (`hart::shootdown`): its process was destroyed from another
        // hart, and its thread and context are gone. The hart's own mark says so, not the process
        // table, which may already hold a new process under the same PID. Nothing of the trap is
        // handled: the hart goes to `kmain`. What it ran is its budget's, if that lives on (a
        // sibling's exit ended the process, not a destruction of the budget).
        if crate::arch::hart::shot_down() {
            crate::sched::from_user();
            ProcessTable::with_mut(|ss| ss.switch_to_thread(KERNEL_PID, None)).expect("kmain exists");
            resume_current();
        }
    }
    // A trap in `kmain`'s idle window enters the kernel here (one from user mode, at
    // `sched::from_user`); `kmain`'s own switch is in it already.
    #[cfg(feature = "sched-trace")]
    if !from_user {
        crate::sched::trace::kernel_from(crate::sched::now_ticks());
    }
    let sc = scause::read();
    #[cfg(feature = "hold-trace")]
    crate::sched::trace::hold_cause(match (sc.is_interrupt(), from_user) {
        (true, _) => 0x200 + sc.code() as u64,
        (false, true) if sc.code() == 8 && (0x100..0x200).contains(&a0) => a0 as u64,
        (false, true) if sc.code() == 8 => 0x1ff,
        (false, _) => 0x300 + sc.code() as u64,
    });

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
    if !matches!(ex, RiscvException::StorePageFault(..) | RiscvException::LoadPageFault(..)) {
        crate::arch::mem::retry_reset();
    }

    // The user time since the last return is the running budget's (`sched.rs`).
    let timer = matches!(ex, RiscvException::SupervisorTimerInterrupt(_));
    let external = matches!(
        ex,
        RiscvException::UserExternalInterrupt(_) | RiscvException::SupervisorExternalInterrupt(_)
    );
    if from_user {
        crate::sched::from_user();
        #[cfg(feature = "sched-trace")]
        if timer {
            crate::sched::trace::timer_entry();
        } else if external {
            crate::sched::trace::external_entry();
        }
    }
    // Every entry but `kmain`'s switch answers the deadlines that have passed first
    // (`time.rs`), so a deadline beats anything that enters after it. If that ended the entering
    // process (a budget deadline), there is nothing of it left to handle: run what is current.
    // A budget deadline is a preemption point (R12): the entering thread yields the CPU before
    // anything else, and its trap is taken again when it next runs.
    let mut expired_last = None;
    if !matches!(ex, RiscvException::CallFromSMode(..)) {
        let expired = crate::time::expire_at_entry();
        expired_last = expired.last;
        // The rest of a timer interrupt that found something (an expired item, or a wait that
        // ended before its timeout) is the budget's billed last, the return included: the budget
        // it interrupted pays for none of it (kernel/scheduling.md, "Charging").
        if from_user && timer {
            crate::sched::bill_from_now(expired.last);
        }
        if from_user
            && (current_pid() != pid
                || ProcessTable::with(|ss| ss.get_process(pid).map_or(true, |p| p.free())))
        {
            resume_current();
        }
        if from_user && expired.destroyed {
            preempt();
        }
    }
    // From here, kernel time is the running budget's: a system call's is its caller's. A timer
    // interrupt's is not (below). Debug only, never in a bench build but one recorded negative run
    // (feature `timer-tail-billed`): it is, as it was before the fix. A device interrupt's is from
    // here once it claims a source; until then, and if another hart claimed it first, it is as a
    // timer interrupt's that ends no slice (below).
    let expiry_end = if external { crate::sched::now_ticks() } else { 0 };
    if from_user && !external && (!timer || cfg!(feature = "timer-tail-billed")) {
        crate::sched::begin_billing();
    }
    #[cfg(any(feature = "debug-print"))] // , feature = "debug-swap-verbose"
    {
        let pid = current_pid();
        let ex = RiscvException::from_regs(sc.bits(), sepc::read(), stval::read());
        let tid = ArchProcess::with_current(|p| p.current_tid());
        println!("IRQ ({}.{}): {} sepc {:x}", pid, tid, ex, sepc::read(),);
    }
    match ex {
        // `kmain`'s switch (`sched::switch_to`), the one S-mode `ecall`: resumed past it.
        RiscvException::CallFromSMode(..) => {
            ArchProcess::with_current_mut(|p| p.current_thread_mut().sepc += 4);
            ProcessTable::with_mut(|ss| crate::sched::switch(ss, a0, a1, a2));
            resume_current()
        }
        RiscvException::CallFromUMode(..) => system_call(pid, [a0, a1, a2, a3, a4, a5, a6, a7]),
        // The kernel's timer: what was due was answered at this entry; arm for what is next.
        RiscvException::SupervisorTimerInterrupt(_) => {
            // The running thread's slice is over: preempt it (R12). An entry that found nothing is
            // the running budget's when it ends its slice, and nobody's otherwise.
            let slice_over = from_user && crate::sched::slice_over();
            if slice_over && expired_last.is_none() && !cfg!(feature = "timer-tail-billed") {
                crate::sched::begin_billing();
            }
            crate::time::on_interrupt(from_user);
            if slice_over {
                preempt();
            }
            resume_current();
        }
        // Another hart's reschedule interrupt: a budget became runnable while this hart idled
        // (`hart::wake_idle`). `kmain` picks again when it is resumed.
        RiscvException::SupervisorSoftwareInterrupt(_) => {
            crate::arch::hart::ack_ipi();
            resume_current()
        }
        // Hardware interrupt
        RiscvException::UserExternalInterrupt(_) | RiscvException::SupervisorExternalInterrupt(_) => {
            // The controller claims one interrupt on this hart's context; `None` is a trap with
            // nothing pending here (another hart claimed it first), which we ignore and just
            // resume from, billing the interrupted budget nothing. Handling it is its device
            // owner's work, not the interrupted budget's (`sched::bill_irq`).
            let started = crate::sched::now_ticks();
            let pending = intc::pending();
            #[cfg(feature = "sched-trace")]
            crate::sched::trace::claimed(pending);

            if let Some(irq) = pending {
                if from_user {
                    crate::sched::begin_billing_from(expiry_end);
                }
                #[cfg(debug_assertions)]
                crate::sched::irq_audits_open();
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
            // A translation already valid that allows the access: another hart changed it and this
            // one cached the old entry. Flush that address in this ASID and retry, once. Only a
            // fault from user mode: the kernel never reaches a user mapping (R24), so its own fault
            // on one is a kernel failure, and `resume_current` would resume the thread, not it.
            if from_user
                && crate::arch::mem::retry_stale(addr, matches!(ex, RiscvException::StorePageFault(..)))
            {
                resume_current();
            }
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
                        pid,
                        process.current_tid(),
                        process.current_thread().sepc,
                    );
                    crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
                });
            })
            .ok(); // If this fails, fall through.
        }

        RiscvException::InstructionPageFault(EXIT_THREAD, _offset) => {
            let tid = ArchProcess::with_current(|process| process.current_tid());
            // Ordinary thread returns use the same lifecycle policy as explicit thread_exit:
            // the final return snapshots open calls/blame before process_exit(0) cleanup (170).
            ProcessTable::with_mut(|ss| crate::process::thread_exit(ss, pid, tid));

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
    // The thread's registers and the address space's map are a kernel failure's diagnosis, and a
    // debug build's. A program's fault prints its one line: the report is printed holding the
    // kernel lock, and the map is a line a mapped page, so it would hold every other hart for as
    // long as the faulting process is large (R12: a call's kernel time follows what it may cost;
    // kernel/scheduling.md, "Fair kernel entry is bounded by count").
    if is_kernel_failure || cfg!(feature = "debug-print") {
        ArchProcess::with_current(|process| {
            println!("Current thread {}:", process.current_tid());
            process.print_current_thread();
        });
        MemoryMapping::current().print_map();
    }

    // If this is a failure in the kernel, go into an infinite loop
    if is_kernel_failure {
        #[allow(clippy::empty_loop)]
        loop {}
    }

    // If it's not a failure in the kernel, the process faults: it is torn down and its exit
    // notice, cause `faulted`, blames the sender of the faulting thread's current call
    // (kernel/processes.md R21; `process.rs`). The code is the RISC-V exception cause.
    crate::process::faulted(pid, (sc.bits() & 0xff) as u32);

    // Resume the parent process.
    ArchProcess::with_current_mut(|process| {
        crate::arch::syscall::resume(current_pid().get() == 1, process.current_thread())
    })
}
