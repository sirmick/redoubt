// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Kernel entry, trap entry and context restore, for both rv64 (Sv39) and rv32 (Sv32).
//!
//! Ported from the original `asm.S` to `global_asm!` so the build needs no C toolchain and
//! no prebuilt blobs. The two widths differ only mechanically: a saved context is
//! `32 x size_of::<usize>()` bytes (256 on rv64, 128 on rv32), so context N lives at
//! `THREAD_CONTEXT_AREA + (N << CTX_SHIFT)`; the load/store width and the reservation-clear
//! instruction change with the register width. All of that is confined to the small,
//! `cfg`-gated preamble below; the entry paths are shared. Addresses come from
//! `xous_kernel::arch` rather than being repeated as literals. There is no suspend/resume
//! entry path; that was specific to the Precursor SoC.

use core::arch::global_asm;

use xous_kernel::arch::{EXCEPTION_STACK_TOP, THREAD_CONTEXT_AREA};

/// `log2(size of a saved context)`: contexts are indexed by `n << CTX_SHIFT`.
const CTX_SHIFT: usize = (32 * core::mem::size_of::<usize>()).trailing_zeros() as usize;

// The trap handler indexes contexts with this shift, so a `Thread` must be exactly that big.
const _: () = assert!(core::mem::size_of::<super::process::Thread>() == 1 << CTX_SHIFT);

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
    call    init
    j       kmain

/*
    Trap entry point. Saves the full context of the interrupted thread into its slot in
    the per-process context area, switches to the exception stack and enters Rust.
*/
.section .trap, "ax"
.global _start_trap
.balign 4
_start_trap:
    csrw    sscratch, sp
    li      sp, {context_area}
    SAVE    x1, 0                   // Stash x1 in the header's scratch field
    RESTORE x1, 1                   // Load the current context number (header slot 1)
    slli    x1, x1, {ctx_shift}     // Each context is 1 << ctx_shift bytes
    add     sp, sp, x1              // sp = &contexts[current]

    SAVE_X3_TO_X31

    csrr    t0, sepc
    SAVE    t0, 31

    // Save the real x1, which was stashed in the header
    li      t0, {context_area}
    LOADR   t1, 0, t0              // t1 = header slot 0 (stashed x1)
    SAVE    t1, 0

    // Save the real sp
    csrr    t0, sscratch
    SAVE    t0, 1

    // Note that a0-a7 still contain the syscall arguments
    li      sp, {exception_sp}
    j       _start_trap_rust

/*
    Resume the context pointed to by a0. sepc and sstatus must already be set.
*/
.global _xous_resume_context
_xous_resume_context:
    mv      sp, a0
    RESTORE x1, 0
    CLEAR_RESERVATION
    RESTORE_X3_TO_X9
    RESTORE_X10_TO_X17
    RESTORE_X18_TO_X31
    RESTORE x2, 1
    sret

/*
    Return from a syscall. Xous returns values in a0-a7, but the C calling convention
    only gives us two return registers, so a0 points at an 8-word result block that is
    unpacked into the argument registers. a1 is the context to resume.
*/
.global _xous_syscall_return_result
_xous_syscall_return_result:
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

.global flush_mmu
flush_mmu:
    sfence.vma
    ret
"#,
    context_area = const THREAD_CONTEXT_AREA,
    exception_sp = const EXCEPTION_STACK_TOP - 16,
    ctx_shift = const CTX_SHIFT,
);
