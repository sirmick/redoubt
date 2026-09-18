// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Sv39 memory management for rv64.
//!
//! Unlike the Sv32 implementation, page tables are never mapped into a window. All of
//! physical RAM is mapped supervisor-only at `PHYSMAP_BASE`, and tables are walked in
//! software starting from the root named by `satp`. See `planning/xous64/MEMORY-LAYOUT.md`.
//!
//! As on rv32, every function here that takes a bare virtual address operates on the
//! *currently active* address space. Callers switch spaces with `MemoryMapping::activate()`.

use ::riscv::register::satp;
use xous_kernel::{MemoryFlags, PID, arch::*};

pub use super::mmu_flags::MMUFlags;
use super::mmu_flags::{translate_flags, untranslate_flags};
use crate::arch::process::InitialProcess;
use crate::mem::MemoryManager;

extern "C" {
    pub fn flush_mmu();
}

const RWX: usize = 0b1110;
const PTE_FLAG_BITS: usize = 0x3ff;
const ENTRIES_PER_TABLE: usize = 512;
const LEVELS: usize = 3;

const SATP_MODE_SV39: usize = 8 << 60;
const SATP_ASID_SHIFT: usize = 44;
const SATP_ASID_MASK: usize = 0xffff;
const SATP_PPN_MASK: usize = (1 << 44) - 1;

/// First root entry belonging to the kernel half of the address space.
const ROOT_KERNEL_START: usize = ENTRIES_PER_TABLE / 2;
/// Root entry holding per-process kernel data. Everything else in the kernel half is shared.
const ROOT_PROCESS_AREA: usize = (PROCESS_AREA >> 30) & (ENTRIES_PER_TABLE - 1);

/// Extract the PID (stored as the ASID) from a raw `satp` value.
pub fn pid_from_satp(satp: usize) -> usize { (satp >> SATP_ASID_SHIFT) & SATP_ASID_MASK }

fn root_from_satp(satp: usize) -> usize { (satp & SATP_PPN_MASK) << 12 }

fn make_satp(pid: PID, root_phys: usize) -> usize {
    SATP_MODE_SV39 | ((pid.get() as usize) << SATP_ASID_SHIFT) | (root_phys >> 12)
}

/// Kernel-virtual pointer to a physical page, through the physmap.
fn phys_to_kvirt(phys: usize) -> *mut usize {
    debug_assert!(phys < PHYSMAP_SIZE);
    (PHYSMAP_BASE + phys) as *mut usize
}

fn pte_to_phys(pte: usize) -> usize { (pte >> 10) << 12 }

fn phys_to_pte(phys: usize, flags: usize) -> usize { ((phys >> 12) << 10) | flags }

fn is_canonical(virt: usize) -> bool {
    let upper = virt >> 38;
    upper == 0 || upper == (1 << 26) - 1
}

fn vpn(virt: usize, level: usize) -> usize { (virt >> (12 + 9 * level)) & (ENTRIES_PER_TABLE - 1) }

unsafe fn zero_phys_page(phys: usize) { core::ptr::write_bytes(phys_to_kvirt(phys) as *mut u8, 0, PAGE_SIZE); }

fn current_root() -> usize { root_from_satp(satp::read().bits()) }

/// Walk the tables under `root_phys` and return a pointer to the leaf (4 KiB) entry for `virt`.
///
/// If `alloc` is given, missing intermediate tables are allocated on behalf of that PID.
/// Otherwise a missing table is reported as `BadAddress`.
fn walk(
    root_phys: usize,
    virt: usize,
    mut alloc: Option<(&mut MemoryManager, PID)>,
) -> Result<*mut usize, xous_kernel::Error> {
    if !is_canonical(virt) {
        return Err(xous_kernel::Error::BadAddress);
    }
    let mut table = phys_to_kvirt(root_phys);
    for level in (1..LEVELS).rev() {
        let entry = unsafe { table.add(vpn(virt, level)) };
        let mut pte = unsafe { entry.read_volatile() };
        if pte & MMUFlags::VALID.bits() == 0 {
            let Some((mm, pid)) = alloc.as_mut() else {
                return Err(xous_kernel::Error::BadAddress);
            };
            let table_phys = mm.alloc_page(*pid)?;
            unsafe { zero_phys_page(table_phys) };
            // A pointer to the next level has V set and R/W/X clear.
            pte = phys_to_pte(table_phys, MMUFlags::VALID.bits());
            unsafe { entry.write_volatile(pte) };
        } else if pte & RWX != 0 {
            // A superpage leaf (the physmap). These are never edited at 4 KiB granularity.
            return Err(xous_kernel::Error::BadAddress);
        }
        table = phys_to_kvirt(pte_to_phys(pte));
    }
    Ok(unsafe { table.add(vpn(virt, 0)) })
}

fn map_page_in(
    root_phys: usize,
    mm: &mut MemoryManager,
    pid: PID,
    phys: usize,
    virt: usize,
    flags: MMUFlags,
) -> Result<(), xous_kernel::Error> {
    assert!(virt & (PAGE_SIZE - 1) == 0);
    assert!(phys & (PAGE_SIZE - 1) == 0);
    let entry = walk(root_phys, virt, Some((mm, pid)))?;
    // Ensure the entry hasn't already been mapped.
    if unsafe { entry.read_volatile() } & MMUFlags::VALID.bits() != 0 {
        klog!("Page {:08x} already allocated!", virt);
        return Err(xous_kernel::Error::MemoryInUse);
    }
    unsafe {
        entry.write_volatile(phys_to_pte(
            phys,
            (flags | MMUFlags::VALID | MMUFlags::D | MMUFlags::A).bits(),
        ))
    };
    Ok(())
}

#[derive(Copy, Clone, Default, PartialEq)]
pub struct MemoryMapping {
    satp: usize,
}

impl core::fmt::Debug for MemoryMapping {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "(satp: 0x{:016x}, mode: {}, ASID: {}, root: {:08x})",
            self.satp,
            self.satp >> 60,
            pid_from_satp(self.satp),
            root_from_satp(self.satp),
        )
    }
}

/// Controls MMU configurations.
impl MemoryMapping {
    #[allow(dead_code)]
    pub unsafe fn from_raw(&mut self, satp: usize) { self.satp = satp; }

    pub unsafe fn from_init_process(&mut self, init: InitialProcess) { self.satp = init.satp; }

    /// Allocate a brand-new memory mapping. The new address space contains:
    ///
    ///     1. Every shared kernel root entry (physmap and kernel), copied from the current root.
    ///     2. `ProcessImpl` pages at `THREAD_CONTEXT_AREA`, so the process can be run.
    ///
    /// All pages, including the page tables themselves, are owned by `pid`, so they are
    /// released along with everything else when the process is destroyed.
    pub unsafe fn allocate(&mut self, pid: PID) -> Result<(), xous_kernel::Error> {
        if self.satp != 0 {
            return Err(xous_kernel::Error::MemoryInUse);
        }

        crate::mem::MemoryManager::with_mut(|mm| {
            let root_phys = mm.alloc_page(pid)?;
            zero_phys_page(root_phys);

            let new_root = phys_to_kvirt(root_phys);
            let current_root = phys_to_kvirt(current_root());
            for idx in ROOT_KERNEL_START..ENTRIES_PER_TABLE {
                if idx != ROOT_PROCESS_AREA {
                    new_root.add(idx).write_volatile(current_root.add(idx).read_volatile());
                }
            }

            for page in 0..crate::arch::process::PROCESS_IMPL_PAGES {
                let context_phys = mm.alloc_page(pid)?;
                zero_phys_page(context_phys);
                map_page_in(
                    root_phys,
                    mm,
                    pid,
                    context_phys,
                    THREAD_CONTEXT_AREA + page * PAGE_SIZE,
                    MMUFlags::R | MMUFlags::W,
                )?;
            }

            self.satp = make_satp(pid, root_phys);
            Ok(())
        })
    }

    /// Get the currently active memory mapping.
    pub fn current() -> MemoryMapping { MemoryMapping { satp: satp::read().bits() } }

    /// Get the "PID" (actually, ASID) from the current mapping
    pub fn get_pid(&self) -> Option<PID> { PID::new(pid_from_satp(self.satp) as _) }

    pub fn is_allocated(&self) -> bool { self.get_pid().is_some() }

    pub fn is_kernel(&self) -> bool { self.get_pid().map(|v| v.get() == 1).unwrap_or(false) }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    pub fn activate(self) -> Result<(), xous_kernel::Error> {
        unsafe {
            satp::write(satp::Satp::from_bits(self.satp));
            flush_mmu();
        }
        Ok(())
    }

    /// Call `f(virt, pte)` for every valid or shared 4 KiB leaf in the user half.
    fn for_each_user_leaf(&self, mut f: impl FnMut(usize, usize)) {
        let root = phys_to_kvirt(root_from_satp(self.satp));
        for i2 in 0..ROOT_KERNEL_START {
            let l2 = unsafe { root.add(i2).read_volatile() };
            if l2 & MMUFlags::VALID.bits() == 0 || l2 & RWX != 0 {
                continue;
            }
            let l1_table = phys_to_kvirt(pte_to_phys(l2));
            for i1 in 0..ENTRIES_PER_TABLE {
                let l1 = unsafe { l1_table.add(i1).read_volatile() };
                if l1 & MMUFlags::VALID.bits() == 0 || l1 & RWX != 0 {
                    continue;
                }
                let l0_table = phys_to_kvirt(pte_to_phys(l1));
                for i0 in 0..ENTRIES_PER_TABLE {
                    let l0 = unsafe { l0_table.add(i0).read_volatile() };
                    if l0 & (MMUFlags::VALID | MMUFlags::S).bits() != 0 {
                        f((i2 << 30) | (i1 << 21) | (i0 << 12), l0);
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn phys_to_virt(&self, phys: usize) -> Result<Option<usize>, xous_kernel::Error> {
        if phys & (PAGE_SIZE - 1) != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }
        let mut found = None;
        let mut twice = false;
        self.for_each_user_leaf(|virt, pte| {
            if pte_to_phys(pte) == phys {
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
            println!(
                "    {:016x} -> {:010x} ({:?})",
                virt,
                pte_to_phys(pte),
                MMUFlags::from_bits_truncate(pte & PTE_FLAG_BITS)
            );
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
        let pid = crate::arch::current_pid();
        let entry = walk(current_root(), addr, Some((mm, pid)))?;
        if unsafe { entry.read_volatile() } & MMUFlags::VALID.bits() != 0 {
            // can't double-reserve pages
            return Err(xous_kernel::Error::ShareViolation);
        }
        unsafe { entry.write_volatile(translate_flags(flags).bits()) };
        Ok(())
    }

    pub fn unreserve_address(&self, addr: usize) -> Result<(), xous_kernel::Error> {
        let Ok(entry) = walk(current_root(), addr, None) else {
            // No leaf table, so nothing was ever reserved here.
            return Ok(());
        };
        // Refuse to touch a live mapping. Only undo reservations.
        if unsafe { entry.read_volatile() } & MMUFlags::VALID.bits() != 0 {
            return Err(xous_kernel::Error::ShareViolation);
        }
        unsafe { entry.write_volatile(0) };
        Ok(())
    }
}

pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping { satp: 0 };

/// When we allocate pages, they are owned by the kernel so we can zero
/// them out.  After that is done, hand the page to the user.
pub fn hand_page_to_user(virt: *mut u8) -> Result<(), xous_kernel::Error> {
    let entry = pagetable_entry(virt as usize)?;
    let pte = unsafe { entry.read_volatile() };
    if pte & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    unsafe { entry.write_volatile(pte | MMUFlags::USER.bits()) };
    unsafe { flush_mmu() };
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
    unsafe { flush_mmu() };
    Ok(())
}

/// Get the pagetable entry for a given address, or `Err()` if the address is invalid
pub fn pagetable_entry(addr: usize) -> Result<*mut usize, xous_kernel::Error> {
    if addr & 3 != 0 {
        return Err(xous_kernel::Error::BadAlignment);
    }
    walk(current_root(), addr, None)
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
    let entry = pagetable_entry(virt)?;
    let phys = pte_to_phys(unsafe { entry.read_volatile() });
    unsafe { entry.write_volatile(0) };
    unsafe { flush_mmu() };
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
    let entry = walk(root_from_satp(src_space.satp), src_addr as usize, None)?;
    let previous_entry = unsafe { entry.read_volatile() };
    if previous_entry & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::BadAddress);
    }
    // Invalidate the old entry
    unsafe { entry.write_volatile(0) };

    let flags = translate_flags(untranslate_flags(previous_entry))
        | if dest_pid.get() != 1 { MMUFlags::USER } else { MMUFlags::NONE };
    let result = map_page_in(
        root_from_satp(dest_space.satp),
        mm,
        dest_pid,
        pte_to_phys(previous_entry),
        dest_addr as usize,
        flags,
    );
    unsafe { flush_mmu() };
    result
}

/// Determine if a virtual page has been lent.
pub fn page_is_lent(src_addr: *mut u8) -> bool {
    pagetable_entry(src_addr as usize)
        .map_or(false, |v| unsafe { v.read_volatile() } & MMUFlags::S.bits() != 0)
}

/// Mark the given virtual address as being lent.  If `writable`, clear the
/// `valid` bit so that this process can't accidentally write to this page while
/// it is lent.
///
/// This uses the `RWS` fields to keep track of the following pieces of information:
///
/// * **PTE[8]**: This is set to `1` indicating the page is lent
/// * **PTE[9]**: This is `1` if the page was previously writable
///
/// # Returns
///
/// # Errors
///
/// * **ShareViolation**: Tried to mutably share a region that was already shared
pub fn lend_page_inner(
    mm: &mut MemoryManager,
    src_space: &MemoryMapping,
    src_addr: *mut u8,
    dest_pid: PID,
    dest_space: &MemoryMapping,
    dest_addr: *mut u8,
    mutable: bool,
) -> Result<usize, xous_kernel::Error> {
    let entry = walk(root_from_satp(src_space.satp), src_addr as usize, None)?;
    let current_entry = unsafe { entry.read_volatile() };
    let phys = pte_to_phys(current_entry);

    // If we try to share a page that's not ours, that's just wrong.
    if current_entry & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // If we try to share a page that's already shared, that's a sharing violation.
    if current_entry & MMUFlags::S.bits() != 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Strip the `VALID` flag, and set the `SHARED` flag.
    let new_entry = (current_entry & !MMUFlags::VALID.bits()) | MMUFlags::S.bits();
    unsafe { entry.write_volatile(new_entry) };

    let mut new_flags = MMUFlags::R;
    if mutable && (new_entry & MMUFlags::W.bits()) != 0 {
        new_flags |= MMUFlags::W;
    }
    if dest_pid.get() != 1 {
        new_flags |= MMUFlags::USER;
    }

    let result =
        map_page_in(root_from_satp(dest_space.satp), mm, dest_pid, phys, dest_addr as usize, new_flags);
    unsafe { flush_mmu() };
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
    let src_entry = walk(root_from_satp(src_space.satp), src_addr as usize, None)?;
    let src_entry_value = unsafe { src_entry.read_volatile() };
    let phys = pte_to_phys(src_entry_value);

    // If the page is not valid in this program, we can't return it.
    if src_entry_value & MMUFlags::VALID.bits() == 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    // Mark the page as `Free`, which unmaps it.
    unsafe { src_entry.write_volatile(0) };

    let dest_entry = walk(root_from_satp(dest_space.satp), dest_addr as usize, None)
        .expect("page wasn't lent in destination space");
    let dest_entry_value = unsafe { dest_entry.read_volatile() };

    // If the page wasn't marked as `Shared` in the destination address space, bail.
    if dest_entry_value & MMUFlags::S.bits() == 0 {
        panic!("page wasn't shared in destination space");
    }

    // Clear the `SHARED` and `PREVIOUSLY-WRITABLE` bits, and set the `VALID` bit.
    unsafe {
        dest_entry
            .write_volatile(dest_entry_value & !(MMUFlags::S | MMUFlags::P).bits() | MMUFlags::VALID.bits())
    };
    unsafe { flush_mmu() };
    Ok(phys)
}

fn pte_to_phys_checked(pte: usize) -> Result<usize, xous_kernel::Error> {
    // If the page is "Valid" but shared, issue a sharing violation.
    if pte & MMUFlags::S.bits() != 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }
    if pte & MMUFlags::VALID.bits() == 0 {
        // Reserved for demand paging, but not yet backed by a page.
        if pte != 0 {
            return Err(xous_kernel::Error::MemoryInUse);
        }
        return Err(xous_kernel::Error::BadAddress);
    }
    Ok(pte_to_phys(pte))
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous_kernel::Error> {
    let entry = walk(current_root(), virt, None)?;
    pte_to_phys_checked(unsafe { entry.read_volatile() })
}

/// Translate `virt` in the address space of `pid`. No address space switch is needed.
pub fn virt_to_phys_pid(pid: PID, virt: usize) -> Result<usize, xous_kernel::Error> {
    let mapping = crate::services::SystemServices::with(|ss| {
        ss.get_process(pid).map(|p| p.mapping).or(Err(xous_kernel::Error::InvalidPID))
    })?;
    let entry = walk(root_from_satp(mapping.satp), virt, None)?;
    pte_to_phys_checked(unsafe { entry.read_volatile() })
}

/// Back a reserved (demand-paged) address with a real, zeroed page.
pub fn ensure_page_exists_inner(address: usize) -> Result<usize, xous_kernel::Error> {
    // Disallow mapping memory outside of user land
    if !MemoryMapping::current().is_kernel() && address >= USER_AREA_END {
        return Err(xous_kernel::Error::OutOfMemory);
    }
    let virt = address & !(PAGE_SIZE - 1);
    let entry = pagetable_entry(virt).or(Err(xous_kernel::Error::BadAddress))?;
    let flags = unsafe { entry.read_volatile() } & PTE_FLAG_BITS;

    if flags & MMUFlags::VALID.bits() != 0 {
        return Ok(address);
    }

    // The page is either unreserved, or lent out to another process.
    if flags == 0 || (flags & MMUFlags::S.bits()) != 0 {
        return Err(xous_kernel::Error::BadAddress);
    }

    let new_page = MemoryManager::with_mut(|mm| {
        mm.alloc_page(crate::arch::process::current_pid()).expect("Couldn't allocate new page")
    });

    // Zero through the physmap before the page becomes visible, then hand it to the user.
    unsafe {
        zero_phys_page(new_page);
        entry.write_volatile(phys_to_pte(
            new_page,
            flags | (MMUFlags::VALID | MMUFlags::USER | MMUFlags::D | MMUFlags::A).bits(),
        ));
        flush_mmu();
    }

    Ok(new_page)
}

/// Determine whether a virtual address has been mapped
pub fn address_available(virt: usize) -> bool {
    if let Err(e) = virt_to_phys(virt) { e == xous_kernel::Error::BadAddress } else { false }
}

/// Get the `MemoryFlags` for the requested virtual address. The address must
/// be valid and page-aligned, and must not be Shared.
///
/// # Returns
///
/// * **None**: The page is not valid or is shared
/// * **Some(MemoryFlags)**: The translated sharing permissions of the given flags
pub fn page_flags(virt: usize) -> Option<MemoryFlags> {
    let entry = walk(current_root(), virt, None).ok()?;
    let mmu_flags = unsafe { entry.read_volatile() };

    if mmu_flags & MMUFlags::S.bits() != 0 {
        return None;
    }

    let mut return_flags = MemoryFlags::empty();
    if mmu_flags & MMUFlags::R.bits() != 0 {
        return_flags = return_flags | MemoryFlags::R;
    }
    if mmu_flags & MMUFlags::W.bits() != 0 {
        return_flags = return_flags | MemoryFlags::W;
    }
    if mmu_flags & MMUFlags::X.bits() != 0 {
        return_flags = return_flags | MemoryFlags::X;
    }

    if return_flags.is_empty() { None } else { Some(return_flags) }
}

/// Remove permissions from a page. Permissions can only be dropped, never added.
pub fn update_page_flags(virt: usize, flags: MemoryFlags) -> Result<(), xous_kernel::Error> {
    // Stripping every permission would turn the entry into a pointer to another table.
    if (flags & (MemoryFlags::R | MemoryFlags::W | MemoryFlags::X)).is_empty() {
        return Err(xous_kernel::Error::MemoryInUse);
    }

    let entry = walk(current_root(), virt, None).or(Err(xous_kernel::Error::OutOfMemory))?;
    let mut mmu_flags = unsafe { entry.read_volatile() };

    if mmu_flags & MMUFlags::S.bits() != 0 {
        return Err(xous_kernel::Error::ShareViolation);
    }

    for (requested, bit) in
        [(MemoryFlags::X, MMUFlags::X), (MemoryFlags::R, MMUFlags::R), (MemoryFlags::W, MMUFlags::W)]
    {
        if (flags & requested).is_empty() {
            mmu_flags &= !bit.bits();
        } else if mmu_flags & bit.bits() == 0 {
            return Err(xous_kernel::Error::ShareViolation);
        }
    }

    unsafe { entry.write_volatile(mmu_flags) };
    unsafe { flush_mmu() };
    Ok(())
}
