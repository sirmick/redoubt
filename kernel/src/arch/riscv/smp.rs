// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two-hart bring-up spike (feature `smp`). Validates the load-bearing SMP assumption end
//! to end on real hardware: a second hart, started through SBI HSM, runs kernel code and
//! contends with the boot hart on a spinlock-backed `KernelCell` (cell.rs) without losing
//! updates. It does **not** run the full kernel on two harts (that needs per-hart trap
//! state); it exercises exactly the lock the big-kernel-lock plan rests on.
//!
//! The secondary starts at the trampoline's *physical* address with the MMU off. The
//! trampoline is position-independent (only `a1`-relative loads), loads `satp`, `sp` and
//! the virtual entry from the physical block in `a1`, and turns paging on with the same
//! `stvec`-trap handoff the loader uses: after `csrw satp` the next physical fetch faults
//! and traps straight to the virtual entry.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::cell::KernelCell;

/// Critical sections each hart runs. Large enough that the two harts interleave heavily.
const ITERS: usize = 50_000;

/// The shared state both harts hammer through the spinlock. If the lock is sound the final
/// value is exactly `2 * ITERS`; a broken lock loses updates.
static COUNTER: KernelCell<usize> = KernelCell::new(0);
static SECONDARY_DONE: AtomicBool = AtomicBool::new(false);
static SECONDARY_HARTID: AtomicUsize = AtomicUsize::new(usize::MAX);

/// What the trampoline hands the secondary hart. `repr(C)`, read field-by-field with the
/// MMU off, so the offsets must be stable: `satp` at 0, `sp` at one word, `entry` at two.
#[repr(C)]
struct HartBlock {
    satp: usize,
    sp: usize,
    entry: usize,
}

/// A `Sync` cell for the block: only the boot hart writes it (once, before the secondary is
/// started), and the secondary reads it after. A `SeqCst` fence pairs the two.
struct BlockCell(UnsafeCell<HartBlock>);
// SAFETY: written once by the boot hart before `hart_start`, read once by the secondary
// after it starts; the fence in `run` establishes the happens-before.
unsafe impl Sync for BlockCell {}
static BLOCK: BlockCell = BlockCell(UnsafeCell::new(HartBlock { satp: 0, sp: 0, entry: 0 }));

/// The secondary hart's stack, in the kernel image so it is mapped in every address space.
/// `static mut` so it lands in writable `.bss`: an immutable `static` goes to `.rodata`, which
/// the kernel maps read-only, and the first spill would fault. Only its address is taken here;
/// the secondary hart is its sole user.
const STACK_WORDS: usize = 1024;
/// 16-byte aligned, as the RISC-V psABI requires of `sp`.
#[repr(align(16))]
struct Stack([usize; STACK_WORDS]);
static mut SECONDARY_STACK: Stack = Stack([0; STACK_WORDS]);

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    r#"
    .section .text
    .global _smp_secondary_start
_smp_secondary_start:              // a0 = hartid, a1 = &HartBlock (physical, MMU off)
    ld      t0, 0(a1)              // satp
    ld      sp, 8(a1)              // sp (virtual; only used after paging is on)
    ld      t1, 16(a1)             // entry (virtual)
    csrw    stvec, t1
    sfence.vma
    csrw    satp, t0               // paging on; the next physical fetch faults to stvec
    unimp
"#
);
#[cfg(target_arch = "riscv32")]
core::arch::global_asm!(
    r#"
    .section .text
    .global _smp_secondary_start
_smp_secondary_start:              // a0 = hartid, a1 = &HartBlock (physical, MMU off)
    lw      t0, 0(a1)              // satp
    lw      sp, 4(a1)              // sp
    lw      t1, 8(a1)              // entry
    csrw    stvec, t1
    sfence.vma
    csrw    satp, t0               // paging on; the next physical fetch faults to stvec
    unimp
"#
);

// Where the secondary lands once paging is on, and the same for both widths: `stvec`'s low
// two bits are its mode, so its target must be 4-byte aligned, which a Rust function under
// the C extension is not.
core::arch::global_asm!(
    r#"
    .section .text
    .global _smp_secondary_land
    .balign 4
_smp_secondary_land:               // virtual, MMU on; a0 = hartid still
    tail    {main}
"#,
    main = sym secondary_main
);

extern "C" {
    fn _smp_secondary_start();
    fn _smp_secondary_land();
}

/// The secondary hart lands here, through `_smp_secondary_land` (virtual, MMU on, interrupts
/// off, `sp` set).
extern "C" fn secondary_main(_hartid: usize) -> ! {
    for _ in 0..ITERS {
        COUNTER.with(|c| *c += 1);
    }
    SECONDARY_DONE.store(true, Ordering::Release);
    // Nothing further for it to do: park. (Real SMP would enter the scheduler here.)
    loop {
        // SAFETY: `wfi` has no memory effect. (`unsafe` on the vendored rv32 riscv crate,
        // a safe no-op wrapper on the rv64 one.)
        #[allow(unused_unsafe)]
        unsafe {
            riscv::asm::wfi()
        };
    }
}

/// Run the spike on the boot hart, after the console and paging are up. Prints one line.
pub fn run() {
    let satp = riscv::register::satp::read().bits();
    // The top of the stack, less one 16-byte slot, as the loader leaves for a user thread.
    // `Stack` is 16-aligned and a whole number of slots, so `sp` is too.
    let sp = &raw const SECONDARY_STACK as usize + core::mem::size_of::<Stack>() - 16;
    // Not `secondary_main` itself: a Rust function is only 2-aligned under the C extension.
    let entry = _smp_secondary_land as *const () as usize;

    // SAFETY: sole writer; the fence below publishes it before the secondary can read.
    unsafe { *BLOCK.0.get() = HartBlock { satp, sp, entry } };
    core::sync::atomic::fence(Ordering::SeqCst);

    // `virt_to_phys` returns the page-aligned frame base, so add the page offset back to get
    // the exact physical address of the block and the trampoline entry.
    const OFFSET: usize = xous_kernel::arch::PAGE_SIZE - 1;
    let block_virt = BLOCK.0.get() as usize;
    let tramp_virt = _smp_secondary_start as *const () as usize;
    let block_phys = match crate::arch::mem::virt_to_phys(block_virt) {
        Ok(p) => p + (block_virt & OFFSET),
        Err(_) => {
            println!("SMP SPIKE: could not translate the hart block");
            return;
        }
    };
    let tramp_phys = match crate::arch::mem::virt_to_phys(tramp_virt) {
        Ok(p) => p + (tramp_virt & OFFSET),
        Err(_) => {
            println!("SMP SPIKE: could not translate the trampoline");
            return;
        }
    };

    // Start the first hart that will start: not us (already running) and one that exists.
    for id in 0..8 {
        if sbi_rt::hart_start(id, tramp_phys, block_phys).is_ok() {
            SECONDARY_HARTID.store(id, Ordering::Relaxed);
            break;
        }
    }
    let started = SECONDARY_HARTID.load(Ordering::Relaxed);
    if started == usize::MAX {
        println!("SMP SPIKE: no secondary hart could be started");
        return;
    }

    // Both harts hammer the shared counter through the spinlock.
    for _ in 0..ITERS {
        COUNTER.with(|c| *c += 1);
    }
    // Bounded wait so a wedged secondary reports instead of hanging the boot.
    let mut spins: u64 = 0;
    while !SECONDARY_DONE.load(Ordering::Acquire) {
        core::hint::spin_loop();
        spins += 1;
        if spins > 200_000_000 {
            println!("SMP SPIKE: secondary hart {} wedged (counter so far {})", started, COUNTER.with(|c| *c));
            return;
        }
    }

    let total = COUNTER.with(|c| *c);
    let expected = 2 * ITERS;
    println!(
        "SMP SPIKE: hart {} joined; counter {} (expected {}) {}",
        started,
        total,
        expected,
        if total == expected { "OK" } else { "MISMATCH" }
    );
}
