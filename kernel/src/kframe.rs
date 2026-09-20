// SPDX-License-Identifier: MIT OR Apache-2.0

//! Words in RAM frames, reached through the physmap: the one place the kernel reads and writes
//! memory by physical address outside the page-table code.
//!
//! Two kinds of frame go through here:
//! - frames holding kernel objects (budgets, handle-table pages): the kernel allocated them to
//!   [`crate::mem::OBJECT_OWNER`], and they are never mapped into any process;
//! - a process's own pages, while a system call copies a record in or a result out; the caller
//!   found the frame by walking the process's page tables and checked its permissions.
//!
//! Objects are stored as plain 64-bit words, encoded and decoded by their own modules, so no
//! frame is ever read as a Rust type with invalid bit patterns (an enum, a reference, a `bool`).

use xous_kernel::arch::{PAGE_SIZE, PHYSMAP_PHYS_BASE, PHYSMAP_SIZE, physmap_virt};

/// The physmap address of byte `offset` of the frame at `phys`, for an access of `size` bytes.
/// Panics (a kernel bug, never input) unless the frame is one the physmap reaches and the access
/// lies inside it, aligned to its size.
fn at(phys: usize, offset: usize, size: usize) -> usize {
    assert!(
        phys % PAGE_SIZE == 0 && phys >= PHYSMAP_PHYS_BASE && phys - PHYSMAP_PHYS_BASE < PHYSMAP_SIZE,
        "kframe: {:#x} is not a RAM frame",
        phys
    );
    assert!(offset % size == 0 && offset + size <= PAGE_SIZE, "kframe: bad offset {:#x}", offset);
    physmap_virt(phys) + offset
}

/// The word at byte `offset` (a multiple of 8) of the frame at `phys`.
pub fn read(phys: usize, offset: usize) -> u64 {
    let virt = at(phys, offset, 8);
    // SAFETY: `at` checked that this is an aligned word inside a RAM frame, and the loader maps
    // all of RAM read-write at the physmap in every address space (MEMORY-LAYOUT.md). Every bit
    // pattern is a valid `u64`. A process may write its own frame concurrently only from another
    // hart; the volatile read then sees one value or the other, never undefined behaviour in the
    // kernel's view.
    unsafe { (virt as *const u64).read_volatile() }
}

/// Writes the word at byte `offset` (a multiple of 8) of the frame at `phys`.
pub fn write(phys: usize, offset: usize, value: u64) {
    let virt = at(phys, offset, 8);
    // SAFETY: as in `read`, and the write cannot reach the kernel's own code or data: every
    // `phys` passed here is a frame the ownership table handed out (a kernel object's, or a page
    // of the calling process's that the caller checked is its own and writable), never the kernel
    // image, whose physmap alias is read-only anyway. The caller owns what the word means.
    unsafe { (virt as *mut u64).write_volatile(value) }
}

/// Zeroes the whole frame at `phys`. Every page a process first sees goes through here (R11).
pub fn zero(phys: usize) {
    for offset in (0..redoubt_sys::PAGE_SIZE).step_by(8) {
        write(phys, offset, 0);
    }
}
