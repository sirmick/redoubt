// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Kernel entry, trap entry and context restore, for both rv64 (Sv39) and rv32 (Sv32).
//!
//! Ported from the original `asm.S` to `global_asm!` so the build needs no C toolchain and
//! no prebuilt blobs. The two widths differ only mechanically: a saved context is
//! `32 x size_of::<usize>()` bytes (256 on rv64, 128 on rv32), at the address the hart's block
//! holds (`hart.rs`, reached through `sscratch`); the load/store width and the reservation-clear
//! instruction change with the register width. All of that is confined to the small,
//! `cfg`-gated preamble below; the entry paths are shared. Addresses come from
//! `redoubt_layout` rather than being repeated as literals. There is no suspend/resume
//! entry path (Redoubt has no low-power suspend).

use core::arch::global_asm;

use super::hart;

// Width-specific macros. The register width sets the load/store and the reservation clear;
// everything else is shared below. These persist into the following `global_asm!` block.
#[cfg(target_arch = "riscv64")]
global_asm!(
    r#"
.macro SAVE reg, slot
    sd      \reg, 8*\slot(sp)
.endm
.macro RESTORE reg, slot
    ld      \reg, 8*\slot(sp)
.endm
.macro LOADR reg, slot, base
    ld      \reg, 8*\slot(\base)
.endm
.macro CLEAR_RESERVATION
    sc.d    zero, x1, (sp)
.endm
"#
);
#[cfg(target_arch = "riscv32")]
global_asm!(
    r#"
.macro SAVE reg, slot
    sw      \reg, 4*\slot(sp)
.endm
.macro RESTORE reg, slot
    lw      \reg, 4*\slot(sp)
.endm
.macro LOADR reg, slot, base
    lw      \reg, 4*\slot(\base)
.endm
.macro CLEAR_RESERVATION
    sc.w    zero, x1, (sp)
.endm
"#
);

global_asm!(
    r#"
// Module-level assembly does not inherit the target's features under LTO.
.option arch, +a, +c

// Slot N holds register x(N+1): slot 0 = x1 (ra), slot 1 = x2 (sp), ..., slot 30 = x31.
// Slot 31 holds sepc. x1 and x2 need special handling and are not covered here.
.macro SAVE_X3_TO_X31
    SAVE x3, 2
    SAVE x4, 3
    SAVE x5, 4
    SAVE x6, 5
    SAVE x7, 6
    SAVE x8, 7
    SAVE x9, 8
    SAVE x10, 9
    SAVE x11, 10
    SAVE x12, 11
    SAVE x13, 12
    SAVE x14, 13
    SAVE x15, 14
    SAVE x16, 15
    SAVE x17, 16
    SAVE x18, 17
    SAVE x19, 18
    SAVE x20, 19
    SAVE x21, 20
    SAVE x22, 21
    SAVE x23, 22
    SAVE x24, 23
    SAVE x25, 24
    SAVE x26, 25
    SAVE x27, 26
    SAVE x28, 27
    SAVE x29, 28
    SAVE x30, 29
    SAVE x31, 30
.endm

.macro RESTORE_X3_TO_X9
    RESTORE x3, 2
    RESTORE x4, 3
    RESTORE x5, 4
    RESTORE x6, 5
    RESTORE x7, 6
    RESTORE x8, 7
    RESTORE x9, 8
.endm

.macro RESTORE_X10_TO_X17
    RESTORE x10, 9
    RESTORE x11, 10
    RESTORE x12, 11
    RESTORE x13, 12
    RESTORE x14, 13
    RESTORE x15, 14
    RESTORE x16, 15
    RESTORE x17, 16
.endm

.macro RESTORE_X18_TO_X31
    RESTORE x18, 17
    RESTORE x19, 18
    RESTORE x20, 19
    RESTORE x21, 20
    RESTORE x22, 21
    RESTORE x23, 22
    RESTORE x24, 23
    RESTORE x25, 24
    RESTORE x26, 25
    RESTORE x27, 26
    RESTORE x28, 27
    RESTORE x29, 28
    RESTORE x30, 29
    RESTORE x31, 30
.endm

/*
    Kernel entry point. The loader jumps here in S-mode with the MMU on, `sp` at the top
    of the kernel stack, and the kernel arguments in a0-a3.
*/
.section .text.init, "ax"
.global _start
_start:
    la      t0, _start_trap
    csrw    stvec, t0
    // sstatus.SUM (bit 18) and MXR (bit 19) clear, whatever the firmware left: the kernel
    // never loads or stores through a user mapping (R24), and nothing sets either again.
    li      t0, (1 << 18) | (1 << 19)
    csrc    sstatus, t0
    // senvcfg 0, whatever the firmware or the reset left: every cache-block operation traps in
    // user mode, so no process can discard the zeroes the kernel wrote (R11). CSR 0x10a, named
    // by number for assemblers that predate it; a hart without it (privileged spec before 1.12)
    // traps here and the boot stops.
    csrw    0x10a, zero
    // The boot hart's block (`hart.rs`), which the trap entry and every per-hart read find
    // through `sscratch`.
    la      t0, HART_BLOCKS
    csrw    sscratch, t0
    call    init
.if {plant_senvcfg}
    // The `plant-senvcfg` test build: user cbo.inval flushes, cbo.clean, cbo.flush and cbo.zero
    // run, after `init` has checked the 0 (bench-cbo-self-unrefused).
    li      t0, (0b01 << 4) | (1 << 6) | (1 << 7)
    csrw    0x10a, t0
.endif
    j       kmain

/*
    Trap entry point. Saves the full context of the interrupted thread at the address the hart's
    block names (in the thread's IPC page, or the block's own area for the kernel's thread),
    switches to the hart's trap stack and enters Rust. `sscratch` holds the block throughout,
    but for the swap at the top, which the end undoes.
*/
.section .trap, "ax"
.global _start_trap
.balign 4
_start_trap:
    csrrw   sp, sscratch, sp        // sp = the hart's block, sscratch = the interrupted sp
    SAVE    x1, {scratch}           // Stash x1 in the block's scratch word
    mv      x1, sp                  // x1 = the block
    LOADR   sp, {context}, x1       // sp = the running thread's context

    SAVE_X3_TO_X31

    csrr    t0, sepc
    SAVE    t0, 31

    // Save the real x1, which was stashed in the block
    LOADR   t1, {scratch}, x1
    SAVE    t1, 0

    // Save the real sp, and give `sscratch` the block again
    csrrw   t0, sscratch, x1
    SAVE    t0, 1

    // Note that a0-a7 still contain the syscall arguments
    LOADR   sp, {trap_sp}, x1
    j       _start_trap_rust

/*
    Resume the context pointed to by a0. sepc and sstatus must already be set.
*/
.global _redoubt_resume_context
_redoubt_resume_context:
    mv      sp, a0
    RESTORE x1, 0
    CLEAR_RESERVATION
    RESTORE_X3_TO_X9
    RESTORE_X10_TO_X17
    RESTORE_X18_TO_X31
    RESTORE x2, 1
    sret

/*
    Return from a syscall. Redoubt returns values in a0-a7, but the C calling convention
    only gives us two return registers, so a0 points at an 8-word result block that is
    unpacked into the argument registers. a1 is the context to resume.
*/
.global _redoubt_syscall_return_result
_redoubt_syscall_return_result:
    mv      sp, a1
    RESTORE t0, 31
    csrw    sepc, t0

    RESTORE x1, 0
    CLEAR_RESERVATION
    RESTORE_X3_TO_X9

    LOADR   a7, 7, a0
    LOADR   a6, 6, a0
    LOADR   a5, 5, a0
    LOADR   a4, 4, a0
    LOADR   a3, 3, a0
    LOADR   a2, 2, a0
    LOADR   a1, 1, a0
    LOADR   a0, 0, a0

    RESTORE_X18_TO_X31
    RESTORE x2, 1
    sret

/*
    The flushes (arch/riscv/mem.rs, `flush`; kernel/memory-layout.md, "`satp`"). An ASID in a
    register is that ASID, 0 included; only `x0` stands for every ASID, so the global form has
    its own name.
*/
.global flush_mmu
flush_mmu:
    sfence.vma
    ret

.global flush_asid
flush_asid:
    sfence.vma zero, a0
    ret

.global flush_page
flush_page:
    sfence.vma a0, a1
    ret

.global flush_page_global
flush_page_global:
    sfence.vma a0, zero
    ret
"#,
    scratch = const hart::BLOCK_SCRATCH,
    context = const hart::BLOCK_CONTEXT,
    trap_sp = const hart::BLOCK_TRAP_SP,
    plant_senvcfg = const cfg!(feature = "plant-senvcfg") as usize,
);

// A started hart's way in (`hart::start_others`). The firmware starts it at `_hart_start`'s
// *physical* address with the MMU off, `a0` its hart id and `a1` its block's physical address.
// The trampoline is position-independent (only `a1`-relative loads): it loads `satp`, `sp`, the
// virtual landing address and the block's virtual address, and turns paging on with the same
// `stvec`-trap handoff the loader uses: after `csrw satp` the next physical fetch faults and
// traps straight to `_hart_land`, with every register but the program counter intact.
global_asm!(
    r#"
    .section .text
    .global _hart_start
    .balign 4
_hart_start:
    LOADR   t0, {satp}, a1
    LOADR   sp, {sp}, a1
    LOADR   t1, {entry}, a1
    LOADR   t2, {block}, a1
    csrw    stvec, t1
    sfence.vma
    csrw    satp, t0
    unimp

    // Virtual, MMU on; `stvec`'s low two bits are its mode, so this is 4-byte aligned, which a
    // Rust function under the C extension is not. It sets the hart up as `_start` does the boot
    // hart: the trap vector, SUM and MXR clear (R24), `senvcfg` 0 (R11), and `sscratch` its
    // block.
    .global _hart_land
    .balign 4
_hart_land:
    la      t0, _start_trap
    csrw    stvec, t0
    li      t0, (1 << 18) | (1 << 19)
    csrc    sstatus, t0
    csrw    0x10a, zero
    csrw    sscratch, t2
    tail    hart_main
"#,
    satp = const hart::BLOCK_START_SATP,
    sp = const hart::BLOCK_START_SP,
    entry = const hart::BLOCK_START_ENTRY,
    block = const hart::BLOCK_START_BLOCK,
);
