// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Physmap memory management for RISC-V, both Sv39 (rv64) and Sv32 (rv32).
//!
//! Page tables are never mapped into a window. All of physical RAM is mapped
//! supervisor-only at `PHYSMAP_BASE`, and tables are walked in software starting from a
//! root. See `planning/redoubt/MEMORY-LAYOUT.md`. Everything width-specific (the level
//! count, entries per table, VPN width and `satp` layout) lives in the `paging` crate,
//! reached here through `physmap`; this file is written in terms of `LEVELS`, `vpn()` and
//! `leaf_size()` and so is identical for both modes.
//!
//! All page-table memory is accessed through that typed layer; this file contains policy,
//! not pointer arithmetic. Functions that take a bare virtual address operate on the
//! currently active address space.

use ::riscv::register::satp;
use xous_kernel::{MemoryFlags, PID, arch::*};

pub use super::mmu_flags::MMUFlags;
use super::mmu_flags::{translate_flags, untranslate_flags};
use super::physmap::{self, Pte, Slot, Table, window};
use crate::arch::process::InitialProcess;
use crate::mem::MemoryManager;

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

/// First root entry belonging to the kernel half of the address space.
const ROOT_KERNEL_START: usize = physmap::ENTRIES / 2;
/// Root entry holding per-process kernel data. Everything else in the kernel half is shared.
const ROOT_PROCESS_AREA: usize = physmap::vpn(PROCESS_AREA, physmap::LEVELS - 1);

/// Extract the PID (stored as the ASID) from a raw `satp` value.
pub fn pid_from_satp(satp: usize) -> usize { physmap::satp_pid(satp) }

fn make_satp(pid: PID, root_phys: usize) -> usize { physmap::make_satp(pid.get() as usize, root_phys) }

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
/// Otherwise a missing table is reported as `BadAddress`.
fn walk(
    root: Table,
    virt: usize,
    mut alloc: Option<(&mut MemoryManager, PID)>,
) -> Result<Slot, xous_kernel::Error> {
    if !physmap::is_canonical(virt) {
        return Err(xous_kernel::Error::BadAddress);
    }
    let mut table = root;
    for level in (1..physmap::LEVELS).rev() {
        let index = physmap::vpn(virt, level);
        table = match table.child(index) {
            Some(child) => child,
            // A superpage (the physmap). These are never edited at 4 KiB granularity.
            None if table.get(index).is_leaf() => return Err(xous_kernel::Error::BadAddress),
            None => {
                let Some((mm, pid)) = alloc.as_mut() else {
                    return Err(xous_kernel::Error::BadAddress);
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
    pid: PID,
    phys: usize,
    virt: usize,
    flags: MMUFlags,
) -> Result<(), xous_kernel::Error> {
    assert!(virt & (PAGE_SIZE - 1) == 0);
    assert!(phys & (PAGE_SIZE - 1) == 0);
    check_permissions(flags)?;
    let slot = walk(root, virt, Some((mm, pid)))?;
    if slot.get().is_valid() {
        klog!("Page {:08x} already allocated!", virt);
        return Err(xous_kernel::Error::MemoryInUse);
    }
    slot.set(Pte::leaf(phys, flags));
    Ok(())
}

/// Refuse mappings that the page-table layer would reject outright. These flags come
/// from syscall arguments, so a bad combination is the caller's error, not a kernel bug.
fn check_permissions(flags: MMUFlags) -> Result<(), xous_kernel::Error> {
    let permissions = flags & (MMUFlags::R | MMUFlags::W | MMUFlags::X);
    // W^X: no page is ever writable and executable at once.
    if permissions.is_empty() || permissions.contains(MMUFlags::W | MMUFlags::X) {
        return Err(xous_kernel::Error::InvalidArgument);
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
            if pte.is_valid() || pte.has(MMUFlags::S) {
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
        if !pte.has(MMUFlags::X) {
            return;
        }
        executable += 1;
        assert!(!pte.has(MMUFlags::W), "kernel page {:#x} is writable and executable", pte.phys());
        let alias = lookup(root, physmap_virt(pte.phys())).expect("kernel frame is missing from the physmap");
        assert!(!alias.has(MMUFlags::W), "kernel code frame {:#x} is writable through the physmap", pte.phys());
        assert!(!alias.has(MMUFlags::X), "the physmap must never be executable");
    });
    executable
}

fn user_flag(pid: PID) -> MMUFlags { if pid.get() != 1 { MMUFlags::USER } else { MMUFlags::NONE } }

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
    /// `satp` must name a root page table, as built by the loader.
    #[allow(dead_code)]
    pub unsafe fn from_raw(&mut self, satp: usize) { self.satp = satp; }

    /// # Safety
    /// `init` must be a process description produced by the loader.
    pub unsafe fn from_init_process(&mut self, init: InitialProcess) { self.satp = init.satp; }

    /// Allocate a brand-new memory mapping. The new address space contains:
    ///
    ///     1. Every shared kernel root entry (physmap and kernel), copied from the current root.
    ///     2. `ProcessImpl` pages at `THREAD_CONTEXT_AREA`, so the process can be run.
    ///
    /// All pages, including the page tables themselves, are owned by `pid`, so they are
    /// released along with everything else when the process is destroyed.
    pub fn allocate(&mut self, pid: PID) -> Result<(), xous_kernel::Error> {
        if self.satp != 0 {
            return Err(xous_kernel::Error::MemoryInUse);
        }

        crate::mem::MemoryManager::with_mut(|mm| {
            let root_phys = mm.alloc_page(pid)?;
            // SAFETY: `alloc_page` returns a RAM frame that was free until now.
            let root = unsafe { Table::new_in(window(), root_phys) };

            let current = current_root();
            for index in (ROOT_KERNEL_START..physmap::ENTRIES).filter(|index| *index != ROOT_PROCESS_AREA) {
                root.slot(index).copy_from(current.slot(index));
            }

            for page in 0..crate::arch::process::PROCESS_IMPL_PAGES {
                let context_phys = mm.alloc_page(pid)?;
                // SAFETY: a freshly allocated frame, as above.
                unsafe { window().zero_frame(context_phys) };
                let virt = THREAD_CONTEXT_AREA + page * PAGE_SIZE;
                map_page_in(root, mm, pid, context_phys, virt, MMUFlags::R | MMUFlags::W)?;
            }

            self.satp = make_satp(pid, root_phys);
            Ok(())
        })
    }

    /// Get the currently active memory mapping.
    pub fn current() -> MemoryMapping { MemoryMapping { satp: satp::read().bits() } }

    /// Get the "PID" (actually, ASID) from the current mapping
    pub fn get_pid(&self) -> Option<PID> { PID::new(pid_from_satp(self.satp) as _) }

    #[allow(dead_code)]
    pub fn is_allocated(&self) -> bool { self.get_pid().is_some() }

    pub fn is_kernel(&self) -> bool { self.get_pid().map(|v| v.get() == 1).unwrap_or(false) }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    pub fn activate(self) -> Result<(), xous_kernel::Error> {
        let _ = root_of(self.satp); // refuses an unallocated mapping
        // SAFETY: every address space shares the kernel's root entries (see `allocate`), so
        // the code, stack and data in use right now stay mapped across the switch.
        unsafe { satp::write(satp::Satp::from_bits(self.satp)) };
        flush_tlb();
        Ok(())
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

    /// Call `f` with the physical frame of every page this address space has lent out
    /// (a leaf with the shared bit set).
    pub fn for_each_lent_frame(&self, mut f: impl FnMut(usize)) {
        self.for_each_user_leaf(|_virt, pte| {
            if pte.has(MMUFlags::S) {
                f(pte.phys());
            }
        });
    }

    #[allow(dead_code)]
    pub fn phys_to_virt(&self, phys: usize) -> Result<Option<usize>, xous_kernel::Error> {
        if phys & (PAGE_SIZE - 1) != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }
        let mut found = None;
        let mut twice = false;
        self.for_each_user_leaf(|virt, pte| {
            if pte.phys() == phys {
                twice |= found.is_some();
                found = Some(virt);
            }
        });
        if twice {
            println!("Page is mapped twice within process {:08x}!", phys);
            return Err(xous_kernel::Error::MemoryInUse);
        }
        Ok(found)
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
        flags: MemoryFlags,
    ) -> Result<(), xous_kernel::Error> {
        let slot = walk(current_root(), addr, Some((mm, crate::arch::current_pid())))?;
        if slot.get().is_valid() {
            // can't double-reserve pages
            return Err(xous_kernel::Error::ShareViolation);
        }
        let flags = translate_flags(flags);
        check_permissions(flags)?;
        slot.set(Pte::reservation(flags));
        Ok(())
    }

    pub fn unreserve_address(&self, addr: usize) -> Result<(), xous_kernel::Error> {
        let Ok(slot) = walk(current_root(), addr, None) else {
            // No leaf table, so nothing was ever reserved here.
            return Ok(());
        };
        // Refuse to touch a live mapping. Only undo reservations.
        if slot.get().is_valid() {
            return Err(xous_kernel::Error::ShareViolation);
        }
        slot.set(Pte::EMPTY);
        Ok(())
    }
}

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping { satp: 0 };

/// Call `f` with the physical frame of every page the current process has lent out.
pub fn for_each_lent_frame(f: impl FnMut(usize)) { MemoryMapping::current().for_each_lent_frame(f); }

/// When we allocate pages, they are owned by the kernel so we can zero
/// them out.  After that is done, hand the page to the user.
pub fn hand_page_to_user(virt: *mut u8) -> Result<(), xous_kernel::Error> {
    let slot = walk(current_root(), virt as usize, None)?;
    if !slot.get().is_valid() {
        return Err(xous_kernel::Error::BadAddress);
    }
    slot.set(slot.get().with(MMUFlags::USER));
    flush_tlb();
    Ok(())
}

/// Map the given page into the current address space.  If necessary,
/// allocate new page tables on behalf of `pid`.
///
/// # Errors
///
/// * OutOfMemory - Tried to allocate a new pagetable, but ran out of memory.
pub fn map_page_inner(
    mm: &mut MemoryManager,
    pid: PID,
    phys: usize,
    virt: usize,
    req_flags: MemoryFlags,
    map_user: bool,
) -> Result<(), xous_kernel::Error> {
    let flags = translate_flags(req_flags) | if map_user { MMUFlags::USER } else { MMUFlags::NONE };
    map_page_in(current_root(), mm, pid, phys, virt, flags)?;
    flush_tlb();
    Ok(())
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
/// * BadAddress - Address was not already mapped.
pub fn unmap_page_inner(_mm: &mut MemoryManager, virt: usize) -> Result<usize, xous_kernel::Error> {
    if virt & 3 != 0 {
        return Err(xous_kernel::Error::BadAlignment);
    }
    let slot = walk(current_root(), virt, None)?;
    let phys = slot.get().phys();
    slot.set(Pte::EMPTY);
    flush_tlb();
    Ok(phys)
}

/// Move a page from one address space to another.
pub fn move_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<(), xous_kernel::Error> {
    let src = walk(root_of(src_space.satp), src_addr as usize, None)?;
    let previous = src.get();
    if !previous.is_valid() {
        return Err(xous_kernel::Error::BadAddress);
    }
    // Invalidate the old entry
    src.set(Pte::EMPTY);

    let flags = translate_flags(untranslate_flags(previous.flags().bits())) | user_flag(dest_pid);
    let result = map_page_in(root_of(dest_space.satp), mm, dest_pid, previous.phys(), dest_addr as usize, flags);
    flush_tlb();
    result
}

/// Determine if a virtual page has been lent.
pub fn page_is_lent(src_addr: *mut u8) -> bool {
    walk(current_root(), src_addr as usize, None).is_ok_and(|slot| slot.get().has(MMUFlags::S))
}

/// Mark the given virtual address as being lent: clear `VALID`, so that this process
/// cannot touch the page while it is lent, and set `S` to remember that it is lent.
///
/// # Errors
///
/// * **ShareViolation**: Tried to share a page that is not ours, or is already shared
pub fn lend_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
    mutable: bool,
) -> Result<usize, xous_kernel::Error> {
    let src = walk(root_of(src_space.satp), src_addr as usize, None)?;
    let current = src.get();
    let phys = current.phys();

    // Sharing a page that is not ours, or that is already shared, is a violation.
    if !current.is_valid() || current.has(MMUFlags::S) {
        return Err(xous_kernel::Error::ShareViolation);
    }
    src.set(current.without(MMUFlags::VALID).with(MMUFlags::S));

    let mut new_flags = MMUFlags::R | user_flag(dest_pid);
    if mutable && current.has(MMUFlags::W) {
        new_flags |= MMUFlags::W;
    }

    let result = map_page_in(root_of(dest_space.satp), mm, dest_pid, phys, dest_addr as usize, new_flags);
    flush_tlb();
    result.map(|_| phys)
}

/// Return a page from `src_space` back to `dest_space`.
pub fn return_page_inner(
    _mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    _dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
) -> Result<usize, xous_kernel::Error> {
    let src = walk(root_of(src_space.satp), src_addr as usize, None)?;
    let phys = src.get().phys();

    // If the page is not valid in this program, we can't return it.
    if !src.get().is_valid() {
        return Err(xous_kernel::Error::ShareViolation);
    }
    src.set(Pte::EMPTY);

    let dest = walk(root_of(dest_space.satp), dest_addr as usize, None)
        .expect("page wasn't lent in destination space");
    // If the page wasn't marked as `Shared` in the destination address space, bail.
    assert!(dest.get().has(MMUFlags::S), "page wasn't shared in destination space");
    dest.set(dest.get().without(MMUFlags::S | MMUFlags::P).with(MMUFlags::VALID));
    flush_tlb();
    Ok(phys)
}

fn checked_phys(pte: Pte) -> Result<usize, xous_kernel::Error> {
    // If the page is "Valid" but shared, issue a sharing violation.
    if pte.has(MMUFlags::S) {
        return Err(xous_kernel::Error::ShareViolation);
    }
    if !pte.is_valid() {
        // Reserved for demand paging, but not yet backed by a page.
        return Err(if pte.is_empty() { xous_kernel::Error::BadAddress } else { xous_kernel::Error::MemoryInUse });
    }
    Ok(pte.phys())
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous_kernel::Error> {
    checked_phys(walk(current_root(), virt, None)?.get())
}

/// Translate `virt` in the address space of `pid`. No address space switch is needed.
#[allow(dead_code)]
pub fn virt_to_phys_pid(pid: PID, virt: usize) -> Result<usize, xous_kernel::Error> {
    let mapping = crate::services::SystemServices::with(|ss| {
        ss.get_process(pid).map(|p| p.mapping).or(Err(xous_kernel::Error::InvalidPID))
    })?;
    checked_phys(walk(root_of(mapping.satp), virt, None)?.get())
}

/// Back a reserved (demand-paged) address with a real, zeroed page.
pub fn ensure_page_exists_inner(address: usize) -> Result<usize, xous_kernel::Error> {
    // Disallow mapping memory outside of user land
    if !MemoryMapping::current().is_kernel() && address >= USER_AREA_END {
        return Err(xous_kernel::Error::OutOfMemory);
    }
    let virt = address & !(PAGE_SIZE - 1);
    let slot = walk(current_root(), virt, None).or(Err(xous_kernel::Error::BadAddress))?;
    let reservation = slot.get();

    if reservation.is_valid() {
        return Ok(address);
    }
    // The page is either unreserved, or lent out to another process.
    if reservation.is_empty() || reservation.has(MMUFlags::S) {
        return Err(xous_kernel::Error::BadAddress);
    }

    let new_page = MemoryManager::with_mut(|mm| {
        mm.alloc_page(crate::arch::process::current_pid()).expect("Couldn't allocate new page")
    });
    // Zero through the physmap before the page becomes visible to the process.
    // SAFETY: `alloc_page` returns a RAM frame that was free until now.
    unsafe { window().zero_frame(new_page) };
    slot.set(Pte::leaf(new_page, reservation.flags() | MMUFlags::USER));
    flush_tlb();

    Ok(new_page)
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    virt_to_phys(virt).is_err_and(|e| e == xous_kernel::Error::BadAddress)
}

/// Get the `MemoryFlags` for the requested virtual address. The address must
/// be valid and page-aligned, and must not be Shared.
///
/// # Returns
///
/// * **None**: The page is not valid or is shared
/// * **Some(MemoryFlags)**: The translated sharing permissions of the given flags
pub fn page_flags(virt: usize) -> Option<MemoryFlags> {
    let pte = walk(current_root(), virt, None).ok()?.get();
    if pte.has(MMUFlags::S) {
        return None;
    }
    let mut flags = MemoryFlags::empty();
    for (bit, flag) in [(MMUFlags::R, MemoryFlags::R), (MMUFlags::W, MemoryFlags::W), (MMUFlags::X, MemoryFlags::X)] {
        if pte.has(bit) {
            flags = flags | flag;
        }
    }
    (!flags.is_empty()).then_some(flags)
}

/// Remove permissions from a page. Permissions can only be dropped, never added.
pub fn update_page_flags(virt: usize, flags: MemoryFlags) -> Result<(), xous_kernel::Error> {
    // Stripping every permission would turn the entry into a pointer to another table.
    if (flags & (MemoryFlags::R | MemoryFlags::W | MemoryFlags::X)).is_empty() {
        return Err(xous_kernel::Error::MemoryInUse);
    }

    let slot = walk(current_root(), virt, None).or(Err(xous_kernel::Error::OutOfMemory))?;
    let mut pte = slot.get();
    if pte.has(MMUFlags::S) {
        return Err(xous_kernel::Error::ShareViolation);
    }

    for (requested, bit) in
        [(MemoryFlags::X, MMUFlags::X), (MemoryFlags::R, MMUFlags::R), (MemoryFlags::W, MMUFlags::W)]
    {
        if (flags & requested).is_empty() {
            pte = pte.without(bit);
        } else if !pte.has(bit) {
            return Err(xous_kernel::Error::ShareViolation);
        }
    }

    slot.set(pte);
    flush_tlb();
    Ok(())
}
