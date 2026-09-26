// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Physmap memory management for RISC-V, both Sv39 (rv64) and Sv32 (rv32).
//!
//! Page tables are never mapped into a window. All of physical RAM is mapped
//! supervisor-only at `PHYSMAP_BASE`, and tables are walked in software starting from a
//! root. See `docs/MEMORY-LAYOUT.md`. Everything width-specific (the level
//! count, entries per table, VPN width and `satp` layout) lives in the `paging` crate,
//! reached here through `physmap`; this file is written in terms of `LEVELS`, `vpn()` and
//! `leaf_size()` and so is identical for both modes.
//!
//! All page-table memory is accessed through that typed layer; this file contains policy,
//! not pointer arithmetic. Functions that take a bare virtual address operate on the
//! currently active address space.

use ::riscv::register::satp;
use redoubt_layout::{KERNEL_AREA, PROCESS_AREA, Pid, physmap_virt};
use redoubt_sys::{MemFlags, PAGE_SIZE, USER_AREA_END};

use super::mmu_flags::translate_flags;
use super::physmap::{self, Pte, PteFlags, Slot, Table, window};
use crate::arch::process::InitialProcess;
use crate::mem::{MemoryManager, PageError};

extern "C" {
    #[link_name = "flush_mmu"]
    fn sfence_vma();
}

/// Flush every cached translation on this hart.
fn flush_tlb() {
    // SAFETY: `flush_mmu` (asm.rs) is a bare `sfence.vma; ret`. Dropping cached
    // translations is always sound; at worst it costs page-table walks.
    unsafe { sfence_vma() };
}

/// Make this hart's instruction fetches see every store it made before (RISC-V `fence.i`,
/// Zifencei). Called after anything that makes memory executable for userspace: an image moved in
/// by `process_map`, and `map_anon`, `map_fixed`, `set_flags` or a demand-paged fault installing
/// X, and once at boot before the first user dispatch. Single hart (WP-K5): another hart would
/// need its own fence (post-M1 SMP).
pub fn sync_icache() {
    // SAFETY: `fence.i` takes no operands, touches no memory the compiler tracks and changes no
    // register; it only orders this hart's later instruction fetches after its earlier stores.
    unsafe { core::arch::asm!("fence.i", options(nostack, preserves_flags)) };
}

/// First root entry belonging to the kernel half of the address space.
const ROOT_KERNEL_START: usize = physmap::ENTRIES / 2;
/// Root entry holding per-process kernel data. Everything else in the kernel half is shared.
const ROOT_PROCESS_AREA: usize = physmap::vpn(PROCESS_AREA, physmap::LEVELS - 1);

/// Extract the PID (stored as the ASID) from a raw `satp` value.
pub fn pid_from_satp(satp: usize) -> usize { physmap::satp_pid(satp) }

fn make_satp(pid: Pid, root_phys: usize) -> usize { physmap::make_satp(pid.get() as usize, root_phys) }

/// The root table of the address space that `satp` names.
fn root_of(satp: usize) -> Table {
    assert!(physmap::satp_is_active(satp), "address space is not allocated");
    // SAFETY: a `satp` value with the mode bits set comes from the loader or from
    // `MemoryMapping::allocate()`, both of which store the address of a root page table.
    unsafe { Table::at(window(), physmap::satp_root(satp)) }
}

fn current_root() -> Table { root_of(satp::read().bits()) }

/// Find the leaf (4 KiB) entry for `virt` under `root`.
///
/// If `alloc` is given, missing intermediate tables are allocated on behalf of that PID.
/// Otherwise a missing table is reported as `Unmapped`.
fn walk(
    root: Table,
    virt: usize,
    mut alloc: Option<(&mut MemoryManager, Pid)>,
) -> Result<Slot, PageError> {
    if !physmap::is_canonical(virt) {
        return Err(PageError::NonCanonical);
    }
    let mut table = root;
    for level in (1..physmap::LEVELS).rev() {
        let index = physmap::vpn(virt, level);
        table = match table.child(index) {
            Some(child) => child,
            // A superpage (the physmap). These are never edited at 4 KiB granularity.
            None if table.get(index).is_leaf() => return Err(PageError::Unmapped),
            None => {
                let Some((mm, pid)) = alloc.as_mut() else {
                    return Err(PageError::Unmapped);
                };
                let frame = mm.alloc_page(*pid)?;
                // SAFETY: `alloc_page` returns a RAM frame that was free until now.
                unsafe { table.slot(index).install_table(frame) }
            }
        };
    }
    Ok(table.slot(physmap::vpn(virt, 0)))
}

fn map_page_in(
    root: Table,
    mm: &mut MemoryManager,
    pid: Pid,
    phys: usize,
    virt: usize,
    flags: PteFlags,
) -> Result<(), PageError> {
    assert!(virt & (PAGE_SIZE - 1) == 0);
    assert!(phys & (PAGE_SIZE - 1) == 0);
    check_permissions(flags)?;
    let slot = walk(root, virt, Some((mm, pid)))?;
    if is_occupied(slot.get()) {
        klog!("Page {:08x} already allocated!", virt);
        return Err(PageError::InUse);
    }
    slot.set(Pte::leaf(phys, flags));
    Ok(())
}

/// A page that is mapped, or lent out (the `S` bit, with `VALID` cleared). A lent page's entry
/// is the lender's only record of the loan: the borrower's return restores it. So nothing but
/// that return may overwrite it: not a new mapping, a reservation or an unmap.
fn is_occupied(pte: Pte) -> bool { pte.is_valid() || pte.has(PteFlags::S) }

/// How many page-table pages `space` still lacks to map the `pages` pages from `virt`, none of
/// which is mapped yet. R4 counts them among what a receiver must be able to pay for before a
/// message is delivered, so they are counted here and allocated only once the message is
/// certain: nothing is charged for a delivery that is refused.
///
/// The range is contiguous and ascending, so one table serves consecutive pages, and the table
/// below a `level` entry is named by `addr / leaf_size(level)`, which only grows along the range;
/// counting each such name once as it changes therefore counts each missing table exactly once.
/// (Not the entry's index within its own table: below the root that index recurs in the next
/// table up, so a missing table at index 5 in one gigabyte and another at index 5 in the next,
/// with present tables between them, would be counted once.)
pub fn tables_needed(space: &MemoryMapping, virt: usize, pages: usize) -> usize {
    let mut needed = 0;
    let mut counted = [usize::MAX; physmap::LEVELS];
    for i in 0..pages {
        let addr = virt + i * PAGE_SIZE;
        let mut table = Some(root_of(space.satp));
        for level in (1..physmap::LEVELS).rev() {
            table = table.and_then(|t| t.child(physmap::vpn(addr, level)));
            let name = addr / physmap::leaf_size(level);
            if table.is_none() && counted[level] != name {
                counted[level] = name;
                needed += 1;
            }
        }
    }
    needed
}

/// Get `virt` in `space` ready for a mapping on behalf of `pid`: allocate the page tables it
/// needs and check that nothing occupies it. A `map_page_in` there with valid flags then cannot
/// fail, so a transfer of many pages can prepare them all before it changes anything.
pub fn prepare_map(
    mm: &mut MemoryManager,
    space: &MemoryMapping,
    pid: Pid,
    virt: usize,
) -> Result<(), PageError> {
    let slot = walk(root_of(space.satp), virt, Some((mm, pid)))?;
    if is_occupied(slot.get()) {
        return Err(PageError::InUse);
    }
    Ok(())
}

/// Refuse mappings that the page-table layer would reject outright. These flags come
/// from syscall arguments, so a bad combination is the caller's error, not a kernel bug.
fn check_permissions(flags: PteFlags) -> Result<(), PageError> {
    let permissions = flags & (PteFlags::R | PteFlags::W | PteFlags::X);
    // W^X: no page is ever writable and executable at once. Nor writable without readable
    // (R11): the privileged architecture reserves that encoding.
    if permissions.is_empty()
        || permissions.contains(PteFlags::W | PteFlags::X)
        || (permissions.contains(PteFlags::W) && !permissions.contains(PteFlags::R))
    {
        return Err(PageError::BadFlags);
    }
    Ok(())
}

/// Visit every occupied leaf under `table`, a table at `level` whose first entry maps
/// virtual address `base`. "Occupied" means a valid leaf or a lent page (the `S` bit set
/// with `VALID` cleared); reservations are skipped, matching the original walk. Recurses
/// through valid intermediate tables, so it works for any `LEVELS` (Sv32 and Sv39).
fn for_each_leaf(table: Table, level: usize, base: usize, f: &mut impl FnMut(usize, Pte)) {
    for index in 0..physmap::ENTRIES {
        let pte = table.get(index);
        let virt = base + index * physmap::leaf_size(level);
        if level == 0 {
            if pte.is_valid() || pte.has(PteFlags::S) {
                f(virt, pte);
            }
        } else if let Some(child) = table.child(index) {
            for_each_leaf(child, level - 1, virt, f);
        }
    }
}

/// The entry that translates `virt` under `root`, at whatever level it is found.
fn lookup(root: Table, virt: usize) -> Option<Pte> {
    let mut table = root;
    for level in (0..physmap::LEVELS).rev() {
        let pte = table.get(physmap::vpn(virt, level));
        if pte.is_leaf() {
            return Some(pte);
        }
        table = table.child(physmap::vpn(virt, level))?;
    }
    None
}

/// Check W^X over the kernel's own mappings: no kernel page is writable and executable,
/// and no executable kernel frame has a writable alias in the physmap. Returns the
/// number of executable pages checked. The loader is supposed to guarantee this; the
/// kernel refuses to run if it did not.
pub fn verify_kernel_wx() -> usize {
    let root = current_root();
    let root_index = physmap::vpn(KERNEL_AREA, physmap::LEVELS - 1);
    let Some(sub) = root.child(root_index) else { panic!("kernel area is not mapped") };
    let base = root_index * physmap::leaf_size(physmap::LEVELS - 1);
    let mut executable = 0;
    for_each_leaf(sub, physmap::LEVELS - 2, base, &mut |_virt, pte| {
        if !pte.has(PteFlags::X) {
            return;
        }
        executable += 1;
        assert!(!pte.has(PteFlags::W), "kernel page {:#x} is writable and executable", pte.phys());
        let alias = lookup(root, physmap_virt(pte.phys())).expect("kernel frame is missing from the physmap");
        assert!(
            !alias.has(PteFlags::W),
            "kernel code frame {:#x} is writable through the physmap",
            pte.phys()
        );
        assert!(!alias.has(PteFlags::X), "the physmap must never be executable");
    });
    executable
}

fn user_flag(pid: Pid) -> PteFlags { if pid.get() != 1 { PteFlags::USER } else { PteFlags::NONE } }

#[derive(Copy, Clone, Default, PartialEq)]
pub struct MemoryMapping {
    satp: usize,
}

impl core::fmt::Debug for MemoryMapping {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "(satp: {:#x}, ASID: {}, root: {:#x})",
            self.satp,
            physmap::satp_pid(self.satp),
            physmap::satp_root(self.satp),
        )
    }
}

/// Controls MMU configurations.
impl MemoryMapping {

    /// # Safety
    /// `init` must be a process description produced by the loader.
    pub unsafe fn from_init_process(&mut self, init: InitialProcess) { self.satp = init.satp; }

    /// Allocate a brand-new memory mapping. The new address space contains:
    ///
    ///     1. Every shared kernel root entry (physmap and kernel), copied from the current root.
    ///     2. `ProcessImpl` pages at `PROCESS_AREA`, so the process can be run.
    ///
    /// All pages, including the page tables themselves, are owned by `pid`, so they are
    /// released along with everything else when the process is destroyed.
    pub fn allocate(&mut self, mm: &mut MemoryManager, pid: Pid) -> Result<(), PageError> {
        if self.satp != 0 {
            return Err(PageError::InUse);
        }

        let root_phys = mm.alloc_page(pid)?;
        // SAFETY: `alloc_page` returns a RAM frame that was free until now.
        let root = unsafe { Table::new_in(window(), root_phys) };

        let current = current_root();
        for index in (ROOT_KERNEL_START..physmap::ENTRIES).filter(|index| *index != ROOT_PROCESS_AREA) {
            root.slot(index).copy_from(current.slot(index));
        }

        for page in 0..crate::arch::process::PROCESS_IMPL_PAGES {
            // Saved contexts are frames charged to the running budget (answer 127).
            let context_phys = mm.alloc_context_page(pid)?;
            // SAFETY: a freshly allocated frame, as above.
            unsafe { window().zero_frame(context_phys) };
            let virt = PROCESS_AREA + page * PAGE_SIZE;
            map_page_in(root, mm, pid, context_phys, virt, PteFlags::R | PteFlags::W)?;
        }

        self.satp = make_satp(pid, root_phys);
        Ok(())
    }

    /// Get the currently active memory mapping.
    pub fn current() -> MemoryMapping { MemoryMapping { satp: satp::read().bits() } }

    /// Get the "PID" (actually, ASID) from the current mapping
    pub fn get_pid(&self) -> Option<Pid> { Pid::new(pid_from_satp(self.satp) as _) }

    pub fn is_kernel(&self) -> bool { self.get_pid().map(|v| v.get() == 1).unwrap_or(false) }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    pub fn activate(self) {
        let _ = root_of(self.satp); // refuses an unallocated mapping
        // SAFETY: every address space shares the kernel's root entries (see `allocate`), so
        // the code, stack and data in use right now stay mapped across the switch.
        unsafe { satp::write(satp::Satp::from_bits(self.satp)) };
        flush_tlb();
    }

    /// Call `f(virt, pte)` for every valid or shared 4 KiB leaf in the user half.
    fn for_each_user_leaf(&self, mut f: impl FnMut(usize, Pte)) {
        let root = root_of(self.satp);
        for index in 0..ROOT_KERNEL_START {
            let Some(child) = root.child(index) else { continue };
            let base = index * physmap::leaf_size(physmap::LEVELS - 1);
            for_each_leaf(child, physmap::LEVELS - 2, base, &mut f);
        }
    }

    /// Call `f` with the physical frame of every page this address space has lent out.
    /// An outgoing loan is `S` without `VALID`; `VALID | S` is its protected borrower
    /// alias and must not make teardown reparent somebody else's frame.
    pub fn for_each_lent_frame(&self, mut f: impl FnMut(usize)) {
        self.for_each_user_leaf(|_virt, pte| {
            if pte.has(PteFlags::S) && !pte.is_valid() {
                f(pte.phys());
            }
        });
    }

    pub fn print_map(&self) {
        println!("Memory Maps for PID {}:", pid_from_satp(self.satp));
        self.for_each_user_leaf(|virt, pte| {
            println!("    {:016x} -> {:010x} ({:?})", virt, pte.phys(), pte.flags());
        });
        println!("End of map");
    }

    /// Reserve `addr` for demand paging: the leaf entry gets its permission bits but not `VALID`,
    /// and `ensure_page_exists_inner()` backs it with a real page on first touch.
    pub fn reserve_address(
        &mut self,
        mm: &mut MemoryManager,
        addr: usize,
        flags: MemFlags,
    ) -> Result<(), PageError> {
        let slot = walk(current_root(), addr, Some((mm, crate::arch::current_pid())))?;
        if is_occupied(slot.get()) {
            // can't double-reserve pages
            return Err(PageError::InUse);
        }
        let flags = translate_flags(flags);
        check_permissions(flags)?;
        slot.set(Pte::reservation(flags));
        Ok(())
    }

    pub fn unreserve_address(&self, addr: usize) -> Result<(), PageError> {
        let Ok(slot) = walk(current_root(), addr, None) else {
            // No leaf table, so nothing was ever reserved here.
            return Ok(());
        };
        // Refuse to touch a live or lent mapping. Only undo reservations.
        if is_occupied(slot.get()) {
            return Err(PageError::InUse);
        }
        slot.set(Pte::EMPTY);
        Ok(())
    }
}

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping { satp: 0 };

/// Map the given page into the current address space.  If necessary,
/// allocate new page tables on behalf of `pid`.
///
/// # Errors
///
/// * NoFrame - Tried to allocate a new pagetable, but ran out of memory.
pub fn map_page_inner(
    mm: &mut MemoryManager,
    pid: Pid,
    phys: usize,
    virt: usize,
    req_flags: MemFlags,
    map_user: bool,
) -> Result<(), PageError> {
    let flags = translate_flags(req_flags) | if map_user { PteFlags::USER } else { PteFlags::NONE };
    map_page_in(current_root(), mm, pid, phys, virt, flags)?;
    flush_tlb();
    Ok(())
}

/// Map device registers `phys` at kernel address `virt`, read-write and for the kernel alone (no
/// U bit): WP-K5b's DMA register window. The loader created and shared the tables above it
/// (`reserve_tables`), so nothing is allocated and every address space sees the page; a missing
/// table is a boot bug, and the kernel stops.
pub fn map_kernel_page(phys: usize, virt: usize) {
    let slot = walk(current_root(), virt, None).expect("the loader shares the window's page tables");
    assert!(!is_occupied(slot.get()), "kernel window page {:x} is already mapped", virt);
    slot.set(Pte::leaf(phys, translate_flags(MemFlags::READ | MemFlags::WRITE)));
    flush_tlb();
}

/// Ummap the given page from the current address space.  Never allocate a new
/// page.
///
/// # Returns
///
/// The physical address for the page that was just unmapped
///
/// # Errors
///
/// * Unmapped - No table reaches the address.
/// * Lent - The page is either alias of a loan.
pub fn unmap_page_inner(_mm: &mut MemoryManager, virt: usize) -> Result<usize, PageError> {
    if virt & 3 != 0 {
        return Err(PageError::Unaligned);
    }
    let slot = walk(current_root(), virt, None)?;
    if slot.get().has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    let phys = slot.get().phys();
    slot.set(Pte::EMPTY);
    flush_tlb();
    Ok(phys)
}

/// Return a page from `src_space` back to `dest_space`.
pub fn return_page_inner(
    _mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    _dest_pid: Pid,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<usize, PageError> {
    let src = walk(root_of(src_space.satp), src_addr as usize, None)?;
    let phys = src.get().phys();
    // Check both protected aliases and their frame identity before changing either. The
    // borrower's entry is `VALID | S`; the lender's is `S` without `VALID`.
    if !src.get().is_valid() || !src.get().has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    let dest = walk(root_of(dest_space.satp), dest_addr as usize, None)
        .or(Err(PageError::Lent))?;
    if dest.get().is_valid() || !dest.get().has(PteFlags::S) || dest.get().phys() != phys {
        return Err(PageError::Lent);
    }
    src.set(Pte::EMPTY);
    dest.set(dest.get().without(PteFlags::S | PteFlags::P).with(PteFlags::VALID));
    flush_tlb();
    Ok(phys)
}

/// Take `virt` out of `space`, remembering the loan: clear `VALID` so the lender cannot touch
/// the page, and set `S` so that its entry is the record of the loan (I9). Returns the frame.
/// It maps nothing into the receiver: a Redoubt message is taken out of its sender when it is
/// sent and mapped into its receiver only when someone takes it, which may be much later or
/// never (`message.rs`).
pub fn lend_out(space: &MemoryMapping, virt: usize) -> Result<usize, PageError> {
    let slot = walk(root_of(space.satp), virt, None)?;
    let pte = slot.get();
    if !pte.is_valid() || pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    slot.set(pte.without(PteFlags::VALID).with(PteFlags::S));
    flush_tlb();
    Ok(pte.phys())
}

/// The frame behind a page `space` lent out, from the lender's own entry.
pub fn lent_frame(space: &MemoryMapping, virt: usize) -> Option<usize> {
    let pte = walk(root_of(space.satp), virt, None).ok()?.get();
    (pte.has(PteFlags::S) && !pte.is_valid()).then(|| pte.phys())
}

/// Give a lent page back to its lender: `VALID` again, `S` cleared (`reply`, or a message that
/// never went through).
pub fn lend_back(space: &MemoryMapping, virt: usize) -> Result<(), PageError> {
    let slot = walk(root_of(space.satp), virt, None)?;
    let pte = slot.get();
    if pte.is_valid() || !pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    slot.set(pte.without(PteFlags::S).with(PteFlags::VALID));
    flush_tlb();
    Ok(())
}

/// The lender never gets this page back: a transfer (R4), or a lend whose call was abandoned
/// (R3). Its entry goes; the frame is the receiver's. Returns the frame.
pub fn drop_lent(space: &MemoryMapping, virt: usize) -> Result<usize, PageError> {
    let slot = walk(root_of(space.satp), virt, None)?;
    let pte = slot.get();
    if pte.is_valid() || !pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    slot.set(Pte::EMPTY);
    flush_tlb();
    Ok(pte.phys())
}

/// Map `phys` at `virt` in `space`, readable and writable, for `pid`. A call's borrowed
/// buffer also gets the protected `S` marker; a send's transferred buffer does not.
pub fn map_into(
    mm: &mut MemoryManager,
    pid: Pid,
    space: &MemoryMapping,
    phys: usize,
    virt: usize,
    borrowed: bool,
) -> Result<(), PageError> {
    let mut flags = translate_flags(MemFlags::READ | MemFlags::WRITE) | user_flag(pid);
    if borrowed {
        // `VALID | S` remains normally readable/writable by the borrower, while every
        // ownership-changing mapping API recognizes it as a protected loan alias.
        flags |= PteFlags::S;
    }
    map_page_in(root_of(space.satp), mm, pid, phys, virt, flags)?;
    flush_tlb();
    Ok(())
}

/// Remove a protected borrower alias without changing who owns the frame: an abandoned
/// Map `phys` at `virt` in `space` with exactly `flags`, for `pid`: what `process_map` gives a
/// child, where the parent chooses the permissions and W^X is checked before we get here (R11).
pub fn map_into_with(
    mm: &mut MemoryManager,
    pid: Pid,
    space: &MemoryMapping,
    phys: usize,
    virt: usize,
    flags: MemFlags,
) -> Result<(), PageError> {
    let flags = translate_flags(flags) | user_flag(pid);
    map_page_in(root_of(space.satp), mm, pid, phys, virt, flags)?;
    flush_tlb();
    Ok(())
}

/// Whether `virt` is free in `space`: `address_available`, for an address space that is not the
/// running one (`process_map` looks into a child that has never run).
pub fn address_available_in(space: &MemoryMapping, virt: usize) -> bool {
    debug_assert!(virt < redoubt_sys::USER_AREA_END, "process_map checks its range first");
    match walk(root_of(space.satp), virt, None) {
        // No leaf table yet, so nothing is mapped there. Inside user space the only other way
        // `walk` fails is a non-canonical address, which the caller has already ruled out.
        Err(_) => true,
        // Not just "not occupied": a reservation is not a mapping but it is a claim on the
        // address, and `process_map` never overwrites one.
        Ok(slot) => slot.get().is_empty(),
    }
}

/// Whether every page in `[addr, addr + len)` is free in `space` (`map_fixed`'s overlap check):
/// like `address_available_in` for each page (any nonzero PTE, including a reservation, is
/// occupied), but a missing subtree is skipped as a whole instead of walking it one page at a
/// time, so a huge range that is mostly unmapped costs at most the root entries it spans plus
/// `ENTRIES` (512 on Sv39, 1024 on Sv32) per table actually present in the range -- never one
/// walk per page. `addr` and `addr + len` are assumed already checked (page-aligned, in user
/// space): this only interprets what it finds in the page tables.
pub fn range_available_in(space: &MemoryMapping, addr: usize, len: usize) -> bool {
    !subtree_occupied(root_of(space.satp), physmap::LEVELS - 1, addr, addr + len)
}

/// True if any page in `[start, end)` is occupied under `table`, a table at `level`. `start` and
/// `end` need not be aligned to `level`'s span; each entry's coverage is computed from its own
/// index, not from `start`.
fn subtree_occupied(table: Table, level: usize, start: usize, end: usize) -> bool {
    let span = physmap::leaf_size(level);
    let mut virt = start;
    while virt < end {
        // The next boundary strictly after `virt` at this level's granularity, clamped to `end`.
        let boundary = (virt | (span - 1)).wrapping_add(1);
        let next = boundary.min(end);
        let index = physmap::vpn(virt, level);
        let pte = table.get(index);
        if level == 0 {
            if !pte.is_empty() {
                return true;
            }
        } else if !pte.is_empty() {
            match table.child(index) {
                // A live subtree: recurse into it over the clipped range.
                Some(child) => {
                    if subtree_occupied(child, level - 1, virt, next) {
                        return true;
                    }
                }
                // Nonempty but not a table: a leaf this high (user space has none on either
                // width) or a malformed entry. Fail closed rather than skip it.
                None => return true,
            }
        }
        // Empty entry: the whole `[virt, next)` gap is free (matches `walk`'s "missing table
        // means nothing is mapped" reading, used by `address_available_in`), so skip it.
        virt = next;
    }
    false
}

/// Unmap only the protected borrower alias when a lend leaves the server.
pub fn unmap_from(space: &MemoryMapping, virt: usize) -> Result<usize, PageError> {
    let slot = walk(root_of(space.satp), virt, None)?;
    let pte = slot.get();
    if !pte.is_valid() || !pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    slot.set(Pte::EMPTY);
    flush_tlb();
    Ok(pte.phys())
}

fn checked_phys(pte: Pte) -> Result<usize, PageError> {
    // If the page is "Valid" but shared, issue a sharing violation.
    if pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    if !pte.is_valid() {
        // Reserved for demand paging, but not yet backed by a page.
        return Err(if pte.is_empty() { PageError::Unmapped } else { PageError::Reserved });
    }
    Ok(pte.phys())
}

pub fn virt_to_phys(virt: usize) -> Result<usize, PageError> {
    checked_phys(walk(current_root(), virt, None)?.get())
}

/// Back a reserved (demand-paged) address with a real, zeroed page.
///
/// Takes the memory manager rather than borrowing it: the lend and move paths reach here
/// while they already hold it, and a second borrow of the same `KernelCell` panics the
/// kernel (or, with `smp`, deadlocks). The page-fault handler borrows it for this call.
pub fn ensure_page_exists_inner(mm: &mut MemoryManager, address: usize) -> Result<usize, PageError> {
    // Disallow mapping memory outside of user land
    if !MemoryMapping::current().is_kernel() && address >= USER_AREA_END {
        return Err(PageError::Unmapped);
    }
    let virt = address & !(PAGE_SIZE - 1);
    let slot = walk(current_root(), virt, None).or(Err(PageError::Unmapped))?;
    let reservation = slot.get();

    if reservation.is_valid() {
        return Ok(address);
    }
    // The page is either unreserved, or lent out to another process.
    if reservation.is_empty() || reservation.has(PteFlags::S) {
        return Err(PageError::Unmapped);
    }

    // Out of memory is the process's problem, not the kernel's: the syscall fails, or the
    // faulting process is terminated.
    let new_page = mm.alloc_page(crate::arch::process::current_pid())?;
    // Zero through the physmap before the page becomes visible to the process.
    // SAFETY: `alloc_page` returns a RAM frame that was free until now.
    unsafe { window().zero_frame(new_page) };
    slot.set(Pte::leaf(new_page, reservation.flags() | PteFlags::USER));
    flush_tlb();

    Ok(new_page)
}

/// The frame behind user address `virt` of the current address space, if the process may read
/// it (and, with `write`, write it) there: a system call about to copy a record in or a result
/// out. Anything else (unmapped, reserved but never touched, lent out, kernel, no permission)
/// is `InvalidArgument`: decoding never allocates (answer 115), so a process
/// touches its record buffers before a call. This also excludes a live `VALID | S`
/// borrower alias: userspace can access it normally, but cannot use it as syscall-owned RAM.
pub fn user_frame(virt: usize, write: bool) -> Result<usize, redoubt_sys::Error> {
    use redoubt_sys::Error;
    if virt >= USER_AREA_END {
        return Err(Error::InvalidArgument);
    }
    let page = virt & !(PAGE_SIZE - 1);
    let pte = walk(current_root(), page, None).map_err(|_| Error::InvalidArgument)?.get();
    // A writable record must be readable too (R11), so a write-only entry is never one.
    let wanted =
        PteFlags::VALID | PteFlags::USER | PteFlags::R | if write { PteFlags::W } else { PteFlags::NONE };
    if !pte.has(wanted) || pte.has(PteFlags::S) {
        return Err(Error::InvalidArgument);
    }
    Ok(pte.phys())
}

/// `set_flags` (KERNEL-SPEC.md, R11): give a mapped user page exactly the permissions
/// `flags` asks for, keeping everything else about the entry (its frame, `USER`, the
/// accessed and dirty bits). It may add a permission as well as drop one: a program maps a page writable, writes code into it, and then makes it
/// executable and not writable, which is what W^X asks of it. `Pte::leaf` refuses the
/// combination that would break W^X, as decoding already did.
///
/// A page that is not the caller's own unshared live mapping -- unmapped, reserved but never
/// touched, lent out, or a protected borrower alias -- is `BadAddress`, which the caller
/// reports as `InvalidArgument`.
pub fn set_user_page_flags(virt: usize, flags: MemFlags) -> Result<(), PageError> {
    let wanted = translate_flags(flags);
    check_permissions(wanted)?;
    let slot = walk(current_root(), virt, None)?;
    let pte = slot.get();
    if !pte.is_valid() || pte.has(PteFlags::S) || !pte.has(PteFlags::USER) {
        return Err(PageError::Unmapped);
    }
    let keep = pte.flags() - (PteFlags::R | PteFlags::W | PteFlags::X);
    slot.set(Pte::leaf(pte.phys(), keep | wanted));
    flush_tlb();
    Ok(())
}

/// Whether `virt` is a live, user-visible mapping of the current address space that is not
/// either alias of a loan: what `unmap` and `set_flags` need before either changes one
/// (WP-K0's rule: check the whole range first). The frame it maps, for the caller to check
/// who owns it.
pub fn user_mapping(virt: usize) -> Option<usize> {
    let pte = walk(current_root(), virt, None).ok()?.get();
    (pte.is_valid() && pte.has(PteFlags::USER) && !pte.has(PteFlags::S)).then(|| pte.phys())
}

/// Whether `virt` is already a live mapping of the current address space.
///
/// A page fault on one of these is a **permission** fault -- a store to a page that is only
/// readable, or a fetch from one that is not executable (R11) -- and never a demand-paged page
/// that wants backing. The trap handler must tell the two apart: `ensure_page_exists_inner`
/// answers `Ok` for a page that is already valid, so treating a permission fault as a missing
/// page would resume the faulting instruction, fault again, and spin for ever with the process
/// making no progress and the kernel printing nothing.
pub fn is_mapped(virt: usize) -> bool {
    walk(current_root(), virt, None).is_ok_and(|slot| slot.get().is_valid())
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    debug_assert!(virt < redoubt_sys::USER_AREA_END, "find_virtual_address searches user areas only");
    // An empty entry, or no table reaching it; a reservation or either alias of a loan is not
    // free (`map_anon` would then find a lent page taken, not free).
    walk(current_root(), virt, None).map_or(true, |slot| slot.get().is_empty())
}

/// The permissions of the page at `virt`, a page-aligned address: `None` if it has none or is
/// either alias of a loan.
pub fn page_flags(virt: usize) -> Option<MemFlags> {
    let pte = walk(current_root(), virt, None).ok()?.get();
    if pte.has(PteFlags::S) {
        return None;
    }
    let mut flags = MemFlags::NONE;
    let bits = [
        (PteFlags::R, MemFlags::READ),
        (PteFlags::W, MemFlags::WRITE),
        (PteFlags::X, MemFlags::EXECUTE),
    ];
    for (bit, flag) in bits {
        if pte.has(bit) {
            flags = flags | flag;
        }
    }
    (flags != MemFlags::NONE).then_some(flags)
}
