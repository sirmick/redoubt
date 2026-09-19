// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use xous_kernel::{MemoryFlags, MemoryRange, PID, arch::*};

pub use crate::arch::mem::MemoryMapping;
use crate::arch::process::Process;

#[derive(Debug)]
// below suppresses warning from unused Move argument in hosted mode
#[allow(dead_code)]
enum ClaimReleaseMove {
    Claim,
    Release,
    Move(PID /* from */),
}

/// One entry of the loader's `MREx` table (BOOT.md): a device region processes may claim.
#[cfg(baremetal)]
#[derive(Clone, Copy)]
struct MemoryRangeExtra {
    start: usize,
    size: usize,
}

#[cfg(baremetal)]
impl MemoryRangeExtra {
    /// Words per entry. The loader (one binary for both widths) writes every entry as
    /// `start: u64, size: u64, tag: u32, pad: u32`, each `u64` low word first, so the entry is
    /// six words on rv32 too.
    const WORDS: usize = 6;

    /// Decode one entry. The table is read word by word rather than cast to a struct: the tag
    /// data is only word-aligned, and a struct with `u64` fields needs 8-byte alignment.
    fn from_words(words: &[u32]) -> Self {
        let start = crate::args::wide(words, 0);
        let size = crate::args::wide(words, 2);
        assert!(start.checked_add(size).is_some(), "mm: MREx region wraps the address space");
        // The ownership table has one entry per page of the region, and `extra_index` finds a
        // page by dividing, so a region that is not a whole number of pages would let an
        // address at its end index past the entries counted for it. The loader rounds every
        // region up to a page.
        assert!(size % PAGE_SIZE == 0, "mm: MREx region is not a whole number of pages");
        MemoryRangeExtra { start, size }
    }

    fn contains(&self, addr: usize) -> bool { addr >= self.start && addr - self.start < self.size }
}

/// Construct a `MemoryRange` describing `addr..addr + size`.
///
/// `MemoryRange::new` is `unsafe` because a range may later be handed to a process as
/// valid, page-aligned memory. Inside the kernel that property is established by the page
/// tables, and the descriptor's own invariants -- non-null address, non-zero size -- are
/// exactly what `new` checks and returns an error for. So building the descriptor is a
/// safe kernel operation: a bad address surfaces later as a mapping error, not as
/// unsoundness here.
pub fn memory_range(addr: usize, size: usize) -> Result<MemoryRange, xous_kernel::Error> {
    // SAFETY: see the doc comment.
    unsafe { MemoryRange::new(addr, size) }
}

pub struct MemoryManager {
    #[cfg_attr(not(baremetal), allow(dead_code))]
    ram_start: usize,
    #[cfg_attr(not(baremetal), allow(dead_code))]
    ram_size: usize,
    #[allow(dead_code)]
    ram_name: u32,
    #[allow(dead_code)]
    last_ram_page: usize,
    /// Who owns each page of RAM, indexed by page number within RAM. The loader builds
    /// this table and hands it over in `init_from_memory`.
    #[cfg(baremetal)]
    allocations: &'static mut [RamAllocation],
    /// The same, for the pages of every region in `extra_regions`, back to back.
    #[cfg(baremetal)]
    extra_allocations: &'static mut [Option<PID>],
    /// Memory outside RAM that processes may claim: memory-mapped devices. The data of the
    /// loader's `MREx` tag, `MemoryRangeExtra::WORDS` words per region; see `extra_regions()`.
    #[cfg(baremetal)]
    extra_regions: &'static [u32],
    /// Budgets and the per-process ledger that charges them (`budget.rs`), and the handle tables
    /// (`handle.rs`). Here, beside the ownership table, because a frame changing owner is what
    /// most charges are.
    #[cfg(baremetal)]
    pub objects: crate::budget::Objects,
}

/// Owner, in the ownership table, of frames that hold kernel objects (budgets, handle-table
/// pages). No process has this PID (there are `MAX_PROCESS_COUNT` of them), so such a frame is
/// never mapped into a process, and `release_all_memory_for_process` never frees one.
#[cfg(baremetal)]
pub const OBJECT_OWNER: PID = match PID::new(255) {
    Some(pid) => pid,
    None => unreachable!(),
};
#[cfg(baremetal)]
const _: () = assert!(crate::arch::process::MAX_PROCESS_COUNT < 255);
#[cfg(baremetal)]
type RamAllocation = Option<PID>;

impl Default for MemoryManager {
    fn default() -> Self { Self::default_hack() }
}

#[cfg(not(baremetal))]
std::thread_local!(static MEMORY_MANAGER: core::cell::RefCell<MemoryManager> = core::cell::RefCell::new(MemoryManager::default()));

/// Lock order: `SystemServices` (services.rs) before `MemoryManager`. Code holding the memory
/// manager never takes the process table, so charging a budget from deep inside the allocator
/// (`alloc_page`) needs only this cell; code holding the process table may take this one.
#[cfg(baremetal)]
static MEMORY_MANAGER: crate::cell::KernelCell<MemoryManager> =
    crate::cell::KernelCell::new(MemoryManager::default_hack());
/// as the process entry has not yet been created.
impl MemoryManager {
    const fn default_hack() -> Self {
        MemoryManager {
            ram_start: 0,
            ram_size: 0,
            ram_name: 0,
            last_ram_page: 0,
            #[cfg(baremetal)]
            allocations: &mut [],
            #[cfg(baremetal)]
            extra_allocations: &mut [],
            #[cfg(baremetal)]
            extra_regions: &[],
            #[cfg(baremetal)]
            objects: crate::budget::Objects::new(),
        }
    }

    // /// Calls the provided function with the current inner process state.
    // pub fn with<F, R>(f: F) -> R
    // where
    //     F: FnOnce(&MemoryManager) -> R,
    // {
    //     #[cfg(baremetal)]
    //     unsafe {
    //         f(&MEMORY_MANAGER)
    //     }

    //     #[cfg(not(baremetal))]
    //     MEMORY_MANAGER.with(|ss| f(&ss.borrow()))
    // }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut MemoryManager) -> R,
    {
        #[cfg(baremetal)]
        return MEMORY_MANAGER.with(f);

        #[cfg(not(baremetal))]
        MEMORY_MANAGER.with(|ss| f(&mut ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&MemoryManager) -> R,
    {
        #[cfg(baremetal)]
        return MEMORY_MANAGER.with(|mm| f(mm));

        #[cfg(not(baremetal))]
        MEMORY_MANAGER.with(|ss| f(&ss.borrow_mut()))
    }

    #[cfg(baremetal)]
    pub fn init_from_memory(
        &mut self,
        rpt_base: usize,
        xpt_base: usize,
        args: &crate::args::KernelArguments,
    ) -> Result<(), xous_kernel::Error> {
        use core::slice;
        let mut args_iter = args.iter();
        let xarg_def = args_iter.next().expect("mm: no kernel arguments found");
        assert!(
            self.extra_regions.is_empty(),
            "mm: self.extra.len() was {}, not 0",
            self.extra_regions.len()
        );
        assert!(xarg_def.name == u32::from_le_bytes(*b"XArg"), "mm: first tag wasn't XArg");
        // The loader (the same binary for both widths) writes XArg v2: RAM base and size as
        // 64-bit values, low word first (`args::wide` narrows them).
        assert!(xarg_def.data[1] == 2, "mm: XArg had unexpected version");
        self.ram_start = crate::args::wide(xarg_def.data, 2);
        self.ram_size = crate::args::wide(xarg_def.data, 4);
        self.ram_name = xarg_def.data[6];

        let mem_size = self.ram_size / PAGE_SIZE;
        let mut extra_size = 0;
        for tag in args_iter {
            if tag.name == u32::from_le_bytes(*b"MREx") {
                assert!(self.extra_regions.is_empty(), "mm: MREx tag appears twice");
                assert!(
                    tag.data.len() % MemoryRangeExtra::WORDS == 0,
                    "mm: MREx is not a whole number of entries"
                );
                self.extra_regions = tag.data;
            }
        }

        // Decoding checks every entry, so a malformed one stops the boot here.
        for range in self.extra_regions() {
            extra_size += range.size / PAGE_SIZE;
        }
        // SAFETY: `rpt_base` is the page-aligned ownership table the loader built and filled
        // in, one byte per page of RAM, so it holds these `mem_size` entries; every byte is a
        // valid `Option<PID>` (zero, an unowned page, is `None`). The loader owns it for the
        // kernel and hands it over here, so this is the only reference to it.
        unsafe { self.allocations = slice::from_raw_parts_mut(rpt_base as *mut Option<PID>, mem_size) };
        // SAFETY: as above, for the table the loader built for the `MREx` regions: one byte per
        // page of them, which is the `extra_size` just counted from the same table.
        unsafe {
            self.extra_allocations = slice::from_raw_parts_mut(xpt_base as *mut Option<PID>, extra_size)
        }
        Ok(())
    }

    /// Print the number of RAM bytes used by the specified process.
    /// This does not include memory such as peripherals and CSRs.
    #[cfg(baremetal)]
    pub fn ram_used_by(&self, pid: PID) -> usize {
        let mut owned_bytes = 0;
        #[cfg(baremetal)]
        for owner in &self.allocations[0..self.ram_size / PAGE_SIZE] {
            if owner == &Some(pid) {
                owned_bytes += PAGE_SIZE;
            }
        }
        #[cfg(baremetal)]
        owned_bytes
    }

    /// Allocate a single page to the given process, charged to its budget (R6): `OutOfMemory` if
    /// the budget cannot pay. DOES NOT ZERO THE PAGE!!! This function CANNOT zero the page, as
    /// it hasn't been mapped yet.
    #[cfg(baremetal)]
    pub fn alloc_page(&mut self, pid: PID) -> Result<usize, xous_kernel::Error> {
        let index = self.alloc_frame(pid)?;
        if self.charge_frame(pid).is_err() {
            self.allocations[index] = None;
            return Err(xous_kernel::Error::OutOfMemory);
        }
        Ok(self.ram_start + index * PAGE_SIZE)
    }

    /// Allocate a page to `pid` without charging it: only for the frames holding a process's
    /// saved thread contexts (`ProcessImpl`). The cost table's process page and a page per thread
    /// already pay for them, so charging the frames too would count them twice.
    #[cfg(baremetal)]
    #[allow(dead_code)] // WP-K4's `process_create`, through `MemoryMapping::allocate`
    pub fn alloc_context_page(&mut self, pid: PID) -> Result<usize, xous_kernel::Error> {
        Ok(self.ram_start + self.alloc_frame(pid)? * PAGE_SIZE)
    }

    /// Take a free frame for `owner`; its index in the ownership table.
    #[cfg(baremetal)]
    fn alloc_frame(&mut self, owner: PID) -> Result<usize, xous_kernel::Error> {
        // First fit. (The previous next-fit search computed its starting point with `max`
        // where `min` was meant, so it always scanned from the start anyway.)
        let index =
            self.allocations.iter().position(Option::is_none).ok_or(xous_kernel::Error::OutOfMemory)?;
        self.allocations[index] = Some(owner);
        Ok(index)
    }

    /// A zeroed frame for a kernel object, owned by `OBJECT_OWNER`. The caller charges it to the
    /// budget the cost table names. `OutOfMemory` only if RAM itself is exhausted.
    #[cfg(baremetal)]
    pub fn alloc_object_frame(&mut self) -> Result<u32, redoubt_sys::Error> {
        let index = self.alloc_frame(OBJECT_OWNER).map_err(|_| redoubt_sys::Error::OutOfMemory)?;
        let phys = self.ram_start + index * PAGE_SIZE;
        for offset in (0..PAGE_SIZE).step_by(8) {
            crate::kframe::write(phys, offset, 0);
        }
        self.objects.high_frame = self.objects.high_frame.max(index as u32);
        Ok(index as u32)
    }

    #[cfg(baremetal)]
    pub fn free_object_frame(&mut self, frame: u32) {
        self.object_phys(frame);
        self.allocations[frame as usize] = None;
    }

    /// Whether RAM frame `frame` holds a kernel object.
    #[cfg(baremetal)]
    pub fn is_object_frame(&self, frame: u32) -> bool {
        self.allocations.get(frame as usize) == Some(&Some(OBJECT_OWNER))
    }

    /// The physical address of kernel-object frame `frame`. A frame that is not one means a
    /// stale reference to a freed object: a violated invariant (I1), so the kernel stops.
    #[cfg(baremetal)]
    pub fn object_phys(&self, frame: u32) -> usize {
        assert!(self.is_object_frame(frame), "I1: {} is no object frame", frame);
        self.ram_start + frame as usize * PAGE_SIZE
    }

    /// RAM frames in the ownership table.
    #[cfg(baremetal)]
    pub fn ram_frames(&self) -> u64 { self.allocations.len() as u64 }

    /// RAM frames owned by `pid` in the ownership table.
    #[cfg(baremetal)]
    pub fn ram_frames_owned_by(&self, pid: PID) -> usize {
        self.allocations.iter().filter(|owner| **owner == Some(pid)).count()
    }

    /// Find a virtual address in the current process that is big enough
    /// to fit `size` bytes.
    pub fn find_virtual_address(
        &mut self,
        virt_ptr: *mut u8,
        size: usize,
        kind: xous_kernel::MemoryType,
    ) -> Result<*mut u8, xous_kernel::Error> {
        // If we were supplied a perfectly good address, return that.
        if !virt_ptr.is_null() {
            return Ok(virt_ptr);
        }

        // let process = Process::current();
        Process::with_inner_mut(|process_inner| {
            let (start, end, initial) = match kind {
                xous_kernel::MemoryType::Stack => return Err(xous_kernel::Error::BadAddress),
                xous_kernel::MemoryType::Heap => {
                    let new_virt = process_inner.mem_heap_base + process_inner.mem_heap_size + PAGE_SIZE;
                    if new_virt + size > process_inner.mem_heap_base + process_inner.mem_heap_max {
                        return Err(xous_kernel::Error::OutOfMemory);
                    }
                    return Ok(new_virt as *mut u8);
                }
                xous_kernel::MemoryType::Default => (
                    process_inner.mem_default_base,
                    process_inner.mem_default_base + 0x1000_0000,
                    process_inner.mem_default_last,
                ),
                xous_kernel::MemoryType::Messages => (
                    process_inner.mem_message_base,
                    process_inner.mem_message_base + 0x40_0000, // Limit to one superpage
                    process_inner.mem_message_last,
                ),
            };

            // A request larger than the whole region fits nowhere (and `end - size` would wrap).
            let Some(last_start) = end.checked_sub(size).filter(|last| *last >= start) else {
                return Err(xous_kernel::Error::BadAddress);
            };
            // Look for a sequence of `size` pages that are free.
            for potential_start in (initial..last_start).step_by(PAGE_SIZE) {
                let mut all_free = true;
                for check_page in (potential_start..potential_start + size).step_by(PAGE_SIZE) {
                    if !crate::arch::mem::address_available(check_page) {
                        all_free = false;
                        break;
                    }
                }
                if all_free {
                    match kind {
                        xous_kernel::MemoryType::Default => process_inner.mem_default_last = potential_start,
                        xous_kernel::MemoryType::Messages => process_inner.mem_message_last = potential_start,
                        other => panic!("invalid kind: {:?}", other),
                    }
                    return Ok(potential_start as *mut u8);
                }
            }

            for potential_start in (start..initial).step_by(PAGE_SIZE) {
                let mut all_free = true;
                for check_page in (potential_start..potential_start + size).step_by(PAGE_SIZE) {
                    if !crate::arch::mem::address_available(check_page) {
                        all_free = false;
                        break;
                    }
                }
                if all_free {
                    match kind {
                        xous_kernel::MemoryType::Default => process_inner.mem_default_last = potential_start,
                        xous_kernel::MemoryType::Messages => process_inner.mem_message_last = potential_start,
                        other => panic!("invalid kind: {:?}", other),
                    }
                    return Ok(potential_start as *mut u8);
                }
            }
            Err(xous_kernel::Error::BadAddress)
        })
    }

    /// Reserve the given range without actually allocating memory.
    /// That way we can overpromise on stack size and heap size without
    /// needing to actually have pages to back it.
    pub fn reserve_range(
        &mut self,
        virt_ptr: *mut u8,
        size: usize,
        flags: MemoryFlags,
    ) -> Result<xous_kernel::MemoryRange, xous_kernel::Error> {
        // If no address was specified, pick the next address that fits
        // in the "default" range
        let virt = self.find_virtual_address(virt_ptr, size, xous_kernel::MemoryType::Default)? as usize;

        if virt & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        if size & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        let mut mm = MemoryMapping::current();
        for addr in (virt..(virt + size)).step_by(PAGE_SIZE) {
            if let Err(e) = mm.reserve_address(self, addr, flags) {
                // Roll back the prefix we already reserved.
                for undo in (virt..addr).step_by(PAGE_SIZE) {
                    mm.unreserve_address(undo).expect("Internal error: couldn't unwind failed reservation");
                }
                return Err(e);
            }
        }
        crate::mem::memory_range(virt as usize, size)
    }

    /// Attempt to allocate a single page from the default section.
    /// Note that this will be backed by a real page.
    #[cfg(baremetal)]
    pub fn map_zeroed_page(&mut self, pid: PID, is_user: bool) -> Result<*mut usize, xous_kernel::Error> {
        let virt =
            self.find_virtual_address(core::ptr::null_mut(), PAGE_SIZE, xous_kernel::MemoryType::Default)?
                as usize;

        // Grab the next available page.  This claims it for this process.
        let phys = self.alloc_page(pid)?;

        // Actually perform the map.  At this stage, every physical page should be owned by us.
        if let Err(e) = crate::arch::mem::map_page_inner(
            self,
            pid,
            phys as usize,
            virt as usize,
            xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W,
            false,
        ) {
            self.release_page(phys as *mut usize, pid).ok();
            return Err(e);
        }

        let virt = virt as *mut usize;

        // Zero-out the page
        let range_start = virt;
        let range_end = range_start.wrapping_add(PAGE_SIZE / core::mem::size_of::<usize>());
        // SAFETY: `bzero` zeroes the page just mapped at `virt`, which the kernel owns until handed out.
        unsafe {
            crate::mem::bzero(range_start, range_end);
        };
        if is_user {
            crate::arch::mem::hand_page_to_user(virt as _)?;
        }
        // klog!(
        //     "Mapped {:08x} -> {:08x} (user? {})",
        //     phys as usize, virt as usize, is_user
        // );
        Ok(virt)
    }

    #[cfg_attr(not(baremetal), allow(dead_code))]
    pub fn is_main_memory(&self, phys: *mut u8) -> bool {
        (phys as usize) >= self.ram_start && (phys as usize) < self.ram_start + self.ram_size
    }

    /// This test is needed because peripheral memory is unmappable, but peripherals are not.
    /// The characteristic of peripheral memory is that it is:
    ///   - not in the allocation pool for stack/heap RAM
    ///   - primarily used for I/O buffers
    ///   - in an isolated address space
    ///   - multi-purpose in nature (i.e., not a dedicated frame buffer that would only have one sensible
    ///     driver mapping)
    /// The last point - the fact that the memory could have multiple purposes - is the reason that
    /// drives the need to potentially unmap it, as it may need to be handed off between drivers
    /// that are mutually exclusive in use.
    ///
    /// No platform we target (QEMU virt) has peripheral RAM, so this is always false; it is
    /// kept as the extension point for one that does.
    #[allow(dead_code)]
    pub fn is_peripheral_ram(&self, _phys: usize) -> bool { false }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn map_range(
        &mut self,
        phys_ptr: *mut u8,
        virt_ptr: *mut u8,
        size: usize,
        pid: PID,
        flags: MemoryFlags,
        kind: xous_kernel::MemoryType,
    ) -> Result<xous_kernel::MemoryRange, xous_kernel::Error> {
        #[cfg(baremetal)]
        let mut phys = phys_ptr as usize;
        #[cfg(not(baremetal))]
        let phys = phys_ptr as usize;
        let virt = self.find_virtual_address(virt_ptr, size, kind)?;

        // Determine if a contiguous chunk of RAM needs to be allocated for a device
        // This case happens when we don't specify a physical address, but also specify the DEV
        // flag.
        let device_ram = (flags & MemoryFlags::DEV == MemoryFlags::DEV) && (phys == 0);

        // If no physical address is specified, give the user the next available pages
        if phys == 0 && !device_ram {
            return self.reserve_range(virt, size, flags);
        }

        #[cfg(baremetal)]
        if device_ram {
            // Device RAM allocation: search for contiguous block of physical RAM so we can share
            // the pages with e.g. DMA or other hardware resources.
            let pages_to_claim = size / PAGE_SIZE; // this is correct because only page-sized requests are allowed.

            // Safety: is it safe to iterate through MEMORY_ALLOCATIONS like this? I'm not actually sure.
            // I suppose we could end up in trouble if the kernel is interrupted and the allocation table
            // changes, but we don't have a locking mechanism for this sort of thing (yet). Until we have
            // a better way to do this, it will have to do!
            let mut range_start: Option<usize> = None;
            let mut current_run = 0;
            for (index, entry) in self.allocations.iter().enumerate() {
                if entry.is_none() {
                    if let Some(_start) = range_start {
                        current_run += 1;
                    } else {
                        range_start = Some(index);
                        current_run = 1;
                    }
                } else {
                    range_start = None;
                    current_run = 0;
                }
                if current_run >= pages_to_claim {
                    break;
                }
            }
            if let Some(start) = range_start {
                if current_run >= pages_to_claim {
                    // success!
                    phys = start * PAGE_SIZE + self.ram_start;
                } else {
                    return Err(xous_kernel::Error::OutOfMemory);
                }
            } else {
                // couldn't find a contiguous location large enough; OOM for now.
                // Punt to userland to clear up memory.
                return Err(xous_kernel::Error::OutOfMemory);
            }
        }

        // 1. Attempt to claim all physical pages in the range
        for claim_phys in (phys..(phys + size)).step_by(PAGE_SIZE) {
            if let Err(err) = self.claim_page(claim_phys as *mut usize, pid) {
                // If we were unable to claim one or more pages, release everything and return
                for rel_phys in (phys..claim_phys).step_by(PAGE_SIZE) {
                    self.release_page(rel_phys as *mut usize, pid).ok();
                }
                return Err(err);
            } else {
            }
        }
        // Actually perform the map.  At this stage, every physical page should be owned by us.
        for offset in (0..size).step_by(PAGE_SIZE) {
            if let Err(e) = crate::arch::mem::map_page_inner(
                self,
                pid,
                offset + phys as usize,
                offset + virt as usize,
                flags,
                false,
            ) {
                for unmap_offset in (0..offset).step_by(PAGE_SIZE) {
                    crate::arch::mem::unmap_page_inner(self, unmap_offset + virt as usize).ok();
                    self.release_page((unmap_offset + phys) as *mut usize, pid).ok();
                }
                return Err(e);
            }
        }

        if device_ram {
            // The assumption is that device_ram pages are only ever allocated by a userspace call.
            // If the kernel uses this to allocate kernel structures, it will fail.

            // clear the memory first
            // SAFETY: zeroes the freshly mapped, kernel-owned pages before they are handed to userspace.
            unsafe { crate::mem::bzero(virt, virt.wrapping_add(size)) };

            // now hand it to userspace
            for offset in (0..size).step_by(PAGE_SIZE) {
                crate::arch::mem::hand_page_to_user(virt.wrapping_add(offset))
                    .expect("couldn't hand page to user");
            }

            // sanity check it
        }

        crate::mem::memory_range(virt as usize, size)
    }

    /// Attempt to map the given physical address into the virtual address space
    /// of this process.
    ///
    /// # Errors
    ///
    /// * MemoryInUse - The specified page is already mapped
    pub fn unmap_page(&mut self, virt: *mut usize) -> Result<usize, xous_kernel::Error> {
        let pid = crate::arch::process::current_pid();

        // If the virtual address has an assigned physical address, release that
        // address from this process.
        if let Ok(phys) = crate::arch::mem::virt_to_phys(virt as usize) {
            self.release_page(phys as *mut usize, pid).ok();
        }

        // Free the virtual address.
        crate::arch::mem::unmap_page_inner(self, virt as usize)
    }

    /// Move a page from one process into another, keeping its permissions.
    #[allow(dead_code)]
    pub fn move_page(
        &mut self,
        src_pid: PID,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
    ) -> Result<(), xous_kernel::Error> {
        let phys_addr = crate::arch::mem::virt_to_phys(src_addr as usize)?;
        crate::arch::mem::move_page_inner(self, src_mapping, src_addr, dest_pid, dest_mapping, dest_addr)?;
        self.claim_release_move(phys_addr as *mut usize, dest_pid, ClaimReleaseMove::Move(src_pid))
    }

    #[allow(dead_code)]
    /// Move the page in the process mapping listing without manipulating
    /// the pagetables at all.
    pub fn move_page_raw(&mut self, phys_addr: *mut usize, dest_pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(
            phys_addr as *mut usize,
            dest_pid,
            ClaimReleaseMove::Move(crate::arch::process::current_pid()),
        )
    }

    /// Mark the page in the current process as being lent.  If the borrow is
    /// read-only, then additionally remove the "write" bit on it.  If the page
    /// is writable, then remove it from the current process until the borrow is
    /// returned.
    #[allow(dead_code)]
    pub fn lend_page(
        &mut self,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
        mutable: bool,
    ) -> Result<usize, xous_kernel::Error> {
        // If this page is to be writable, detach it from this process.
        // Otherwise, mark it as read-only to prevent a process from modifying
        // the page while it's borrowed.
        crate::arch::mem::lend_page_inner(
            self,
            src_mapping,
            src_addr as _,
            dest_pid,
            dest_mapping,
            dest_addr as _,
            mutable,
        )
    }

    /// Return the range from `src_mapping` back to `dest_mapping`
    #[allow(dead_code)]
    pub fn unlend_page(
        &mut self,
        src_mapping: &MemoryMapping,
        src_addr: *mut u8,
        dest_pid: PID,
        dest_mapping: &MemoryMapping,
        dest_addr: *mut u8,
    ) -> Result<usize, xous_kernel::Error> {
        // If this page is to be writable, detach it from this process.
        // Otherwise, mark it as read-only to prevent a process from modifying
        // the page while it's borrowed.
        crate::arch::mem::return_page_inner(self, src_mapping, src_addr, dest_pid, dest_mapping, dest_addr)
    }

    /// A frame changes hands between two processes that are not the running one: a transfer
    /// (R4) or an abandoned lend (R3). The budgets follow the frame, as they do for every other
    /// ownership change.
    #[cfg(baremetal)]
    pub fn move_frame(&mut self, phys: usize, from: PID, to: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(phys as *mut usize, to, ClaimReleaseMove::Move(from))
    }

    /// Free a frame `pid` owns (an abandoned lend the server replied to, R3).
    #[cfg(baremetal)]
    pub fn free_frame_of(&mut self, phys: usize, pid: PID) {
        self.release_page(phys as *mut usize, pid).ok();
    }

    /// Back every demand-paged page of `[address, address + len)` in the current address
    /// space, so that the range can be lent or moved. Callers hold the memory manager
    /// already, which is why the backing takes `self` instead of borrowing it again.
    #[cfg(baremetal)]
    pub fn ensure_range_exists(&mut self, address: usize, len: usize) -> Result<(), xous_kernel::Error> {
        let end = address.checked_add(len).ok_or(xous_kernel::Error::BadAddress)?;
        for page in (address..end).step_by(PAGE_SIZE) {
            crate::arch::mem::ensure_page_exists_inner(self, page)?;
        }
        Ok(())
    }

    /// Refuse to move `[address, address + len)` of the current address space unless every
    /// page is a frame credited to `pid` in the ownership table. A page `pid` was only lent is
    /// credited to its lender, not to `pid`, so it fails this check; `move_page` would discover
    /// the mismatch only after changing the page tables, too late to back out. The pages must
    /// already be backed (`ensure_range_exists`), so each has a frame to check.
    #[cfg(baremetal)]
    pub fn check_owned_range(&self, pid: PID, address: usize, len: usize) -> Result<(), xous_kernel::Error> {
        let end = address.checked_add(len).ok_or(xous_kernel::Error::BadAddress)?;
        for page in (address..end).step_by(PAGE_SIZE) {
            let phys = crate::arch::mem::virt_to_phys(page)?;
            let owner = if self.is_main_memory(phys as *mut u8) {
                self.allocations[(phys - self.ram_start) / PAGE_SIZE]
            } else {
                self.extra_index(phys).and_then(|index| self.extra_allocations[index])
            };
            if owner != Some(pid) {
                return Err(xous_kernel::Error::ShareViolation);
            }
        }
        Ok(())
    }

    /// Claim the given memory for the given process, or release the memory
    /// back to the free pool.
    #[cfg(not(baremetal))]
    fn claim_release_move(
        &mut self,
        _addr: *mut usize,
        _pid: PID,
        _action: ClaimReleaseMove,
    ) -> Result<(), xous_kernel::Error> {
        Ok(())
    }

    #[cfg(baremetal)]
    fn claim_release_move(
        &mut self,
        addr: *mut usize,
        pid: PID,
        action: ClaimReleaseMove,
    ) -> Result<(), xous_kernel::Error> {
        /// Modify the memory tracking table to note which process owns
        /// the specified address.
        fn action_inner(
            owner_addr: &mut Option<PID>,
            pid: PID,
            action: ClaimReleaseMove,
            allow_alias: bool,
            addr: usize,
        ) -> Result<(), xous_kernel::Error> {
            if let Some(current_pid) = *owner_addr {
                if current_pid != pid {
                    // klog!(
                    //     "In claim_or_release({}, {}, {:?}) -- addr is owned by {} not {}",
                    //     owner_addr.map(|v| v.get()).unwrap_or_default(),
                    //     pid,
                    //     action,
                    //     current_pid,
                    //     pid
                    // );
                    if let ClaimReleaseMove::Move(existing_pid) = action {
                        if existing_pid != current_pid {
                            return Err(xous_kernel::Error::MemoryInUse);
                        }
                    } else {
                        return Err(xous_kernel::Error::MemoryInUse);
                    }
                }
            }
            match action {
                ClaimReleaseMove::Claim => {
                    if allow_alias {
                        if let Some(previous_owner) = owner_addr {
                            if previous_owner.get() == pid.get() {
                                // only allow aliases within the same process
                                println!(
                                    "WARN: aliasing physical address {:x} {:?} (requester: {:?})",
                                    addr, owner_addr, pid
                                );
                            } else {
                                return Err(xous_kernel::Error::MemoryInUse);
                            }
                        }
                        *owner_addr = Some(pid);
                    } else {
                        if owner_addr.is_none() {
                            *owner_addr = Some(pid);
                        } else {
                            println!(
                                "ERR: physical address {:x} already used by {:?} (requester: {:?})",
                                addr, owner_addr, pid
                            );
                            return Err(xous_kernel::Error::MemoryInUse);
                        }
                    }
                }
                ClaimReleaseMove::Move(_) => {
                    *owner_addr = Some(pid);
                }
                ClaimReleaseMove::Release => {
                    *owner_addr = None;
                }
            }
            Ok(())
        }

        let addr = addr as usize;

        // Ensure the address lies on a page boundary
        if cfg!(baremetal) && addr & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        let mut offset = 0;
        // Happy path: The address is in main RAM
        if self.is_main_memory(addr as *mut u8) {
            offset += (addr - self.ram_start) / PAGE_SIZE;
            let before = self.allocations[offset];
            action_inner(&mut self.allocations[offset], pid, action, false, addr)?;
            // A RAM frame changing owner changes who pays for it (R6). The new owner's budget
            // may refuse; then nothing changes.
            let after = self.allocations[offset];
            if before != after {
                // Uncharge first, so a move between two processes of one budget nets to nothing.
                if let Some(old) = before {
                    self.uncharge_frame(old);
                }
                if let Some(new) = after {
                    if self.charge_frame(new).is_err() {
                        self.allocations[offset] = before;
                        if let Some(old) = before {
                            self.charge_frame(old).expect("re-charging the page just uncharged");
                        }
                        return Err(xous_kernel::Error::OutOfMemory);
                    }
                }
            }
            return Ok(());
        }

        offset = 0;
        // Go through additional regions looking for this address, and claim it
        // if it's not in use.
        for region in self.extra_regions() {
            if region.contains(addr) {
                offset += (addr - region.start) / PAGE_SIZE;
                if self.is_peripheral_ram(offset) {
                    // don't allow aliasing of peripheral RAM, because peripheral RAM can be unmapped
                    return action_inner(&mut self.extra_allocations[offset], pid, action, false, addr);
                } else {
                    // aliasing is allowed, however, unmapping is NOT allowed. This allows us to not have
                    // to do reference counting to avoid unmap races
                    return action_inner(&mut self.extra_allocations[offset], pid, action, true, addr);
                }
            }
            offset += region.size / PAGE_SIZE;
        }
        // println!(
        //     "mem: unable to claim or release physical address {:08x}",
        //     addr
        // );
        Err(xous_kernel::Error::BadAddress)
    }

    /// Mark a given address as being owned by the specified process ID
    fn claim_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(addr, pid, ClaimReleaseMove::Claim)
    }

    /// Mark a given address as no longer being owned by the specified process ID
    fn release_page(&mut self, addr: *mut usize, pid: PID) -> Result<(), xous_kernel::Error> {
        self.claim_release_move(addr, pid, ClaimReleaseMove::Release)
    }

    /// The regions of the loader's `MREx` table, in order.
    ///
    /// The iterator walks the `'static` table itself, copied out of `self` first, so it does
    /// not borrow the memory manager: callers iterate the regions while claiming pages in
    /// `extra_allocations`. `use<>` states that, keeping `self`'s lifetime out of the
    /// returned type.
    #[cfg(baremetal)]
    fn extra_regions(&self) -> impl Iterator<Item = MemoryRangeExtra> + use<> {
        let table: &'static [u32] = self.extra_regions;
        table.chunks_exact(MemoryRangeExtra::WORDS).map(MemoryRangeExtra::from_words)
    }

    /// The index into `extra_allocations` for a physical address in one of the extra
    /// (device) regions, if any.
    #[cfg(baremetal)]
    fn extra_index(&self, phys: usize) -> Option<usize> {
        let mut base = 0;
        for region in self.extra_regions() {
            if region.contains(phys) {
                return Some(base + (phys - region.start) / PAGE_SIZE);
            }
            base += region.size / PAGE_SIZE;
        }
        None
    }

    /// Free all memory that belongs to a process. This does not unmap the memory from the
    /// process, it only marks it as free. Because a freed frame can be re-allocated
    /// immediately, only call this as part of destroying a process.
    ///
    /// # Safety
    /// Only sound as the final step of destroying `pid`: after this, frames it owned may
    /// be handed to other processes, so `pid` must never run again.
    pub unsafe fn release_all_memory_for_process(&mut self, pid: PID, space: &MemoryMapping) {
        #[cfg(baremetal)]
        {
            let kernel = PID::new(1).unwrap();

            // Pass 1: a frame this process has lent out is still mapped in the borrower.
            // Reparent it to the kernel so the frame is not reused while the borrower holds
            // it; it is freed when the borrower returns it. Which frames are lent is read
            // from this process's own page table, where the "shared" bit actually lives --
            // not guessed from a physical address. (INTERIM: the kernel then holds such a
            // frame uncharged; under answer 70, WP-K2's lends are charged to the borrower too
            // while the call is open, and to it alone once abandoned, R3.)
            space.for_each_lent_frame(|phys| {
                if self.is_main_memory(phys as *mut u8) {
                    let idx = (phys - self.ram_start) / PAGE_SIZE;
                    {
                        self.allocations[idx] = Some(kernel);
                    }
                } else if let Some(idx) = self.extra_index(phys) {
                    self.extra_allocations[idx] = Some(kernel);
                }
            });

            // Pass 2: free every frame still owned by this process.
            for idx in 0..self.allocations.len() {
                if self.allocations[idx] == Some(pid) {
                    self.allocations[idx] = None;
                }
            }
            for idx in 0..self.extra_allocations.len() {
                if self.extra_allocations[idx] == Some(pid) {
                    self.extra_allocations[idx] = None;
                }
            }
            // Both passes took RAM frames away from `pid`.
            self.uncharge_all_frames(pid);
        }
        #[cfg(not(baremetal))]
        let _ = (pid, space);
    }

    /// Adjust the flags on the given memory range. This allows for stripping flags from a memory
    /// range but does not allow adding flags. The memory range must exist, and the flags must be valid.
    pub fn update_memory_flags(
        &mut self,
        range: MemoryRange,
        flags: MemoryFlags,
    ) -> Result<(), xous_kernel::Error> {
        let virt = range.as_mut_ptr() as usize;
        let size = range.len();
        if virt & (PAGE_SIZE - 1) != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        if size & (PAGE_SIZE - 1) != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        // Pre-check the range to ensure the new flags are valid
        for virt in (virt..(virt + size)).step_by(PAGE_SIZE) {
            let existing_flags = crate::arch::mem::page_flags(virt).ok_or(xous_kernel::Error::MemoryInUse)?;
            // If the new flags add to the range, return an error.
            if !(!existing_flags & flags).is_empty() {
                return Err(xous_kernel::Error::MemoryInUse);
            }
        }

        // Now that the flags are validated, perform the update. This is fine as long as
        // we're unicore.
        for virt in (virt..(virt + size)).step_by(PAGE_SIZE) {
            let existing_flags = crate::arch::mem::page_flags(virt).ok_or(xous_kernel::Error::MemoryInUse)?;
            // If the new flags add to the range, return an error.
            if !(!existing_flags & flags).is_empty() {
                return Err(xous_kernel::Error::MemoryInUse);
            }

            crate::arch::mem::update_page_flags(virt, flags)?;
        }

        Ok(())
    }

    #[cfg(all(baremetal, any(target_arch = "riscv32", target_arch = "riscv64")))]
    pub fn check_for_duplicates(&self) {
        use crate::services::SystemServices;

        SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Activate the debugging process and iterate through it,
            // noting down each active thread.
            for phys in (self.ram_start..self.ram_start + self.ram_size).step_by(PAGE_SIZE) {
                let mut owner = None;
                for pid in 1..crate::services::MAX_PROCESS_COUNT {
                    let pid = PID::new(pid as u8).unwrap();
                    let Ok(process) = system_services.get_process(pid) else {
                        continue;
                    };
                    let Ok(_) = process.activate() else {
                        continue;
                    };
                    match MemoryMapping::current().phys_to_virt(phys) {
                        Err(e) => {
                            println!("!!! ERROR {:?} !!!", e);
                            continue;
                        }
                        Ok(None) => continue,
                        Ok(Some(virt)) => {
                            let allocation_offset = (phys - self.ram_start) / PAGE_SIZE;
                            let existing_owner = &self.allocations[allocation_offset];
                            let eo = existing_owner;
                            if eo != &Some(pid) {
                                let is_lent = {
                                    if let Some(existing_owner) = eo {
                                        system_services
                                            .get_process(*existing_owner)
                                            .unwrap()
                                            .activate()
                                            .unwrap();
                                        let is_lent = if let Ok(Some(owned_address)) =
                                            MemoryMapping::current().phys_to_virt(phys)
                                        {
                                            crate::arch::mem::page_is_lent(owned_address as *mut u8)
                                        } else {
                                            false
                                        };
                                        system_services.get_process(pid).unwrap().activate().unwrap();
                                        is_lent
                                    } else {
                                        false
                                    }
                                };
                                println!(
                                    "!!! 0x{:08x} is owned by {} ({}) but is mapped to {} ({}) -- {}",
                                    phys,
                                    eo.map(|v| v.get() as isize).unwrap_or(-1),
                                    eo.map(|v| system_services.process_name(v).unwrap_or("<unknown>"))
                                        .unwrap_or("<none>"),
                                    pid.get(),
                                    system_services.process_name(pid).unwrap_or("<unknown>"),
                                    if is_lent { "page is lent" } else { "duplicate!" },
                                );
                            }
                            if !crate::arch::mem::page_is_lent(virt as *mut u8) {
                                if owner.is_none() {
                                    owner = Some((pid, virt));
                                } else {
                                    println!(
                                        "!!! DUPLICATE !!! Page {:08x} owned by both {} ({}) @ {:08x} and {} ({}) @ {:08x}",
                                        phys,
                                        owner.map(|v| v.0.get() as isize).unwrap_or(-1),
                                        owner
                                            .map(|v| system_services.process_name(v.0).unwrap_or("<unknown>"))
                                            .unwrap_or("<none>"),
                                        owner.map(|v| v.1).unwrap_or(0),
                                        pid.get(),
                                        system_services.process_name(pid).unwrap_or("<unknown>"),
                                        virt,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Restore the previous PID
            system_services.get_process(current_pid).unwrap().activate().unwrap();
        })
    }
}

/// Zero the memory in `start..end` with volatile writes.
///
/// # Safety
/// `start..end` must be a single valid, writable, `T`-aligned allocation the caller owns.
pub unsafe fn bzero<T>(mut start: *mut T, end: *mut T)
where
    T: Copy,
{
    while start < end {
        // NOTE(volatile) to prevent this from being transformed into `memclr`
        core::ptr::write_volatile(start, core::mem::zeroed());
        start = start.offset(1);
    }
}
