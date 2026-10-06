//! The kernel half of the address map, and the PIDs the boot handoff carries
//! (`docs/kernel/boot.md`).
//!
//! The loader builds this map and the kernel asserts it, so both take it from here. None of
//! it is user-visible: these addresses have no U bit (R11), and the user ABI (`redoubt-sys`)
//! speaks in handles, so publishing them there would couple userspace to the kernel's layout.
//! See `docs/kernel/memory-layout.md`.

#![no_std]
#![forbid(unsafe_code)]

use core::num::NonZeroU16;

use redoubt_sys::PAGE_SIZE;

/// A process ID, 16 bits. The loader's ownership table and the kernel's frame table are one
/// table of `Option<Pid>` handed from one to the other, two bytes a page, with 0 (`None`) for a
/// free page.
pub type Pid = NonZeroU16;

/// The kernel's own PID: it owns the kernel image, the page tables and every kernel object.
pub const KERNEL_PID: Pid = match Pid::new(1) {
    Some(pid) => pid,
    None => unreachable!(),
};

/// Sv32 layout. The physmap design of `docs/kernel/memory-layout.md` scaled to two
/// levels: the kernel half is the upper 2 GiB (root entries 512..=1023, 4 MiB each), and
/// RAM is identity-mapped low in it so `PHYSMAP_BASE == PHYSMAP_PHYS_BASE`. Host tests read it
/// too, so the rv32 values are checked on the build host.
#[cfg(any(target_pointer_width = "32", test))]
pub mod sv32 {
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
    /// (`docs/kernel/devices.md`), one page per device, so a PLIC must end below it.
    pub const KERNEL_DMA_REGS: usize = 0xff7f_0000;
    /// Root entry 1022: per-process kernel data, the process's header page at its base.
    pub const PROCESS_AREA: usize = 0xff80_0000;
    /// Root entry 1023: the kernel image, stacks and arguments, shared by every address space.
    pub const KERNEL_AREA: usize = 0xffc0_0000;
    /// The kernel's code and constants, 512 KiB: `FLASH` in `kernel/link.x`, which a host test
    /// holds to this value.
    pub const KERNEL_TEXT: usize = 0xffd0_0000;
    /// Every hart's kernel and trap stacks ([`super::hart_kernel_stack_top`]), below the
    /// kernel's code, in root entry 1023.
    pub const HART_STACKS: usize = 0xffc0_0000;
}

/// Sv39 layout. See `docs/kernel/memory-layout.md`.
#[cfg(target_pointer_width = "64")]
mod sv39 {
    /// Root entries 256..=383: physical memory `[PHYSMAP_PHYS_BASE, +PHYSMAP_SIZE)` mapped
    /// at `virt = PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)`. rv64 maps from physical 0.
    pub const PHYSMAP_BASE: usize = 0xffff_ffc0_0000_0000;
    pub const PHYSMAP_PHYS_BASE: usize = 0;
    pub const PHYSMAP_SIZE: usize = 128 << 30;
    /// Root entry 510: per-process kernel data, the process's header page at its base.
    pub const PROCESS_AREA: usize = 0xffff_ffff_8000_0000;
    /// Root entry 511: the kernel, shared by every address space.
    pub const KERNEL_AREA: usize = 0xffff_ffff_c000_0000;
    /// The kernel's code and constants, 512 KiB: `FLASH` in `kernel/link64.x`, which a host
    /// test holds to this value.
    pub const KERNEL_TEXT: usize = 0xffff_ffff_ffd0_0000;
    /// Where the kernel maps the platform's interrupt controller.
    pub const KERNEL_PLIC_BASE: usize = 0xffff_ffff_f000_0000;
    /// The kernel's window on DMA devices' registers, one page per device: right
    /// after the largest PLIC (64 MiB), far below the kernel stacks and image.
    pub const KERNEL_DMA_REGS: usize = 0xffff_ffff_f400_0000;
    /// Every hart's kernel and trap stacks ([`super::hart_kernel_stack_top`]), below the
    /// kernel's code, above the DMA register window.
    pub const HART_STACKS: usize = 0xffff_ffff_ffc0_0000;
}
#[cfg(target_pointer_width = "32")]
pub use sv32::*;
#[cfg(target_pointer_width = "64")]
pub use sv39::*;

// User space ends where the kernel half begins. Sv39 translates only addresses whose bits 63..=38
// are all equal, so its lower half is the 2^38 bytes below bit 38.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(redoubt_sys::USER_AREA_END <= 1 << 38, "user space past Sv39's lower half");
// Sv32 has no hole: the kernel half starts at the physmap, its lowest address.
#[cfg(target_pointer_width = "32")]
const _: () = assert!(redoubt_sys::USER_AREA_END <= PHYSMAP_BASE, "user space reaches the kernel half");

/// Whether `addr` is a canonical Sv39 address in the upper half: bits 63..=38 all set. Any other
/// kernel-half address is not canonical, and every access to it faults.
#[cfg(target_pointer_width = "64")]
const fn canonical(addr: usize) -> bool { (addr as isize) >> 38 == -1 }
#[cfg(target_pointer_width = "64")]
const _: () = {
    let bases = [
        PHYSMAP_BASE,
        KERNEL_PLIC_BASE,
        KERNEL_DMA_REGS,
        PROCESS_AREA,
        KERNEL_AREA,
        KERNEL_TEXT,
        KERNEL_STACK_TOP,
        TRAP_STACK_TOP,
        HART_STACKS,
    ];
    let mut i = 0;
    while i < bases.len() {
        assert!(canonical(bases[i]), "a kernel-half base is not a canonical Sv39 address");
        i += 1;
    }
};

// The physmap's end is an address of this width: `physmap_covers` adds the two unchecked.
const _: () = assert!(PHYSMAP_PHYS_BASE.checked_add(PHYSMAP_SIZE).is_some(), "the physmap ends past usize");

/// The most harts the kernel runs; a hart past it stays parked in the firmware.
pub const MAX_HARTS: usize = 8;

/// Pages in each hart's kernel stack and trap stack.
pub const KERNEL_STACK_PAGES: usize = 8;
pub const TRAP_STACK_PAGES: usize = 8;

/// One slot of [`HART_STACKS`] per boot index: an unmapped guard page, the hart's kernel stack,
/// another guard page, its trap stack. Slot 0 is the boot hart's, which the loader maps; the
/// kernel maps a slot when it starts its hart, and the guards are never mapped.
pub const HART_STACK_SLOT: usize = (1 + KERNEL_STACK_PAGES + 1 + TRAP_STACK_PAGES) * PAGE_SIZE;

/// The top of the kernel stack of the hart with boot index `index`.
pub const fn hart_kernel_stack_top(index: usize) -> usize {
    HART_STACKS + index * HART_STACK_SLOT + (1 + KERNEL_STACK_PAGES) * PAGE_SIZE
}

/// The top of the trap stack of the hart with boot index `index`.
pub const fn hart_trap_stack_top(index: usize) -> usize { HART_STACKS + (index + 1) * HART_STACK_SLOT }

/// The boot hart's stacks: slot 0's.
pub const KERNEL_STACK_TOP: usize = hart_kernel_stack_top(0);
pub const TRAP_STACK_TOP: usize = hart_trap_stack_top(0);

const _: () = {
    // The window lies in the shared kernel area (so past the per-process area), below the
    // kernel's image, above the PLIC and DMA register windows, and on Sv32 within the 1 MiB below
    // the image.
    let end = HART_STACKS + MAX_HARTS * HART_STACK_SLOT;
    assert!(HART_STACKS >= KERNEL_AREA && end <= KERNEL_TEXT);
    assert!(HART_STACKS > KERNEL_PLIC_BASE && HART_STACKS >= KERNEL_DMA_REGS + KERNEL_DMA_PAGES * PAGE_SIZE);
    assert!(end - HART_STACKS <= 1 << 20);
};

/// Pages in the DMA register window: one per DMA device the kernel can reset
/// (`docs/kernel/devices.md`).
pub const KERNEL_DMA_PAGES: usize = 16;
const _: () = {
    let end = KERNEL_DMA_REGS + KERNEL_DMA_PAGES * PAGE_SIZE;
    // In the shared kernel half, clear of the physmap, the per-process area and the kernel
    // stacks and image; the PLIC's own end is checked at boot, against its reported size.
    assert!(KERNEL_DMA_REGS > KERNEL_PLIC_BASE);
    assert!(KERNEL_DMA_REGS >= PHYSMAP_BASE + PHYSMAP_SIZE);
    assert!(end <= PROCESS_AREA || KERNEL_DMA_REGS >= PROCESS_AREA + (1 << 30));
    assert!(end <= HART_STACKS);
};

/// RAM pages the kernel keeps at boot for `dma_alloc`'s runs, on both widths: the DMA pool
/// (`docs/kernel/devices.md`).
pub const DMA_POOL_PAGES: usize = 1024;

/// The virtual address at which the kernel's physmap sees physical frame `phys`:
/// `PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)`. rv64 maps from physical 0
/// (`PHYSMAP_PHYS_BASE == 0`), so the subtraction matters only on rv32.
pub const fn physmap_virt(phys: usize) -> usize {
    PHYSMAP_BASE.wrapping_add(phys.wrapping_sub(PHYSMAP_PHYS_BASE))
}

/// Whether the physmap covers all of `ram`. The kernel hands out frames lowest first and reaches
/// each through the physmap, so RAM past its end would stop the kernel the first time a process
/// allocated a frame there; the loader refuses such a machine at boot instead (R17).
pub fn physmap_covers(ram: &core::ops::Range<usize>) -> bool {
    ram.start >= PHYSMAP_PHYS_BASE && ram.end <= PHYSMAP_PHYS_BASE + PHYSMAP_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    const END: usize = PHYSMAP_PHYS_BASE + PHYSMAP_SIZE;

    #[test]
    fn ram_ending_at_the_physmap_end_is_covered() {
        assert!(physmap_covers(&(PHYSMAP_PHYS_BASE..END)));
        assert!(physmap_covers(&(END - PAGE_SIZE..END)));
    }

    #[test]
    fn ram_one_page_past_the_physmap_end_is_refused() {
        assert!(!physmap_covers(&(PHYSMAP_PHYS_BASE..END + PAGE_SIZE)));
    }

    /// `FLASH`'s origin in a kernel link script.
    fn flash_origin(script: &str) -> usize {
        let line = script.lines().find(|l| l.trim_start().starts_with("FLASH")).expect("a FLASH region");
        let origin = line.split("ORIGIN = 0x").nth(1).expect("an ORIGIN").split(',').next().unwrap();
        usize::from_str_radix(origin.trim(), 16).expect("a hex ORIGIN")
    }

    /// The kernel's link scripts place its text where `KERNEL_TEXT` says, on both widths: the
    /// linker cannot read this crate, so this is what keeps the two from drifting apart.
    #[test]
    fn the_link_scripts_put_the_kernel_text_at_kernel_text() {
        assert_eq!(flash_origin(include_str!("../../../kernel/link.x")), sv32::KERNEL_TEXT);
        assert_eq!(flash_origin(include_str!("../../../kernel/link64.x")), KERNEL_TEXT);
    }

    /// The rv32 constants, on the host: the physmap's end fits in a 32-bit address.
    #[test]
    fn the_sv32_physmap_ends_inside_32_bits() {
        let end = sv32::PHYSMAP_PHYS_BASE as u64 + sv32::PHYSMAP_SIZE as u64;
        assert!(end <= u64::from(u32::MAX), "the Sv32 physmap ends at {end:#x}");
    }
}
