// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#![no_main]
#![no_std]

#[macro_use]
mod debug;

mod arch;

#[macro_use]
mod args;
mod budget;
mod cell;
mod device;
mod dma;
mod endpoint;
mod grants;
mod handle;
mod io;
mod irq;
mod kframe;
mod mem;
mod message;
mod platform;
mod process;
mod redoubt;
mod services;
mod syscall;
mod sched;
mod time;

use services::SystemServices;

#[no_mangle]
/// Called from the startup code to initialize the kernel's structures from the arguments the
/// loader passed.
///
/// # Safety
///
/// This is safe to call only to initialize the kernel.
pub unsafe extern "C" fn init(
    arg_offset: *const u32,
    init_offset: *const u32,
    rpt_offset: usize,
    xpt_offset: usize,
) {
    args::KernelArguments::init(arg_offset);
    platform::early_init();
    let args = args::KernelArguments::get();
    // Everything needs memory, so the first thing we should do is initialize the memory manager.
    crate::mem::MemoryManager::with_mut(|mm| {
        mm.init_from_memory(rpt_offset, xpt_offset, &args).expect("couldn't initialize memory manager")
    });
    SystemServices::with_mut(|system_services| system_services.init_from_memory(init_offset, &args));

    // Test builds only: the scheduling trace's ring, before the budget tree counts free RAM.
    #[cfg(feature = "sched-trace")]
    crate::mem::MemoryManager::with_mut(crate::sched::trace::init);

    // The budget tree, with the loader's processes in `system` (budget.rs, `boot_budgets`).
    crate::mem::MemoryManager::with_mut(|mm| mm.boot_budgets());

    // Now that the memory manager is set up, perform any architecture and
    // platform specific initializations.
    arch::init();
    platform::init();

    println!("KMAIN (clean boot): Supervisor mode started...");
    if cfg!(debug_assertions) {
        // The bench's `debug_assertions` cases expect this line, so a build that silently
        // lost the checks fails instead of passing quietly (`bench-debug-assertions`).
        println!("kernel: checks on (debug assertions, overflow checks)");
    }

    // rand::init() already clears the initial pipe, but pump the TRNG a little more out of no other reason
    // than sheer paranoia
    platform::rand::get_u32();
    platform::rand::get_u32();
}

/// The kernel's main loop, entered once `init` has run.
#[no_mangle]
pub extern "C" fn kmain() {
    // SMP bring-up spike: start a second hart and validate the spinlock big-kernel-lock
    // under real cross-hart contention before entering the scheduler. See arch/riscv/smp.rs.
    #[cfg(all(feature = "smp", feature = "sbi"))]
    crate::arch::smp::run();

    // The loader wrote every boot program's image: make instruction fetch see it before the
    // first of them runs (`fence.i`; every later executable page is fenced as it is mapped).
    crate::arch::mem::sync_icache();

    loop {
        // Deadlines first (`time.rs`): answering them makes threads runnable. This is the kernel's
        // own loop, not an entry; nothing enters between here and the switch below.
        crate::sched::pause_billing();
        SystemServices::with_mut(crate::time::expire_due);
        crate::sched::resume_billing();

        // One stride queue over every runnable budget (`sched.rs`).
        let next = SystemServices::with(|ss| mem::MemoryManager::with_mut(|mm| crate::sched::pick(ss, mm)));

        match next {
            Some((pid, tid)) => {
                #[cfg(feature = "debug-print")]
                println!("  ->PID{:?}:{}", pid, tid); // keep this succinct as it happens often
                use arch::syscall::kernel_syscall;
                // A process that cannot be switched to (it died since it was picked) is simply
                // not run: pick again.
                if kernel_syscall(redoubt_abi::SysCall::SwitchTo(pid, tid)).is_err() {
                    continue;
                }
            }
            None => {
                #[cfg(feature = "debug-print")]
                klog!("NO RUNNABLE TASKS FOUND, entering idle state");

                #[cfg(feature = "debug-print")]
                SystemServices::with(|system_services| {
                    for (test_idx, process) in system_services.processes.iter().enumerate() {
                        if !process.free() {
                            klog!("PID {}: {:?}", test_idx + 1, process);
                        }
                    }
                });

                // Sleep until an interrupt: a device, or the timer, which is always armed for the
                // next deadline (`time.rs`). A deadline that passed since the check above is
                // already pending, so `wfi` returns at once.
                crate::sched::stop_billing();
                if !arch::idle() {
                    return;
                }
            }
        }
    }
}
