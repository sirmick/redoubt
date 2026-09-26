// SPDX-License-Identifier: MIT OR Apache-2.0

//! How the kernel reaches page tables: through the physmap. The typed page-table layer
//! itself is the `paging` crate, which the loader uses too.

pub use paging::*;
use redoubt_layout::{PHYSMAP_BASE, PHYSMAP_PHYS_BASE, PHYSMAP_SIZE};

/// The kernel's view of physical memory.
pub fn window() -> Window {
    // SAFETY: the loader maps all of RAM at `PHYSMAP_BASE`, read-write and supervisor-only,
    // using root entries that `MemoryMapping::allocate` copies into every address space.
    // Nothing ever unmaps it, so `PHYSMAP_BASE + phys` is valid for every RAM frame, always.
    unsafe {
        Window::new(
            PHYSMAP_BASE.wrapping_sub(PHYSMAP_PHYS_BASE),
            PHYSMAP_PHYS_BASE..PHYSMAP_PHYS_BASE + PHYSMAP_SIZE,
        )
    }
}
