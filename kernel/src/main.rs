// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#![no_main]
#![no_std]

// `kmain`'s switch enters the trap handler as an S-mode `ecall` would (`sched::switch_to`); only
// SBI firmware leaves S-mode `ecall` to the kernel's own design.
#[cfg(not(feature = "sbi"))]
compile_error!("the kernel runs under SBI firmware: enable the `sbi` feature (or `qemu-virt`)");

// Test-only features build only into a checked kernel, which a shipped one (`./build`, release)
// is not; each implies `test-only`, and the bench builds every case that turns one on with debug
// assertions (R23).
#[cfg(all(feature = "test-only", not(debug_assertions)))]
compile_error!("a test-only kernel feature needs a checked build (debug assertions on)");

#[macro_use]
mod debug;

mod arch;

#[macro_use]
mod args;
mod bits;
mod budget;
mod cell;
mod device;
mod dma;
mod endpoint;
mod handle;
mod io;
mod kframe;
mod mem;
mod message;
mod platform;
mod process;
mod ptable;
mod redoubt;
mod sched;
mod time;

use ptable::ProcessTable;

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
    // The boot hart holds the kernel lock from here to its first return to user mode or idle;
    // the harts it starts below wait for it (cell.rs).
    arch::hart::init_boot();
    cell::KERNEL_LOCK.acquire();
    args::KernelArguments::init(arg_offset);
    platform::early_init();
    // Before anything else writes `satp` (kernel/boot.md, "Hardware bounds").
    arch::process::check_asid_field();
    let args = args::KernelArguments::get();
    // A process reaches a device only through its device handle: a boot whose arguments still
    // carry a device grant (`Grnt`) is refused rather than run as if it granted something.
    assert!(args.iter().all(|arg| arg.name != u32::from_le_bytes(*b"Grnt")), "a Grnt boot argument");
    // Everything needs memory, so the first thing we should do is initialize the memory manager.
    crate::mem::MemoryManager::with_mut(|mm| {
        mm.init_from_memory(rpt_offset, xpt_offset, &args).expect("couldn't initialize memory manager")
    });
    ProcessTable::with_mut(|pt| pt.init_from_memory(init_offset));

    // The other harts' stacks, before the budget tree counts free RAM (arch/riscv/hart.rs).
    crate::mem::MemoryManager::with_mut(arch::hart::map_stacks);
    // Test builds only: the scheduling trace's ring, before the budget tree counts free RAM.
    #[cfg(feature = "sched-trace")]
    crate::mem::MemoryManager::with_mut(crate::sched::trace::init);

    // The budget tree, with `init` in `root` (budget.rs, `boot_budgets`). `init`'s header page
    // names its threads' IPC pages, so the tree records it with `init`'s account.
    let init_header = ProcessTable::with(|pt| pt.mapping_of(crate::budget::INIT_PID))
        .and_then(|space| arch::mem::header_phys(&space))
        .expect("boot: init has no header page");
    crate::mem::MemoryManager::with_mut(|mm| mm.boot_budgets(init_header));

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
    // Debug only: the negative case's broken floor (the stride crate's `capped-holds-floor`) says
    // so, and `sched-capped-holds-floor` expects the line, so its failure is this kernel's.
    #[cfg(feature = "sched-capped-holds-floor")]
    println!("kernel: sched-capped-holds-floor: the floor counts the capped budgets");
    // Debug only: the negative case's kernel says so, as above (`irq-boot-hart-only`).
    #[cfg(feature = "irq-boot-hart-only")]
    println!("kernel: irq-boot-hart-only: device interrupts reach the boot hart alone");
    // Test builds only: a print, then a panic, inside `print!` (debug/console.rs).
    #[cfg(feature = "panic-in-print")]
    {
        struct Panics;
        impl core::fmt::Display for Panics {
            fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
                println!("a Display printed inside print!");
                panic!("a Display panicked inside print!")
            }
        }
        println!("panic-in-print: {}", Panics);
    }

    // rand::init() already clears the initial pipe, but pump the TRNG a little more out of no other reason
    // than sheer paranoia
    platform::rand::get_u32();
    platform::rand::get_u32();
    // The other harts, each into `kmain` once it holds the lock (arch/riscv/hart.rs).
    arch::hart::start_others();
}

/// The kernel's main loop, on every hart: entered once `init` has run, and by each hart it
/// started (`arch::hart`), holding the kernel lock.
#[no_mangle]
pub extern "C" fn kmain() {
    // The loader wrote every boot program's image: make instruction fetch see it before the
    // first of them runs (`fence.i`; every later executable page is fenced as it is mapped).
    crate::arch::mem::sync_icache();

    loop {
        // Deadlines first (`time.rs`): answering them makes threads runnable. This is the kernel's
        // own loop, not an entry; nothing enters between here and the switch below.
        crate::sched::pause_billing();
        ProcessTable::with_mut(crate::time::expire_due);
        crate::sched::resume_billing();

        // One stride queue over every runnable budget (`sched.rs`).
        let next = ProcessTable::with(|ss| mem::MemoryManager::with_mut(|mm| crate::sched::pick(ss, mm)));

        match next {
            Some((pid, tid)) => {
                #[cfg(feature = "debug-print")]
                println!("  ->PID{:?}:{}", pid, tid); // keep this succinct as it happens often
                // A process that cannot be switched to (it died since it was picked) is simply
                // not run: pick again.
                if crate::sched::switch_to(pid, tid).is_err() {
                    continue;
                }
            }
            None => {
                #[cfg(feature = "debug-print")]
                klog!("NO RUNNABLE TASKS FOUND, entering idle state");

                #[cfg(feature = "debug-print")]
                ProcessTable::with(|pt| {
                    for (test_idx, process) in pt.processes.iter().enumerate() {
                        if !process.free() {
                            klog!("PID {}: {:?}", test_idx + 1, process);
                        }
                    }
                });

                // Sleep until an interrupt: a device, or the timer, which is always armed for the
                // next deadline (`time.rs`). A deadline that passed since the check above is
                // already pending, so `wfi` returns at once.
                // A checked build's full audit of the IPC lists first (`message::check_all`).
                #[cfg(debug_assertions)]
                crate::sched::audit(crate::sched::AUDIT_IPC_LISTS, || {
                    crate::mem::MemoryManager::with(crate::message::check_all)
                });
                crate::sched::stop_billing();
                if !arch::idle() {
                    return;
                }
            }
        }
    }
}
