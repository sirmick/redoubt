// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[macro_use]
mod debug;

#[cfg(all(test, not(baremetal)))]
mod test;

mod arch;

#[macro_use]
mod args;
#[cfg(baremetal)]
mod budget;
mod cell;
#[cfg(baremetal)]
mod device;
#[cfg(baremetal)]
mod dma;
mod endpoint;
#[cfg(baremetal)]
mod grants;
#[cfg(baremetal)]
mod handle;
mod io;
mod irq;
#[cfg(baremetal)]
mod kframe;
mod macros;
mod mem;
mod message;
mod platform;
#[cfg(baremetal)]
mod process;
#[cfg(baremetal)]
mod redoubt;
mod server;
mod services;
mod syscall;
#[cfg(baremetal)]
mod sched;
#[cfg(baremetal)]
mod time;

#[cfg(not(baremetal))]
use redoubt_abi::*;
use services::SystemServices;

#[cfg(baremetal)]
#[no_mangle]
/// This function is called from baremetal startup code to initialize various kernel structures
/// based on arguments passed by the bootloader. It is unused when running under an operating system.
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

/// Loop through the SystemServices list to determine the next PID to be run (hosted builds;
/// on the machine, `sched.rs` picks). If no process is ready, return `None`.
#[cfg(not(baremetal))]
fn next_pid_to_run(last_pid: Option<PID>) -> Option<PID> {
    // PIDs are 1-indexed but arrays are 0-indexed.  By not subtracting
    // 1 from the PID when we use it as an array index, we automatically
    // pick the next process in the list.
    let next_pid = last_pid.map(|v| v.get() as usize).unwrap_or(1);

    SystemServices::with(|system_services| {
        for process in
            system_services.processes[next_pid..].iter().chain(system_services.processes[..next_pid].iter())
        {
            if process.runnable() {
                return Some(process.pid);
            }
        }
        None
    })
}

/// Common main function for baremetal and hosted environments.
#[no_mangle]
pub extern "C" fn kmain() {
    // SMP bring-up spike: start a second hart and validate the spinlock big-kernel-lock
    // under real cross-hart contention before entering the scheduler. See arch/riscv/smp.rs.
    #[cfg(all(baremetal, feature = "smp", feature = "sbi"))]
    crate::arch::smp::run();

    // Hosted builds round-robin by PID; on the machine, `sched.rs` picks.
    #[cfg(not(baremetal))]
    let mut pid = None;

    #[cfg(not(any(baremetal, all(ci, test))))]
    {
        use std::panic;
        panic::set_hook(Box::new(|arg| {
            println!("Panic Details: {:?}", arg);
            // debug_here::debug_here!();
        }));
    }

    // The loader wrote every boot program's image: make instruction fetch see it before the
    // first of them runs (`fence.i`; every later executable page is fenced as it is mapped).
    #[cfg(all(baremetal, any(target_arch = "riscv32", target_arch = "riscv64")))]
    crate::arch::mem::sync_icache();

    loop {
        // Deadlines first (`time.rs`): answering them makes threads runnable. This is the kernel's
        // own loop, not an entry; nothing enters between here and the switch below.
        #[cfg(baremetal)]
        {
            crate::sched::pause_billing();
            SystemServices::with_mut(crate::time::expire_due);
            crate::sched::resume_billing();
        }

        // One stride queue over every runnable budget (`sched.rs`).
        #[cfg(baremetal)]
        let next = SystemServices::with(|ss| mem::MemoryManager::with_mut(|mm| crate::sched::pick(ss, mm)));
        #[cfg(not(baremetal))]
        let next = {
            pid = next_pid_to_run(pid);
            pid.map(|p| (p, 0))
        };

        match next {
            Some((pid, tid)) => {
                #[cfg(feature = "debug-print")]
                println!("  ->PID{:?}:{}", pid, tid); // keep this succinct as it happens often
                #[cfg(all(baremetal, any(target_arch = "riscv32", target_arch = "riscv64")))]
                use arch::syscall::kernel_syscall;
                #[cfg(not(all(baremetal, any(target_arch = "riscv32", target_arch = "riscv64"))))]
                use redoubt_abi::rsyscall as kernel_syscall;
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
                #[cfg(baremetal)]
                crate::sched::stop_billing();
                if !arch::idle() {
                    return;
                }
            }
        }
    }
}

/// The main entrypoint when run in hosted mode. When running in embedded mode,
/// this function does not exist.
#[cfg(not(baremetal))]
fn main() { kmain(); }
