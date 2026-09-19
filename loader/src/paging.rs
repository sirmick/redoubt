//! Address space construction (Sv32 and Sv39). See `planning/redoubt/MEMORY-LAYOUT.md`.
//!
//! Page-table memory is only touched through the `paging` crate, which the kernel uses too.

use paging::{PteFlags, Slot, Table, Window, ENTRIES, LARGEST_LEAF, LEVELS};
use xous::arch::{PHYSMAP_PHYS_BASE, PROCESS_AREA};

use crate::alloc::{PageAllocator, Pid};
use crate::PAGE_SIZE;

pub use paging::PteFlags as Pte;

const ROOT_KERNEL_START: usize = ENTRIES / 2;
const ROOT_PROCESS_AREA: usize = paging::vpn(PROCESS_AREA, LEVELS - 1);

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
        // Map [PHYSMAP_PHYS_BASE, ram.end) at virt = PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE),
        // in leaves of the largest size (gigapage on Sv39, megapage on Sv32).
        let first = PHYSMAP_PHYS_BASE / LARGEST_LEAF;
        let last = ram.end.div_ceil(LARGEST_LEAF);
        for leaf in first..last {
            let phys = leaf * LARGEST_LEAF;
            let virt = xous::arch::physmap_virt(phys);
            root.slot(paging::vpn(virt, LEVELS - 1)).set(paging::Pte::leaf(phys, flags));
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

    pub fn satp(&self) -> usize { paging::make_satp(self.pid as usize, self.root_phys) }

    fn leaf_slot(&self, alloc: &mut PageAllocator, virt: usize) -> Slot {
        assert!(paging::is_canonical(virt), "{virt:#x} is not a canonical address");
        let mut table = self.root;
        for level in (1..LEVELS).rev() {
            let index = paging::vpn(virt, level);
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
        table.slot(paging::vpn(virt, 0))
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
        slot.set(paging::Pte::leaf(phys, flags | existing.flags()));
    }

    /// Make the physmap's view of the frame at `phys` read-only.
    ///
    /// The physmap maps all of RAM writable, which would otherwise give the kernel a
    /// writable alias of its own code, in breach of W^X. The physmap is built from
    /// gigapages, so the superpages covering `phys` are first split into smaller ones.
    pub fn write_protect_in_physmap(&self, alloc: &mut PageAllocator, phys: usize) {
        let virt = xous::arch::physmap_virt(phys);
        let mut table = self.root;
        for level in (1..LEVELS).rev() {
            let slot = table.slot(paging::vpn(virt, level));
            table = match table.child(paging::vpn(virt, level)) {
                Some(child) => child,
                None => {
                    // Replace this superpage with a table of the next size down that maps
                    // exactly the same memory with the same permissions.
                    let superpage = slot.get();
                    assert!(superpage.is_leaf(), "{phys:#x} is not in the physmap");
                    let frame = alloc.alloc(self.pid);
                    // SAFETY: `alloc` returns a RAM frame that nothing else uses.
                    let child = unsafe { slot.install_table(frame) };
                    let flags = superpage.flags() - PteFlags::VALID;
                    for index in 0..ENTRIES {
                        let part = superpage.phys() + index * paging::leaf_size(level - 1);
                        child.slot(index).set(paging::Pte::leaf(part, flags));
                    }
                    child
                }
            };
        }
        let slot = table.slot(paging::vpn(virt, 0));
        slot.set(slot.get().without(PteFlags::W));
    }

    /// Reserve a page for demand paging: permissions without `VALID`. The kernel backs
    /// it with memory on first touch.
    pub fn reserve(&self, alloc: &mut PageAllocator, virt: usize, flags: PteFlags) {
        self.leaf_slot(alloc, virt).set(paging::Pte::reservation(flags));
    }

    /// Pre-create the intermediate page tables covering `[virt, virt + size)` without
    /// mapping any leaf. When these tables belong to the shared kernel region and this runs
    /// before user address spaces copy the kernel's root entries, a leaf the kernel later
    /// maps into them (its PLIC, say) becomes visible in every address space. On rv64 the
    /// single shared kernel L1 already spans that region, so this only pre-allocates a
    /// leaf-level table there; on rv32, where the region crosses several 4 MiB root entries,
    /// it is what makes those roots shared tables rather than empty copies.
    pub fn reserve_tables(&self, alloc: &mut PageAllocator, virt: usize, size: usize) {
        let mut addr = virt;
        let end = virt + size;
        while addr < end {
            // Walking to the leaf slot installs every table above it; the slot is discarded.
            let _ = self.leaf_slot(alloc, addr);
            addr += paging::leaf_size(1);
        }
    }

    /// Allocate and map `count` pages ending at `top`.
    pub fn map_stack(&self, alloc: &mut PageAllocator, top: usize, count: usize, flags: PteFlags) {
        for i in 1..=count {
            let page = alloc.alloc(self.pid);
            self.map(alloc, page, top - i * PAGE_SIZE, flags);
        }
    }
}
