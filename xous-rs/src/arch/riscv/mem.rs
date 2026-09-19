use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

// pub const DEFAULT_STACK_TOP: usize = 0x8000_0000;
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

// open a large aperture from A000-E000 for a potential RAM-mapped swap area: this gives us up to 1GiB swap
// space. Please don't actually use all of it: performance will be unimaginably bad. Note that the
// A000-E000 range is also shared with the MMAP virtual region.
pub const SWAP_HAL_VADDR: usize = 0xa000_0000;
pub const MMAP_VIRT_BASE: usize = 0xb000_0000;

pub const PAGE_SIZE: usize = 4096;

#[cfg(target_pointer_width = "32")]
mod layout {
    pub const USER_AREA_END: usize = 0xff00_0000;
    pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
    pub const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
    pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
    pub const THREAD_CONTEXT_AREA: usize = 0xff80_1000;
    pub const USERSPACE_BUFFER: usize = 0xff90_0000;
}

/// Sv39 layout. See `planning/xous64/MEMORY-LAYOUT.md`.
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
    pub const KERNEL_STACK_TOP: usize = 0xffff_ffff_fff8_0000;
    pub const KERNEL_STACK_PAGES: usize = 8;
    pub const EXCEPTION_STACK_TOP: usize = 0xffff_ffff_ffff_0000;
    pub const EXCEPTION_STACK_PAGES: usize = 8;
    /// Top of the initial thread's stack in every user process.
    pub const USER_STACK_TOP: usize = 0x8000_0000;
}
pub use layout::*;

/// `SysCall::PlatformSpecific` operations on platforms where the kernel runs under SBI
/// firmware. See `planning/xous64/TIMER.md`.
pub mod platform_call {
    /// The hart timer is delivered as this interrupt. Claim it with `claim_interrupt`.
    pub const TIMER_IRQ: usize = 0;
    /// Returns `Scalar1(ticks per second of the `time` CSR)`.
    pub const TIMER_TIMEBASE: usize = 1;
    /// `a2` = absolute `time` value at which to raise `TIMER_IRQ`. Only for the owner of `TIMER_IRQ`.
    pub const TIMER_SET_DEADLINE: usize = 2;
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

/// swap-specific flags
pub const SWAP_FLG_WIRED: u32 = 0x1_00;
pub const SWAP_PT_VADDR: usize = 0xE000_0000;
// E000_0000 - E100_0000 => 16 MiB of vaddr space for page tables; should be more than enough
pub const SWAP_CFG_VADDR: usize = 0xE100_0000;
pub const SWAP_RPT_VADDR: usize = 0xE100_1000;
pub const SWAP_COUNT_VADDR: usize = 0xE110_0000;
pub const SWAP_APP_UART_VADDR: usize = 0xE180_0000;
pub const SWAP_APP_UART_IFRAM_VADDR: usize = 0xE180_1000;
pub const SWAP_STACK_TOP_VADDR: usize = 0xE800_0000;

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
