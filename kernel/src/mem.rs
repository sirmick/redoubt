// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use redoubt_layout::Pid;
use redoubt_sys::{MemFlags, PAGE_SIZE, USER_AREA_END};

pub use crate::arch::mem::MemoryMapping;
use crate::arch::process::Process;

#[derive(Debug)]
enum ClaimReleaseMove {
    Claim,
    Release,
    Move(Pid /* from */),
}

/// One entry of the loader's `MREx` table (kernel/boot.md): a device region, whose frames the
/// ownership table tracks.
#[derive(Clone, Copy)]
struct ExtraRegion {
    start: usize,
    size: usize,
}

impl ExtraRegion {
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
        ExtraRegion { start, size }
    }

    fn contains(&self, addr: usize) -> bool { addr >= self.start && addr - self.start < self.size }
}

/// Why the page layer refused: the page tables (`arch::mem`) and the frame ownership table here.
/// Kernel-internal: every system call maps it to the `redoubt_sys::Error` its row names, with
/// an explicit `map_err` at the call's boundary; there is no `From`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageError {
    /// Nothing mapped or reserved there, no table reaches it, or not a frame the table tracks.
    Unmapped,
    /// Not a canonical virtual address (Sv39).
    NonCanonical,
    /// Reserved for demand paging and not yet backed.
    Reserved,
    /// Either alias of a loan (the `S` bit), or not the loan the step expects.
    Lent,
    /// Already mapped, reserved or owned: the step needs it free.
    InUse,
    /// No free frame, or the owner's budget cannot pay for one (R6).
    NoFrame,
    /// No free range of that size in the placement area.
    NoSpace,
    /// Not page-aligned.
    Unaligned,
    /// Permissions the page tables refuse: W+X, W without R, or none (R11).
    BadFlags,
}

/// Where the first page of each placement area is, per process (`ProcessInner`): what
/// `find_virtual_address` searches when the caller names no address.
pub const DEFAULT_BASE: usize = 0x6000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;

/// A placement area: `map_anon`'s (`Default`, 256 MiB from `DEFAULT_BASE`) or the one received
/// messages are mapped into (`Messages`, one superpage from `DEFAULT_MESSAGE_BASE`).
#[derive(Clone, Copy, Debug)]
pub enum MemoryType {
    Default,
    Messages,
}

pub struct MemoryManager {
    ram_start: usize,
    ram_size: usize,
    ram_name: u32,
    /// Who owns each page of RAM, indexed by page number within RAM. The loader builds
    /// this table and hands it over in `init_from_memory`.
    allocations: &'static mut [RamAllocation],
    /// The same, for the pages of every region in `extra_regions`, back to back.
    extra_allocations: &'static mut [Option<Pid>],
    /// Memory outside RAM that processes may claim: memory-mapped devices. The data of the
    /// loader's `MREx` tag, `ExtraRegion::WORDS` words per region; see `extra_regions()`.
    extra_regions: &'static [u32],
    /// Budgets and the per-process ledger that charges them (`budget.rs`), and the handle tables
    /// (`handle.rs`). Here, beside the ownership table, because a frame changing owner is what
    /// most charges are.
    pub objects: crate::budget::Objects,
    /// DMA devices and the runs `dma_alloc` handed out through them (`dma.rs`). Here,
    /// beside the ownership table that names their frames' owner, `DMA_OWNER`.
    pub dma: crate::dma::Registry,
}

/// Owner, in the ownership table, of frames that hold kernel objects (budgets, handle-table
/// pages). No process has this PID (there are `MAX_PROCESS_COUNT` of them), so such a frame is
/// never mapped into a process, and `release_all_memory_for_process` never frees one.
pub const OBJECT_OWNER: Pid = match Pid::new(255) {
    Some(pid) => pid,
    None => unreachable!(),
};
const _: () = assert!(crate::arch::process::MAX_PROCESS_COUNT < 255);
/// Owner, in the ownership table, of `dma_alloc` frames (`dma.rs`). No process has this
/// PID either, so no generic release, move or lend path, all of which check that the caller
/// owns the frame, can free or move one: only `dma_release` pools it, after the reset.
pub const DMA_OWNER: Pid = match Pid::new(254) {
    Some(pid) => pid,
    None => unreachable!(),
};
const _: () = assert!(crate::arch::process::MAX_PROCESS_COUNT < 254);
type RamAllocation = Option<Pid>;

impl Default for MemoryManager {
    fn default() -> Self { Self::default_hack() }
}

/// Lock order: `ProcessTable` (ptable.rs) before `MemoryManager`. Code holding the memory
/// manager never takes the process table, so charging a budget from deep inside the allocator
/// (`alloc_page`) needs only this cell; code holding the process table may take this one.
static MEMORY_MANAGER: crate::cell::KernelCell<MemoryManager> =
    crate::cell::KernelCell::new(MemoryManager::default_hack());
/// as the process entry has not yet been created.
impl MemoryManager {
    const fn default_hack() -> Self {
        MemoryManager {
            ram_start: 0,
            ram_size: 0,
            ram_name: 0,
            allocations: &mut [],
            extra_allocations: &mut [],
            extra_regions: &[],
            objects: crate::budget::Objects::new(),
            dma: crate::dma::Registry::new(),
        }
    }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut MemoryManager) -> R,
    {
        MEMORY_MANAGER.with(f)
    }

    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&MemoryManager) -> R,
    {
        MEMORY_MANAGER.with(|mm| f(mm))
    }

    pub fn init_from_memory(
        &mut self,
        rpt_base: usize,
        xpt_base: usize,
        args: &crate::args::KernelArguments,
    ) -> Result<(), PageError> {
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
                    tag.data.len() % ExtraRegion::WORDS == 0,
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
        unsafe { self.allocations = slice::from_raw_parts_mut(rpt_base as *mut Option<Pid>, mem_size) };
        // SAFETY: as above, for the table the loader built for the `MREx` regions: one byte per
        // page of them, which is the `extra_size` just counted from the same table.
        unsafe {
            self.extra_allocations = slice::from_raw_parts_mut(xpt_base as *mut Option<Pid>, extra_size)
        }
        Ok(())
    }

    /// Allocate a single page to the given process, charged to its budget (R6): `OutOfMemory` if
    /// the budget cannot pay. DOES NOT ZERO THE PAGE!!! This function CANNOT zero the page, as
    /// it hasn't been mapped yet.
    pub fn alloc_page(&mut self, pid: Pid) -> Result<usize, PageError> {
        let index = self.alloc_frame(pid)?;
        if self.charge_frame(pid).is_err() {
            self.allocations[index] = None;
            return Err(PageError::NoFrame);
        }
        Ok(self.ram_start + index * PAGE_SIZE)
    }

    /// Allocate a page for a process's saved thread contexts (`ProcessImpl`), charged to the
    /// budget the process runs in like any other frame it owns (kernel/objects.md: the kernel
    /// charges what a process really costs instead of holding it back from `root` at boot).
    pub fn alloc_context_page(&mut self, pid: Pid) -> Result<usize, PageError> { self.alloc_page(pid) }

    /// Take a free frame for `owner`; its index in the ownership table.
    fn alloc_frame(&mut self, owner: Pid) -> Result<usize, PageError> {
        // First fit. (The previous next-fit search computed its starting point with `max`
        // where `min` was meant, so it always scanned from the start anyway.)
        let index = self.allocations.iter().position(Option::is_none).ok_or(PageError::NoFrame)?;
        self.allocations[index] = Some(owner);
        Ok(index)
    }

    /// A frame for the kernel itself (the test-only trace ring), taken at boot before the budget
    /// tree counts what the kernel keeps.
    #[cfg(feature = "sched-trace")]
    pub fn kernel_frame(&mut self) -> Result<usize, PageError> {
        let index = self.alloc_frame(redoubt_layout::KERNEL_PID)?;
        Ok(self.ram_start + index * PAGE_SIZE)
    }

    /// A zeroed frame for a kernel object, owned by `OBJECT_OWNER`. The caller charges it to the
    /// budget the cost table names. `OutOfMemory` only if RAM itself is exhausted.
    pub fn alloc_object_frame(&mut self) -> Result<u32, redoubt_sys::Error> {
        let index = self.alloc_frame(OBJECT_OWNER).map_err(|_| redoubt_sys::Error::OutOfMemory)?;
        crate::kframe::zero(self.ram_start + index * PAGE_SIZE);
        self.objects.high_frame = self.objects.high_frame.max(index as u32);
        Ok(index as u32)
    }

    pub fn free_object_frame(&mut self, frame: u32) {
        self.object_phys(frame);
        self.allocations[frame as usize] = None;
    }

    /// A dying object's frame is due to be freed. Inside a destruction the free is deferred past
    /// `destroy_marked`'s single handle sweep (I1: a handle may still name it, and the sweep reads
    /// its frame), linked through the frame's [`crate::budget::DEFER_WORD`]; outside one it goes
    /// at once.
    pub(crate) fn release_object_frame(&mut self, frame: u32) {
        if !self.objects.deferring {
            self.free_object_frame(frame);
            return;
        }
        let phys = self.object_phys(frame);
        crate::kframe::write(
            phys,
            crate::budget::DEFER_WORD * 8,
            crate::budget::frame_word(self.objects.deferred),
        );
        self.objects.deferred = Some(frame);
    }

    /// Free every frame a destruction deferred (after its sweep).
    pub(crate) fn free_deferred_frames(&mut self) {
        while let Some(frame) = self.objects.deferred {
            let phys = self.object_phys(frame);
            let next = (crate::kframe::read(phys, crate::budget::DEFER_WORD * 8) as u32).checked_sub(1);
            self.objects.deferred = next;
            self.free_object_frame(frame);
        }
    }

    /// Whether RAM frame `frame` holds a kernel object.
    pub fn is_object_frame(&self, frame: u32) -> bool {
        self.allocations.get(frame as usize) == Some(&Some(OBJECT_OWNER))
    }

    /// The physical address of kernel-object frame `frame`. A frame that is not one means a
    /// stale reference to a freed object: a violated invariant (I1), so the kernel stops.
    pub fn object_phys(&self, frame: u32) -> usize {
        assert!(self.is_object_frame(frame), "I1: {} is no object frame", frame);
        self.ram_start + frame as usize * PAGE_SIZE
    }

    /// RAM frames in the ownership table.
    pub fn ram_frames(&self) -> u64 { self.allocations.len() as u64 }

    /// Whether `[base, end)` touches any of RAM. A device object never may (R11: userspace
    /// never names RAM by physical address), so the boot checks every one against this.
    pub fn overlaps_ram(&self, base: u64, end: u64) -> bool {
        let (ram_start, ram_end) = (self.ram_start as u64, (self.ram_start + self.ram_size) as u64);
        base < ram_end && ram_start < end
    }

    /// `npages` contiguous free RAM frames, claimed for `owner` and zeroed through the physmap
    /// (R11: before any process can see them); the physical address of the first. Not charged:
    /// `dma_new_run`, the only caller, charges the run's budget itself (`dma.rs`).
    pub fn alloc_contiguous(&mut self, owner: Pid, npages: usize) -> Result<usize, redoubt_sys::Error> {
        // First fit over the ownership table, as `alloc_frame` is, with a run to fill.
        let mut run = 0;
        let start = self
            .allocations
            .iter()
            .position(|o| {
                run = if o.is_none() { run + 1 } else { 0 };
                run == npages
            })
            .map(|last| last + 1 - npages)
            .ok_or(redoubt_sys::Error::OutOfMemory)?;
        self.allocations[start..start + npages].fill(Some(owner));
        let phys = self.ram_start + start * PAGE_SIZE;
        for page in 0..npages {
            crate::kframe::zero(phys + page * PAGE_SIZE);
        }
        Ok(phys)
    }

    /// Give back `npages` frames from `phys` that `alloc_contiguous` claimed for `owner`.
    pub fn free_contiguous(&mut self, owner: Pid, phys: usize, npages: usize) {
        let start = (phys - self.ram_start) / PAGE_SIZE;
        for entry in &mut self.allocations[start..start + npages] {
            assert!(*entry == Some(owner), "I1: a contiguous run changed owner");
            *entry = None;
        }
    }

    /// Whether RAM frame `phys` is a `dma_alloc` frame (`DMA_OWNER`'s).
    pub fn is_dma_frame(&self, phys: usize) -> bool {
        self.is_main_memory(phys as *mut u8)
            && self.allocations[(phys - self.ram_start) / PAGE_SIZE] == Some(DMA_OWNER)
    }

    /// RAM frames owned by `pid` in the ownership table.
    pub fn ram_frames_owned_by(&self, pid: Pid) -> usize {
        self.allocations.iter().filter(|owner| **owner == Some(pid)).count()
    }

    /// Find a virtual address in the current process's area `kind` where `size` bytes of free
    /// pages fit, and remember it as the area's last placement.
    ///
    /// Every start in `[area start, area end - size]` is a candidate, the last one included, so a
    /// run that fits only at the area's end is found, and so is a request for the whole of an
    /// empty area. The scan begins at the lower of the last placement and the last start that
    /// fits, runs up to that last start, then wraps once to the area's start and runs up to where
    /// it began. It skips ahead: when page `p` is taken, no run through `p` fits, so the next
    /// candidate is `p` plus one page. Each page of the area is therefore looked at once, and a
    /// missing page table skips its whole span (`first_occupied_page`). So the search costs at
    /// most the area's pages, a fixed size, and never the area's pages times the request's
    /// (R12's bound on a call's kernel time, kernel/scheduling.md). Every run returned lies
    /// inside the area.
    pub fn find_virtual_address(
        &mut self,
        virt_ptr: *mut u8,
        size: usize,
        kind: MemoryType,
    ) -> Result<*mut u8, PageError> {
        // If we were supplied a perfectly good address, return that.
        if !virt_ptr.is_null() {
            return Ok(virt_ptr);
        }

        Process::with_inner_mut(|process_inner| {
            let (start, end, last) = match kind {
                MemoryType::Default => (
                    process_inner.mem_default_base,
                    process_inner.mem_default_base + 0x1000_0000,
                    process_inner.mem_default_last,
                ),
                MemoryType::Messages => (
                    process_inner.mem_message_base,
                    process_inner.mem_message_base + 0x40_0000, // Limit to one superpage
                    process_inner.mem_message_last,
                ),
            };

            // A request larger than the whole region fits nowhere (and `end - size` would wrap).
            let Some(last_start) = end.checked_sub(size).filter(|last| *last >= start) else {
                return Err(PageError::NoSpace);
            };
            let first = last.clamp(start, last_start);
            let taken = |at: usize, until: usize| crate::arch::mem::first_occupied_page(at, until);
            // From `first` up to the last start. The first page taken at or after `first`
            // (within one run of it) is remembered for the wrap.
            let mut candidate = first;
            let mut taken_after_first = None;
            let found = loop {
                if candidate > last_start {
                    break None;
                }
                match taken(candidate, candidate + size) {
                    None => break Some(candidate),
                    Some(page) => {
                        taken_after_first.get_or_insert(page);
                        candidate = page + PAGE_SIZE;
                    }
                }
            };
            // Then from the area's start up to `first`. `[first, taken_after_first)` is known to
            // be free and `taken_after_first` taken, so a run reaching into it is decided without
            // looking at those pages again.
            let found = found.or_else(|| {
                let mut candidate = start;
                while candidate < first {
                    let run_end = candidate + size;
                    match taken(candidate, run_end.min(first)) {
                        Some(page) => candidate = page + PAGE_SIZE,
                        None if taken_after_first.is_none_or(|page| run_end <= page) => {
                            return Some(candidate);
                        }
                        // The run reaches a taken page at or past `first`; so does every later
                        // start below `first`.
                        None => return None,
                    }
                }
                None
            });
            let at = found.ok_or(PageError::NoSpace)?;
            match kind {
                MemoryType::Default => process_inner.mem_default_last = at,
                MemoryType::Messages => process_inner.mem_message_last = at,
            }
            Ok(at as *mut u8)
        })
    }

    /// Reserve the given range without actually allocating memory.
    /// That way we can overpromise on stack size and heap size without
    /// needing to actually have pages to back it.
    pub fn reserve_range(
        &mut self,
        virt_ptr: *mut u8,
        size: usize,
        flags: MemFlags,
    ) -> Result<(), PageError> {
        // If no address was specified, pick the next address that fits
        // in the "default" range
        let virt = self.find_virtual_address(virt_ptr, size, MemoryType::Default)? as usize;

        if virt & 0xfff != 0 || size & 0xfff != 0 {
            return Err(PageError::Unaligned);
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
        Ok(())
    }

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
    pub fn is_peripheral_ram(&self, _phys: usize) -> bool { false }

    /// Map the device registers at `phys` into the virtual address space of `pid` (the kernel's
    /// own: the PLIC). Nothing here reserves pages to back later: reservations are only the
    /// loader programs' stacks (`reserve_range`, the Setup path).
    ///
    /// # Errors
    ///
    /// * InUse - The specified page is already mapped
    pub fn map_range(
        &mut self,
        phys_ptr: *mut u8,
        virt_ptr: *mut u8,
        size: usize,
        pid: Pid,
        flags: MemFlags,
        kind: MemoryType,
    ) -> Result<(), PageError> {
        let phys = phys_ptr as usize;
        let virt = self.find_virtual_address(virt_ptr, size, kind)?;

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
        Ok(())
    }

    /// A frame changes hands between two processes that are not the running one: a transfer
    /// (R4) or an abandoned lend (R3). The budgets follow the frame, as they do for every other
    /// ownership change.
    pub fn move_frame(&mut self, phys: usize, from: Pid, to: Pid) -> Result<(), PageError> {
        self.claim_release_move(phys as *mut usize, to, ClaimReleaseMove::Move(from))
    }

    /// Free a frame `pid` owns (an abandoned lend the server replied to, R3).
    pub fn free_frame_of(&mut self, phys: usize, pid: Pid) -> Result<(), PageError> {
        self.release_page(phys as *mut usize, pid)
    }

    /// Who owns RAM frame `phys` in the ownership table: the budget of that PID pays for it.
    /// `None` for a free frame or one outside RAM.
    pub fn ram_owner(&self, phys: usize) -> Option<Pid> {
        if !self.is_main_memory(phys as *mut u8) {
            return None;
        }
        self.allocations[(phys - self.ram_start) / PAGE_SIZE]
    }

    /// Back every demand-paged page of `[address, address + len)` in the current address
    /// space, so that the range can be lent or moved. Callers hold the memory manager
    /// already, which is why the backing takes `self` instead of borrowing it again.
    pub fn ensure_range_exists(&mut self, address: usize, len: usize) -> Result<(), PageError> {
        let end = address.checked_add(len).ok_or(PageError::Unmapped)?;
        for page in (address..end).step_by(PAGE_SIZE) {
            crate::arch::mem::ensure_page_exists_inner(self, page)?;
        }
        Ok(())
    }

    /// Refuse to move `[address, address + len)` of the current address space unless every
    /// page is a frame credited to `pid` in the ownership table. A page `pid` was only lent is
    /// credited to its lender, not to `pid`, so it fails this check; checked any later, the
    /// mismatch would surface only after the page tables had changed, too late to back out. The
    /// pages must already be backed (`ensure_range_exists`), so each has a frame to check.
    pub fn check_owned_range(&self, pid: Pid, address: usize, len: usize) -> Result<(), PageError> {
        let end = address.checked_add(len).ok_or(PageError::Unmapped)?;
        for page in (address..end).step_by(PAGE_SIZE) {
            let phys = crate::arch::mem::virt_to_phys(page)?;
            let owner = if self.is_main_memory(phys as *mut u8) {
                self.allocations[(phys - self.ram_start) / PAGE_SIZE]
            } else {
                self.extra_index(phys).and_then(|index| self.extra_allocations[index])
            };
            if owner != Some(pid) {
                return Err(PageError::Lent);
            }
        }
        Ok(())
    }

    fn claim_release_move(
        &mut self,
        addr: *mut usize,
        pid: Pid,
        action: ClaimReleaseMove,
    ) -> Result<(), PageError> {
        /// Modify the memory tracking table to note which process owns
        /// the specified address.
        fn action_inner(
            owner_addr: &mut Option<Pid>,
            pid: Pid,
            action: ClaimReleaseMove,
            allow_alias: bool,
            addr: usize,
        ) -> Result<(), PageError> {
            if let Some(current_pid) = *owner_addr {
                if current_pid != pid {
                    if let ClaimReleaseMove::Move(existing_pid) = action {
                        if existing_pid != current_pid {
                            return Err(PageError::InUse);
                        }
                    } else {
                        return Err(PageError::InUse);
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
                                return Err(PageError::InUse);
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
                            return Err(PageError::InUse);
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
        if addr & 0xfff != 0 {
            return Err(PageError::Unaligned);
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
                        return Err(PageError::NoFrame);
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
        Err(PageError::Unmapped)
    }

    /// Mark a given address as being owned by the specified process ID
    fn claim_page(&mut self, addr: *mut usize, pid: Pid) -> Result<(), PageError> {
        self.claim_release_move(addr, pid, ClaimReleaseMove::Claim)
    }

    /// Mark a given address as no longer being owned by the specified process ID
    fn release_page(&mut self, addr: *mut usize, pid: Pid) -> Result<(), PageError> {
        self.claim_release_move(addr, pid, ClaimReleaseMove::Release)
    }

    /// The regions of the loader's `MREx` table, in order.
    ///
    /// The iterator walks the `'static` table itself, copied out of `self` first, so it does
    /// not borrow the memory manager: callers iterate the regions while claiming pages in
    /// `extra_allocations`. `use<>` states that, keeping `self`'s lifetime out of the
    /// returned type.
    fn extra_regions(&self) -> impl Iterator<Item = ExtraRegion> + use<> {
        let table: &'static [u32] = self.extra_regions;
        table.chunks_exact(ExtraRegion::WORDS).map(ExtraRegion::from_words)
    }

    /// The index into `extra_allocations` for a physical address in one of the extra
    /// (device) regions, if any.
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
    pub unsafe fn release_all_memory_for_process(&mut self, pid: Pid, space: &MemoryMapping) {
        {
            let kernel = Pid::new(1).unwrap();

            // One walk of the process's own tables, never of every frame of RAM (R12;
            // kernel/budgets.md, "Residual risks"). A frame it has lent out is still mapped in
            // the borrower: it is reparented to the kernel so it is not reused while the
            // borrower holds it, and freed when the borrower returns it (the caller's charge
            // ends here; the lend stays charged to the server that holds it, kernel/ipc.md R3).
            // Every other frame its tables name that is still credited to it goes back, the
            // tables themselves included; a borrowed page is its lender's. A page lent to itself
            // ends the kernel's whichever of its two entries the walk meets first.
            space.for_each_owned_frame(|phys, lent| {
                let owner = if self.is_main_memory(phys as *mut u8) {
                    &mut self.allocations[(phys - self.ram_start) / PAGE_SIZE]
                } else if let Some(idx) = self.extra_index(phys) {
                    &mut self.extra_allocations[idx]
                } else {
                    return;
                };
                // A DMA frame is never lent (kernel/devices.md), and stays `DMA_OWNER`'s
                // whatever happens: only `dma_release` pools it.
                if lent && *owner != Some(DMA_OWNER) {
                    *owner = Some(kernel);
                } else if !lent && *owner == Some(pid) {
                    *owner = None;
                }
            });
            self.uncharge_all_frames(pid);
        }
    }

    /// The checked build's proof that no frame is credited to a process that has ended: a frame
    /// its tables did not name would have been left out of `release_all_memory_for_process`. A
    /// scan of all of RAM, so never during a destruction: that build runs it once after the
    /// walk instead (`destroy_subtree`), where it does not scale the destruction.
    #[cfg(debug_assertions)]
    pub(crate) fn check_frame_owners(&self) {
        if self.objects.deferring {
            return;
        }
        let ended = |owner: &Option<Pid>| {
            owner.is_some_and(|pid| {
                pid.get() > 1
                    && usize::from(pid.get()) <= crate::arch::process::MAX_PROCESS_COUNT
                    && self.account(pid).is_none()
            })
        };
        assert!(!self.allocations.iter().any(ended), "I1: a RAM frame is credited to a process that ended");
        assert!(
            !self.extra_allocations.iter().any(ended),
            "I1: a device frame is credited to a process that ended"
        );
    }

    /// Give back every frame still owned by a process that never ran (`process_create`'s
    /// rollback). A scan of all of RAM, which needs no page-table access, so it serves a
    /// partially built space; an ended process is walked from its tables instead
    /// ([`MemoryManager::release_all_memory_for_process`]).
    pub fn release_owned_frames(&mut self, pid: Pid) {
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
        self.uncharge_all_frames(pid);
    }
}

// --- The Redoubt memory calls (kernel/memory.md; R11) ----------------------------------------
//
// `map_anon`, `unmap` and `set_flags`. Three rules of R11 shape them: no mapping is ever
// writable and executable (`Pte::leaf` refuses it, as decoding already did), every page is
// zeroed before a process first sees it, and **userspace never names an address**: the kernel
// chooses where each mapping lands, so none of these calls takes a physical address and only
// `map_anon` returns a virtual one.
//
// `map_anon` backs and charges every page at once rather than reserving it for demand paging.
// The spec's row says "pages charged", and a process that is told it has memory and then
// faults for want of it has been told a lie. The only reservations are the loader programs'
// stacks (`reserve_range`, the Setup path).
impl MemoryManager {
    /// A range argument: page-aligned, non-empty, and wholly inside user space. Its end.
    fn user_range(addr: usize, len: usize) -> Result<usize, redoubt_sys::Error> {
        let bad = redoubt_sys::Error::InvalidArgument;
        if len == 0 || len % PAGE_SIZE != 0 || addr % PAGE_SIZE != 0 {
            return Err(bad);
        }
        let end = addr.checked_add(len).ok_or(bad)?;
        if end > USER_AREA_END { Err(bad) } else { Ok(end) }
    }

    /// `map_anon(len, flags) -> addr`: zeroed pages, charged to the caller's budget, at an
    /// address the kernel chooses.
    pub fn map_anon(
        &mut self,
        pid: Pid,
        len: usize,
        flags: redoubt_sys::MemFlags,
    ) -> Result<usize, redoubt_sys::Error> {
        let bad = redoubt_sys::Error::InvalidArgument;
        if len == 0 || len % PAGE_SIZE != 0 {
            return Err(bad);
        }
        // The row's own check: no permission at all, or writable without readable.
        if flags == MemFlags::NONE || (flags.contains(MemFlags::WRITE) && !flags.contains(MemFlags::READ)) {
            return Err(bad);
        }
        let at = self.map_run(pid, len / PAGE_SIZE, flags, None)?;
        sync_if_executable(flags);
        Ok(at)
    }

    /// Map `npages` pages at an address the kernel chooses (R11), with `flags`. `phys` is the
    /// physical run to map (`map_device`'s registers, `dma_alloc`'s buffer), or `None` for
    /// fresh RAM: a frame each, charged to `pid`'s budget and zeroed before the mapping exists
    /// (R6, R11). The one way the kernel maps a range it chose the address of; on any failure
    /// nothing is left mapped, and a run the caller passed in stays the caller's to free.
    pub fn map_run(
        &mut self,
        pid: Pid,
        npages: usize,
        flags: MemFlags,
        phys: Option<usize>,
    ) -> Result<usize, redoubt_sys::Error> {
        let oom = redoubt_sys::Error::OutOfMemory;
        let len = npages.checked_mul(PAGE_SIZE).ok_or(oom)?;
        let at = self
            .find_virtual_address(core::ptr::null_mut(), len, MemoryType::Default)
            .map_err(|_| oom)? as usize;
        let ours = phys.is_none();
        for offset in (0..len).step_by(PAGE_SIZE) {
            let frame = match phys {
                Some(base) => base + offset,
                None => match self.alloc_page(pid) {
                    // Zeroed through the physmap, before the mapping exists at all (R11).
                    Ok(frame) => {
                        crate::kframe::zero(frame);
                        frame
                    }
                    Err(_) => return Err(self.undo_run(pid, at, offset, ours)),
                },
            };
            if crate::arch::mem::map_page_inner(self, pid, frame, at + offset, flags, true).is_err() {
                if ours {
                    self.release_page(frame as *mut usize, pid).ok();
                }
                return Err(self.undo_run(pid, at, offset, ours));
            }
        }
        Ok(at)
    }

    /// Give back what a failed `map_run` had already mapped, the pages it had allocated, and the
    /// page tables it made, the failed page's included (`map_page_inner` may have made a table
    /// before it ran out).
    fn undo_run(&mut self, pid: Pid, at: usize, done: usize, ours: bool) -> redoubt_sys::Error {
        for offset in (0..done).step_by(PAGE_SIZE) {
            if let Ok(frame) = crate::arch::mem::unmap_page_inner(self, at + offset) {
                if ours {
                    self.release_page(frame as *mut usize, pid).ok();
                }
            }
        }
        // One page past `done`: the failed page's table may exist though nothing in it is mapped.
        crate::arch::mem::free_empty_tables(self, &MemoryMapping::current(), at, at + done + PAGE_SIZE);
        redoubt_sys::Error::OutOfMemory
    }

    /// `unmap(addr, len)`: the whole range must be the caller's own mapping and not lent out
    /// (I9), checked before any page moves. A RAM frame goes back to the free pool and to the
    /// caller's budget; a device's registers are not RAM and only lose their mapping -- the
    /// MMIO page-ownership table is left alone, as `map_device` left it alone (the handle, not
    /// a page owner, is the authority there). A `dma_alloc` frame only loses its mapping too: it
    /// stays held, and charged, until the process ends (kernel/devices.md, `dma_alloc`). A page
    /// table the range leaves mapping nothing is freed, and uncharged, too.
    pub fn unmap(&mut self, pid: Pid, addr: usize, len: usize) -> Result<(), redoubt_sys::Error> {
        let end = Self::user_range(addr, len)?;
        for page in (addr..end).step_by(PAGE_SIZE) {
            self.owned_mapping(pid, page)?;
        }
        for page in (addr..end).step_by(PAGE_SIZE) {
            let phys = crate::arch::mem::unmap_page_inner(self, page).expect("checked just above");
            if self.is_main_memory(phys as *mut u8) && !self.is_dma_frame(phys) {
                self.release_page(phys as *mut usize, pid).ok();
            }
        }
        crate::arch::mem::free_empty_tables(self, &MemoryMapping::current(), addr, end);
        Ok(())
    }

    /// `set_flags(addr, len, flags)`: the same range rules, then each page gets exactly the
    /// permissions asked for. W+X cannot be decoded and `Pte::leaf` refuses it again. W^X holds
    /// per frame too (R11): `EXECUTE` only on RAM the caller owns, never on device registers or a
    /// `dma_alloc` frame, which the device or another mapping of the same registers can write.
    pub fn set_flags(
        &mut self,
        pid: Pid,
        addr: usize,
        len: usize,
        flags: redoubt_sys::MemFlags,
    ) -> Result<(), redoubt_sys::Error> {
        let bad = redoubt_sys::Error::InvalidArgument;
        let end = Self::user_range(addr, len)?;
        if flags == MemFlags::NONE {
            return Err(bad);
        }
        for page in (addr..end).step_by(PAGE_SIZE) {
            let phys = self.owned_mapping(pid, page)?;
            let device_memory = !self.is_main_memory(phys as *mut u8) || self.is_dma_frame(phys);
            if flags.contains(MemFlags::EXECUTE) && device_memory {
                return Err(bad);
            }
        }
        for page in (addr..end).step_by(PAGE_SIZE) {
            crate::arch::mem::set_user_page_flags(page, flags).map_err(|_| bad)?;
        }
        sync_if_executable(flags);
        Ok(())
    }

    /// `map_fixed(addr, len, flags)`: as `map_anon`, but at exactly `addr` (R11;
    /// kernel/memory.md, `map_fixed`) -- zeroed pages, charged to the caller's budget, that never
    /// replace a mapping. Checked in the order kernel/abi.md's row gives: the range
    /// (`user_range`), then the flags (`check_map_flags`, shared with `process_map`, so W+X and
    /// W-without-R are refused here and can never reach `map_page_inner`'s `.expect` below), then
    /// the pages alone against the budget, by arithmetic, so a length no budget can pay for never
    /// buys a walk (R22), then that range's overlap with any of the caller's mappings
    /// (`range_available_in`, over the whole range before anything is charged or allocated, so
    /// nothing here needs a rollback), then the pages and the page tables they need. Only once
    /// all of that holds does the guaranteed-success mapping loop run, so a failure never leaves
    /// anything mapped or charged.
    pub fn map_fixed(
        &mut self,
        pid: Pid,
        addr: usize,
        len: usize,
        flags: redoubt_sys::MemFlags,
    ) -> Result<(), redoubt_sys::Error> {
        let oom = redoubt_sys::Error::OutOfMemory;
        Self::user_range(addr, len)?;
        check_map_flags(flags)?;
        let npages = (len / PAGE_SIZE) as u64;
        // `pid` is the running caller, and only the kernel (which makes no syscalls) has no
        // budget. Not an error: map_fixed's error set is InvalidArgument and OutOfMemory only.
        let budget = self.budget_of(pid).expect("map_fixed: the running process has a budget");
        // The pages alone first, cheaply, so a huge `len` that the budget could never pay for is
        // refused before the overlap check or `tables_needed` walks it.
        if npages > self.free_pages(budget) {
            return Err(oom);
        }
        let space = MemoryMapping::current();
        if !crate::arch::mem::range_available_in(&space, addr, len) {
            return Err(redoubt_sys::Error::InvalidArgument);
        }
        let tables = crate::arch::mem::tables_needed(&space, addr, len / PAGE_SIZE) as u64;
        if npages + tables > self.free_pages(budget) {
            return Err(oom);
        }
        // From here nothing fails: the range was free, the flags are good, and the check above
        // found the budget able to pay for exactly this many pages and page tables, which
        // `alloc_page` charges as it takes them.
        let free_before = self.free_pages(budget);
        for offset in (0..len).step_by(PAGE_SIZE) {
            crate::arch::mem::prepare_map(self, &space, pid, addr + offset)
                .expect("map_fixed: range_available_in found this page empty, so prepare_map's own (weaker) occupancy check cannot fail, and the charge check above paid for its page table");
        }
        // The count the charge check trusted is the count `prepare_map` made (a checked build).
        debug_assert_eq!(
            free_before - self.free_pages(budget),
            tables,
            "map_fixed: tables_needed miscounted"
        );
        for offset in (0..len).step_by(PAGE_SIZE) {
            // Zeroed through the physmap, before the mapping exists at all (R11). The charge
            // check above guarantees the budget can pay for `npages` pages. That a free frame
            // exists for each is an assumption: a budget's free_pages is backed by free
            // physical frames. It holds because `boot_budgets` gives `root` only the RAM frames
            // the kernel did not keep, every child carves its limit out of its parent's, and
            // nothing is held back. `process_map` relies on the same thing only
            // for page tables (its `prepare_map` `.expect`, which allocates through `walk`);
            // failing on data frames with `.expect` is new here. `map_run` instead treats a
            // failed `alloc_page` as live and unwinds (`undo_run`).
            let frame = self.alloc_page(pid).expect("map_fixed: charged for above");
            crate::kframe::zero(frame);
            crate::arch::mem::map_page_inner(self, pid, frame, addr + offset, flags, true)
                .expect("map_fixed: prepare_map already made this slot ready");
        }
        sync_if_executable(flags);
        Ok(())
    }

    /// The frame behind `page`, which must be a live user mapping of the caller that is not
    /// lent out and, if it is RAM, is credited to the caller (a lend the caller is holding is
    /// its lender's, not its own) or is a `dma_alloc` frame of a run the caller holds.
    pub(crate) fn owned_mapping(&self, pid: Pid, page: usize) -> Result<usize, redoubt_sys::Error> {
        let bad = redoubt_sys::Error::InvalidArgument;
        let phys = crate::arch::mem::user_mapping(page).ok_or(bad)?;
        let ram = self.is_main_memory(phys as *mut u8);
        let own = !ram
            || self.allocations[(phys - self.ram_start) / PAGE_SIZE] == Some(pid)
            || (self.is_dma_frame(phys) && self.dma_holder(phys) == Some(pid));
        if !own {
            return Err(bad);
        }
        Ok(phys)
    }
}

/// Pages just mapped or remapped with `flags` may be fetched from: if they are executable, make
/// this hart's instruction fetches see what was stored in them (`fence.i`; the pages were zeroed,
/// or written by their owner before becoming executable, since W^X forbids both at once).
pub(crate) fn sync_if_executable(flags: MemFlags) {
    if flags.contains(MemFlags::EXECUTE) {
        crate::arch::mem::sync_icache();
    }
}

/// R11 for a caller that maps with `.expect` afterwards (`map_fixed`, `process_map`): refuse
/// empty flags, W+X, and writable without readable before anything is charged or moved, so the
/// page-table layer's own refusal (`check_permissions`) is never what catches them. Decoding
/// already refuses W+X; this check does not rest on that (kernel/abi.md).
pub(crate) fn check_map_flags(flags: MemFlags) -> Result<(), redoubt_sys::Error> {
    let write_only = flags.contains(MemFlags::WRITE) && !flags.contains(MemFlags::READ);
    if flags == MemFlags::NONE || flags.contains(MemFlags::WRITE | MemFlags::EXECUTE) || write_only {
        return Err(redoubt_sys::Error::InvalidArgument);
    }
    Ok(())
}
