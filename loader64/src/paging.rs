//! Sv39 address space construction. See `planning/xous64/MEMORY-LAYOUT.md`.
//!
//! Page-table memory is only touched through the `sv39` crate, which the kernel uses too.

use sv39::{PteFlags, Slot, Table, Window, ENTRIES, GIGAPAGE};
use xous::arch::{PHYSMAP_BASE, PROCESS_AREA};

use crate::alloc::{PageAllocator, Pid};
use crate::PAGE_SIZE;

pub use sv39::PteFlags as Pte;

const SATP_MODE_SV39: usize = 8 << 60;
const ROOT_KERNEL_START: usize = ENTRIES / 2;
const ROOT_PROCESS_AREA: usize = sv39::vpn(PROCESS_AREA, 2);

fn window() -> Window {
    // SAFETY: the loader runs with address translation off from entry until it hands over
    // to the kernel, so physical addresses are directly usable.
    unsafe { Window::identity() }
}

fn new_table(alloc: &mut PageAllocator, pid: Pid) -> (usize, Table) {
    let phys = alloc.alloc(pid);
    // SAFETY: `alloc` returns a RAM frame that nothing else uses.
    (phys, unsafe { Table::new_in(window(), phys) })
}

/// One address space, identified by its root table.
#[derive(Copy, Clone)]
pub struct AddressSpace {
    root_phys: usize,
    root: Table,
    pid: Pid,
}

impl AddressSpace {
    /// Create the kernel's (PID 1) address space: the physmap over `ram`, plus the L1
    /// table under the kernel's root entry so that every later kernel mapping is shared.
    pub fn new_kernel(alloc: &mut PageAllocator, pid: Pid) -> Self {
        let (root_phys, root) = new_table(alloc, pid);
        let ram = alloc.ram();
        // The physmap is data: readable and writable, never executable.
        let flags = PteFlags::R | PteFlags::W | PteFlags::GLOBAL;
        for giga in (ram.start / GIGAPAGE)..ram.end.div_ceil(GIGAPAGE) {
            let phys = giga * GIGAPAGE;
            root.slot(sv39::vpn(PHYSMAP_BASE + phys, 2)).set(sv39::Pte::leaf(phys, flags));
        }
        let kernel_l1 = alloc.alloc(pid);
        // SAFETY: `alloc` returns a RAM frame that nothing else uses.
        unsafe { root.slot(ENTRIES - 1).install_table(kernel_l1) };
        AddressSpace { root_phys, root, pid }
    }

    /// Create a user address space that shares every kernel root entry with `kernel`.
    pub fn new_user(alloc: &mut PageAllocator, pid: Pid, kernel: &AddressSpace) -> Self {
        let (root_phys, root) = new_table(alloc, pid);
        for index in (ROOT_KERNEL_START..ENTRIES).filter(|index| *index != ROOT_PROCESS_AREA) {
            root.slot(index).copy_from(kernel.root.slot(index));
        }
        AddressSpace { root_phys, root, pid }
    }

    pub fn satp(&self) -> usize { SATP_MODE_SV39 | (self.pid as usize) << 44 | self.root_phys >> 12 }

    fn leaf_slot(&self, alloc: &mut PageAllocator, virt: usize) -> Slot {
        assert!(sv39::is_canonical(virt), "{virt:#x} is not a canonical Sv39 address");
        let mut table = self.root;
        for level in [2, 1] {
            let index = sv39::vpn(virt, level);
            table = match table.child(index) {
                Some(child) => child,
                None => {
                    assert!(table.get(index).is_empty(), "{virt:#x} is inside a superpage");
                    let frame = alloc.alloc(self.pid);
                    // SAFETY: `alloc` returns a RAM frame that nothing else uses.
                    unsafe { table.slot(index).install_table(frame) }
                }
            };
        }
        table.slot(sv39::vpn(virt, 0))
    }

    /// Physical page backing `virt`, if one is mapped.
    pub fn translate(&self, alloc: &mut PageAllocator, virt: usize) -> Option<usize> {
        let pte = self.leaf_slot(alloc, virt).get();
        pte.is_valid().then(|| pte.phys())
    }

    /// Map one page. Mapping the same page again ORs in the new permissions, which
    /// happens when two ELF segments share a page. The result must still satisfy W^X.
    pub fn map(&self, alloc: &mut PageAllocator, phys: usize, virt: usize, flags: PteFlags) {
        assert!(virt % PAGE_SIZE == 0 && phys % PAGE_SIZE == 0);
        let slot = self.leaf_slot(alloc, virt);
        let existing = slot.get();
        assert!(!existing.is_valid() || existing.phys() == phys, "{virt:#x} is already mapped");
        slot.set(sv39::Pte::leaf(phys, flags | existing.flags()));
    }

    /// Reserve a page for demand paging: permissions without `VALID`. The kernel backs
    /// it with memory on first touch.
    pub fn reserve(&self, alloc: &mut PageAllocator, virt: usize, flags: PteFlags) {
        self.leaf_slot(alloc, virt).set(sv39::Pte::reservation(flags));
    }

    /// Allocate and map `count` pages ending at `top`.
    pub fn map_stack(&self, alloc: &mut PageAllocator, top: usize, count: usize, flags: PteFlags) {
        for i in 1..=count {
            let page = alloc.alloc(self.pid);
            self.map(alloc, page, top - i * PAGE_SIZE, flags);
        }
    }
}
