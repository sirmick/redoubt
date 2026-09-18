//! Sv39 page table construction. The loader runs with the MMU off, so page tables are
//! edited through their physical addresses. See `planning/xous64/MEMORY-LAYOUT.md`.

use xous::arch::{PHYSMAP_BASE, PROCESS_AREA};

use crate::alloc::{PageAllocator, Pid};
use crate::PAGE_SIZE;

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Pte: usize {
        const VALID    = 1 << 0;
        const R        = 1 << 1;
        const W        = 1 << 2;
        const X        = 1 << 3;
        const USER     = 1 << 4;
        const GLOBAL   = 1 << 5;
        const ACCESSED = 1 << 6;
        const DIRTY    = 1 << 7;
    }
}

const ENTRIES: usize = 512;
const GIGAPAGE: usize = 1 << 30;
const SATP_MODE_SV39: usize = 8 << 60;
const ROOT_KERNEL_START: usize = ENTRIES / 2;
const ROOT_PROCESS_AREA: usize = (PROCESS_AREA >> 30) & (ENTRIES - 1);

fn vpn(virt: usize, level: usize) -> usize { (virt >> (12 + 9 * level)) & (ENTRIES - 1) }

fn pte(phys: usize, flags: Pte) -> usize { ((phys >> 12) << 10) | flags.bits() }

fn pte_phys(pte: usize) -> usize { (pte >> 10) << 12 }

/// One address space, identified by its root table.
#[derive(Copy, Clone)]
pub struct AddressSpace {
    root: usize,
    pid: Pid,
}

impl AddressSpace {
    /// Create the kernel's (PID 1) address space: the physmap over `ram`, plus the L1
    /// table under the kernel's root entry so that every later kernel mapping is shared.
    pub fn new_kernel(alloc: &mut PageAllocator, pid: Pid) -> Self {
        let space = AddressSpace { root: alloc.alloc(pid), pid };
        let ram = alloc.ram();
        let flags = Pte::VALID | Pte::R | Pte::W | Pte::GLOBAL | Pte::ACCESSED | Pte::DIRTY;
        for giga in (ram.start / GIGAPAGE)..ram.end.div_ceil(GIGAPAGE) {
            let phys = giga * GIGAPAGE;
            unsafe { space.table(space.root).add(vpn(PHYSMAP_BASE + phys, 2)).write(pte(phys, flags)) };
        }
        let kernel_l1 = alloc.alloc(pid);
        unsafe { space.table(space.root).add(ENTRIES - 1).write(pte(kernel_l1, Pte::VALID)) };
        space
    }

    /// Create a user address space that shares every kernel root entry with `kernel`.
    pub fn new_user(alloc: &mut PageAllocator, pid: Pid, kernel: &AddressSpace) -> Self {
        let space = AddressSpace { root: alloc.alloc(pid), pid };
        for idx in (ROOT_KERNEL_START..ENTRIES).filter(|idx| *idx != ROOT_PROCESS_AREA) {
            unsafe { space.table(space.root).add(idx).write(kernel.table(kernel.root).add(idx).read()) };
        }
        space
    }

    pub fn satp(&self) -> usize { SATP_MODE_SV39 | (self.pid as usize) << 44 | self.root >> 12 }

    fn table(&self, phys: usize) -> *mut usize { phys as *mut usize }

    fn leaf_entry(&self, alloc: &mut PageAllocator, virt: usize) -> *mut usize {
        let mut table = self.table(self.root);
        for level in [2, 1] {
            let entry = unsafe { table.add(vpn(virt, level)) };
            let mut value = unsafe { entry.read() };
            if value & Pte::VALID.bits() == 0 {
                value = pte(alloc.alloc(self.pid), Pte::VALID);
                unsafe { entry.write(value) };
            }
            table = self.table(pte_phys(value));
        }
        unsafe { table.add(vpn(virt, 0)) }
    }

    /// Physical page backing `virt`, if one is mapped.
    pub fn translate(&self, alloc: &mut PageAllocator, virt: usize) -> Option<usize> {
        let value = unsafe { self.leaf_entry(alloc, virt).read() };
        (value & Pte::VALID.bits() != 0).then(|| pte_phys(value))
    }

    /// Map one page. Mapping the same page again ORs in the new permissions, which
    /// happens when two ELF segments share a page.
    pub fn map(&self, alloc: &mut PageAllocator, phys: usize, virt: usize, flags: Pte) {
        assert!(virt % PAGE_SIZE == 0 && phys % PAGE_SIZE == 0);
        let entry = self.leaf_entry(alloc, virt);
        let existing = unsafe { entry.read() };
        assert!(existing & Pte::VALID.bits() == 0 || pte_phys(existing) == phys, "{virt:#x} is already mapped");
        let flags = flags | Pte::VALID | Pte::ACCESSED | Pte::DIRTY;
        unsafe { entry.write(existing | pte(phys, flags)) };
    }

    /// Reserve a page for demand paging: permissions without `VALID`. The kernel backs
    /// it with memory on first touch.
    pub fn reserve(&self, alloc: &mut PageAllocator, virt: usize, flags: Pte) {
        let entry = self.leaf_entry(alloc, virt);
        unsafe { entry.write((flags - Pte::VALID).bits()) };
    }

    /// Allocate and map `count` pages ending at `top`.
    pub fn map_stack(&self, alloc: &mut PageAllocator, top: usize, count: usize, flags: Pte) {
        for i in 1..=count {
            let page = alloc.alloc(self.pid);
            self.map(alloc, page, top - i * PAGE_SIZE, flags);
        }
    }
}
