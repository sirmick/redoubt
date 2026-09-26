use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

// Userspace runtime regions, all in the user half on both widths (< 0x8000_0000 on rv32).
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

pub const PAGE_SIZE: usize = 4096;

/// Sv32 layout. The physmap design of `docs/MEMORY-LAYOUT.md` scaled to two
/// levels: the kernel half is the upper 2 GiB (root entries 512..=1023, 4 MiB each), and
/// RAM is identity-mapped low in it so `PHYSMAP_BASE == PHYSMAP_PHYS_BASE`.
#[cfg(target_pointer_width = "32")]
mod layout {
    /// Root entries 0..=511: userspace (`[0, 0x8000_0000)`).
    pub const USER_AREA_END: usize = 0x8000_0000;
    /// Root entries 512..=1019: physical RAM `[PHYSMAP_PHYS_BASE, +PHYSMAP_SIZE)` mapped at
    /// `virt = PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)`, 4 MiB megapage leaves. On QEMU
    /// `virt` RAM starts at 0x8000_0000, exactly the kernel-half boundary, so the map is the
    /// identity and `PHYSMAP_BASE == PHYSMAP_PHYS_BASE` (rv64 offsets from physical 0 instead).
    pub const PHYSMAP_BASE: usize = 0x8000_0000;
    pub const PHYSMAP_PHYS_BASE: usize = 0x8000_0000;
    pub const PHYSMAP_SIZE: usize = 0x7f00_0000; // 0x8000_0000..0xff00_0000
    /// Root entries 1020..=1021: where the kernel maps the platform's interrupt controller
    /// (up to 8 MiB; a QEMU `virt` PLIC is 6 MiB, which is why this needs two 4 MiB roots).
    pub const KERNEL_PLIC_BASE: usize = 0xff00_0000;
    /// The last 64 KiB of root entry 1021: the kernel's window on DMA devices' registers
    /// (WP-K5b), one page per device, so a PLIC must end below it.
    pub const KERNEL_DMA_REGS: usize = 0xff7f_0000;
    /// Root entry 1022: per-process kernel data.
    pub const PROCESS_AREA: usize = 0xff80_0000;
    pub const THREAD_CONTEXT_AREA: usize = PROCESS_AREA;
    pub const USERSPACE_BUFFER: usize = PROCESS_AREA + 0x10_0000; // 0xff90_0000
    /// `ProcessImpl` bookkeeping: a saved context is 32 x 4 = 128 bytes, 32 contexts = 1 page.
    pub const THREAD_CONTEXT_PAGES: usize = 1;
    /// Root entry 1023: the kernel image, stacks and arguments, shared by every address space.
    pub const KERNEL_AREA: usize = 0xffc0_0000;
    pub const KERNEL_STACK_TOP: usize = 0xfff8_0000;
    pub const KERNEL_STACK_PAGES: usize = 8;
    pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
    pub const EXCEPTION_STACK_PAGES: usize = 8;
    /// Top of the initial thread's stack in every user process (top of the user half).
    pub const USER_STACK_TOP: usize = 0x8000_0000;
}

/// Sv39 layout. See `docs/MEMORY-LAYOUT.md`.
#[cfg(target_pointer_width = "64")]
mod layout {
    /// Root entries 0..=255: userspace.
    pub const USER_AREA_END: usize = 0x40_0000_0000;
    /// Root entries 256..=383: physical memory `[PHYSMAP_PHYS_BASE, +PHYSMAP_SIZE)` mapped
    /// at `virt = PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)`. rv64 maps from physical 0.
    pub const PHYSMAP_BASE: usize = 0xffff_ffc0_0000_0000;
    pub const PHYSMAP_PHYS_BASE: usize = 0;
    pub const PHYSMAP_SIZE: usize = 128 << 30;
    /// Root entry 510: per-process kernel data.
    pub const PROCESS_AREA: usize = 0xffff_ffff_8000_0000;
    pub const THREAD_CONTEXT_AREA: usize = PROCESS_AREA;
    pub const USERSPACE_BUFFER: usize = PROCESS_AREA + 0x10_0000;
    /// Pages occupied by the kernel's per-process bookkeeping at `THREAD_CONTEXT_AREA`.
    pub const THREAD_CONTEXT_PAGES: usize = 2;
    /// Root entry 511: the kernel, shared by every address space.
    pub const KERNEL_AREA: usize = 0xffff_ffff_c000_0000;
    /// Where the kernel maps the platform's interrupt controller.
    pub const KERNEL_PLIC_BASE: usize = 0xffff_ffff_f000_0000;
    /// The kernel's window on DMA devices' registers (WP-K5b), one page per device: right
    /// after the largest PLIC (64 MiB), far below the kernel stacks and image.
    pub const KERNEL_DMA_REGS: usize = 0xffff_ffff_f400_0000;
    pub const KERNEL_STACK_TOP: usize = 0xffff_ffff_fff8_0000;
    pub const KERNEL_STACK_PAGES: usize = 8;
    pub const EXCEPTION_STACK_TOP: usize = 0xffff_ffff_ffff_0000;
    pub const EXCEPTION_STACK_PAGES: usize = 8;
    /// Top of the initial thread's stack in every user process.
    pub const USER_STACK_TOP: usize = 0x8000_0000;
}
pub use layout::*;

/// Pages in the DMA register window: one per DMA device the kernel can reset (WP-K5b).
pub const KERNEL_DMA_PAGES: usize = 16;
const _: () = {
    let end = KERNEL_DMA_REGS + KERNEL_DMA_PAGES * PAGE_SIZE;
    // In the shared kernel half, clear of the physmap, the per-process area and the kernel
    // stacks and image; the PLIC's own end is checked at boot, against its reported size.
    assert!(KERNEL_DMA_REGS > KERNEL_PLIC_BASE);
    assert!(KERNEL_DMA_REGS >= PHYSMAP_BASE + PHYSMAP_SIZE);
    assert!(end <= PROCESS_AREA || KERNEL_DMA_REGS >= PROCESS_AREA + (1 << 30));
    assert!(end <= KERNEL_STACK_TOP - KERNEL_STACK_PAGES * PAGE_SIZE);
};

/// The virtual address at which the kernel's physmap sees physical frame `phys`:
/// `PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)`. rv64 maps from physical 0
/// (`PHYSMAP_PHYS_BASE == 0`), so the subtraction matters only on rv32.
pub const fn physmap_virt(phys: usize) -> usize {
    PHYSMAP_BASE.wrapping_add(phys.wrapping_sub(PHYSMAP_PHYS_BASE))
}

pub const FLG_VALID: usize = 0x1;
pub const FLG_R: usize = 0x2;
pub const FLG_W: usize = 0x4;
pub const FLG_X: usize = 0x8;
pub const FLG_U: usize = 0x10; // User
pub const FLG_A: usize = 0x40;
pub const FLG_D: usize = 0x80; // Dirty (explicitly managed, not automatic)
pub const FLG_S: usize = 0x100; // Shared
pub const FLG_P: usize = 0x200; // swaP

pub fn map_memory_pre(
    _phys: &Option<MemoryAddress>,
    _virt: &Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
) -> core::result::Result<(), Error> {
    Ok(())
}

pub fn map_memory_post(
    _phys: Option<MemoryAddress>,
    _virt: Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
    range: MemoryRange,
) -> core::result::Result<MemoryRange, Error> {
    Ok(range)
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> { Ok(()) }

pub fn unmap_memory_post(_range: MemoryRange) -> core::result::Result<(), Error> { Ok(()) }
