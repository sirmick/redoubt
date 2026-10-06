// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Physmap memory management for RISC-V, both Sv39 (rv64) and Sv32 (rv32).
//!
//! Page tables are never mapped into a window. All of physical RAM is mapped
//! supervisor-only at `PHYSMAP_BASE`, and tables are walked in software starting from a
//! root. See `docs/kernel/memory-layout.md`. Everything width-specific (the level
//! count, entries per table, VPN width and `satp` layout) lives in the `paging` crate,
//! reached here through `physmap`; this file is written in terms of `LEVELS`, `vpn()` and
//! `leaf_size()` and so is identical for both modes.
//!
//! All page-table memory is accessed through that typed layer; this file contains policy,
//! not pointer arithmetic. Functions that take a bare virtual address operate on the
//! currently active address space.
//!
//! A process's ASID is its PID, and a switch flushes nothing. So every page-table write flushes
//! what it changed, in the space it changed, whether that space is running or not: a leaf, its
//! address in that space's ASID; a table pointer, the whole ASID; a leaf of the kernel half that
//! every space shares (global, `G`), its address in every ASID (kernel/memory-layout.md, "`satp`").
//! The checked build logs each write and each flush, and stops if a write is unflushed when the
//! kernel returns to user mode (`audit`).

use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_layout::{KERNEL_AREA, PROCESS_AREA, Pid, physmap_virt};
use redoubt_sys::{MemFlags, PAGE_SIZE, USER_AREA_END};
use riscv::register::satp;

use super::mmu_flags::translate_flags;
use super::physmap::tlb::{Flush, Stale};
use super::physmap::{self, Pte, PteFlags, Slot, Table, window};
use crate::arch::process::InitialProcess;
use crate::mem::{MemoryManager, PageError};

extern "C" {
    fn flush_mmu();
    fn flush_asid(asid: usize);
    fn flush_page(virt: usize, asid: usize);
    fn flush_page_global(virt: usize);
}

/// Drop what `flush` covers from this hart's cached translations. Every other hart that may hold
/// the same ASID's translations owes the same flush, and makes it before it next installs that
/// ASID ([`stale`]).
fn flush(flush: Flush) {
    #[cfg(debug_assertions)]
    audit::flushed(flush);
    stale::flushed(flush);
    flush_here(flush);
}

/// `flush` on this hart only, with no record: the stale mask's own flush, and a hart serving a
/// shootdown, which does not hold the kernel lock ([`shot_down`]).
fn flush_here(flush: Flush) {
    // SAFETY: each routine (asm.rs) is one `sfence.vma` and a `ret`. Dropping cached translations
    // is always sound; at worst it costs page-table walks.
    unsafe {
        match flush {
            Flush::All => flush_mmu(),
            Flush::Asid(asid) => flush_asid(asid),
            Flush::Page { page, asid } => flush_page(page, asid),
            Flush::Global(page) => flush_page_global(page),
        }
    }
}

/// Note a page-table write for the checked build's audit; a release build notes nothing.
fn wrote(_stale: Stale) {
    #[cfg(debug_assertions)]
    audit::wrote(_stale);
}

/// Point this hart's `satp` at `value`, which names an allocated address space.
fn write_satp(value: usize) {
    let _ = root_of(value); // refuses an unallocated mapping
    // SAFETY: every address space shares the kernel's root entries (see `allocate`), so the
    // code, stack and data in use right now stay mapped across the switch.
    unsafe { satp::write(satp::Satp::from_bits(value)) };
}

/// Whether `virt` is in the kernel half that every address space shares, the per-process entry
/// aside: its leaves are global (`G`), and a change to one is flushed in every ASID.
fn shared(virt: usize) -> bool {
    let index = physmap::vpn(virt, physmap::LEVELS - 1);
    index >= ROOT_KERNEL_START && index != ROOT_PROCESS_AREA
}

/// An address space as its flushes see it: its root, and its ASID, read from its `satp`.
#[derive(Copy, Clone)]
struct Space {
    root: Table,
    asid: usize,
}

impl Space {
    fn of(satp: usize) -> Space { Space { root: root_of(satp), asid: physmap::SATP.asid(satp) } }

    fn current() -> Space { Space::of(satp::read().bits()) }

    /// What a change to the leaf for `virt` may leave stale.
    fn leaf(self, virt: usize) -> Stale {
        Stale { asid: (!shared(virt)).then_some(self.asid), page: Some(virt & !(PAGE_SIZE - 1)) }
    }

    /// What a change to a table pointer on `virt`'s path may leave stale.
    fn tables(self, virt: usize) -> Stale { Stale { asid: (!shared(virt)).then_some(self.asid), page: None } }

    /// Flush a change to the leaf for `virt`: that address, in this ASID or, for a shared kernel
    /// leaf, in every one.
    fn flush_leaf(self, virt: usize) { flush(self.leaf(virt).flush()) }

    /// Flush a change to a table pointer on `virt`'s path: this whole ASID, or everything for a
    /// table of the shared kernel half (which never changes after boot).
    fn flush_tables(self, virt: usize) { flush(self.tables(virt).flush()) }

    /// Flush this whole ASID: a PID given out, or a space that is ending.
    fn flush_asid(self) { flush(Flush::Asid(self.asid)) }

    /// Write `pte` at `slot`, the leaf for `virt`, without its flush.
    fn write_leaf(self, slot: Slot, virt: usize, pte: Pte) {
        slot.set(pte);
        wrote(self.leaf(virt));
    }

    /// Write `pte` at `slot`, the leaf for `virt`, and flush it.
    fn set_leaf(self, slot: Slot, virt: usize, pte: Pte) {
        self.write_leaf(slot, virt, pte);
        self.flush_leaf(virt);
    }
}

/// The kernel's own `satp` (PID 1, ASID 1), from the boot's switch to it on: the space a
/// destruction moves to before it frees the dying one's frames.
static KERNEL_SATP: AtomicUsize = AtomicUsize::new(0);

/// The kernel's own `satp`, which a started hart enters with (`hart.rs`).
pub fn kernel_satp() -> usize { KERNEL_SATP.load(Ordering::Relaxed) }

/// The boot's ASID step, before anything else writes `satp`: write the kernel's `satp` with its
/// ASID field all ones, read it back and restore it, and return what was read, for the decision
/// (`process::check_asid_field`).
pub fn read_back_asid_ones() -> usize {
    let loader = satp::read().bits();
    write_satp(physmap::SATP.with_asid_ones(loader));
    let read_back = satp::read().bits();
    write_satp(loader);
    read_back
}

/// The boot's switch to the kernel's own ASID, PID 1, and its one whole flush, which also drops
/// the loader's ASID 0 entries and any the probe left.
pub fn enter_kernel_asid() {
    let kernel =
        physmap::SATP.make(physmap::SATP.root(satp::read().bits()), redoubt_layout::KERNEL_PID.get().into());
    write_satp(kernel);
    KERNEL_SATP.store(kernel, Ordering::Relaxed);
    flush(Flush::All);
    #[cfg(debug_assertions)]
    audit::check_globals();
}

/// A hart serving a shootdown of `asid` (`hart.rs`), before it takes the kernel lock: it leaves for
/// the kernel's own space, with no flush, drops `asid`'s translations, and fences its instruction
/// fetches. It reads no kernel cell.
pub fn shot_down(asid: usize) {
    write_satp(KERNEL_SATP.load(Ordering::Relaxed));
    flush_here(Flush::Asid(asid));
    sync_icache();
}

/// The stale mask (kernel/memory.md, "Residual risks"): per PID, the harts that must flush its
/// ASID, `(x0, pid)`, before they next install it. A flush of an ASID's entries on one hart marks
/// every other hart started; so does a PID given out (its whole-ASID flush) and a destruction (its
/// space's flush). On one hart the mask is always empty.
mod stale {
    use super::super::physmap::tlb::Flush;
    use super::super::process::MAX_PROCESS_COUNT;
    use crate::arch::hart;
    use crate::cell::KernelCell;

    static MASK: KernelCell<[u8; MAX_PROCESS_COUNT]> = KernelCell::new([0; MAX_PROCESS_COUNT]);

    /// The checked build's own record of the same debts, kept apart from the mask so that a
    /// mask that is not kept (`smp-no-stale-mask`) is caught: cleared only by a whole-ASID flush
    /// this hart makes.
    #[cfg(debug_assertions)]
    static DEBT: KernelCell<[u8; MAX_PROCESS_COUNT]> = KernelCell::new([0; MAX_PROCESS_COUNT]);

    /// The other harts started, as a mask.
    fn others() -> u8 { (((1u32 << hart::started()) - 1) as u8) & !(1 << hart::index()) }

    /// `flush` was made on this hart: the others owe it.
    pub(super) fn flushed(flush: Flush) {
        #[cfg(debug_assertions)]
        {
            let me = 1 << hart::index();
            DEBT.with(|d| match flush {
                Flush::Asid(asid) if asid > 0 => d[asid - 1] &= !me,
                Flush::All => d.iter_mut().for_each(|b| *b &= !me),
                _ => {}
            });
        }
        let asid = match flush {
            Flush::Asid(asid) | Flush::Page { asid, .. } if asid > 0 => asid,
            // A kernel-half leaf: none is removed after boot, so none needs another hart's flush.
            _ => return,
        };
        let others = others();
        if others == 0 {
            return;
        }
        MASK.with(|m| m[asid - 1] |= others);
        #[cfg(debug_assertions)]
        DEBT.with(|d| d[asid - 1] |= others);
    }

    /// This hart is about to install `asid`: if it owes that ASID a flush, it makes it now.
    pub(super) fn installing(asid: usize) {
        let me = 1 << hart::index();
        let owed = MASK.with(|m| {
            let owed = m[asid - 1] & me != 0;
            m[asid - 1] &= !me;
            owed
        });
        // Debug only, never in a bench build but one recorded negative run: the flush is skipped.
        if owed && !cfg!(feature = "smp-no-stale-mask") {
            super::flush_here(Flush::Asid(asid));
            #[cfg(debug_assertions)]
            DEBT.with(|d| d[asid - 1] &= !me);
        }
        #[cfg(debug_assertions)]
        assert!(
            DEBT.with(|d| d[asid - 1] & me == 0),
            "ASID audit: hart {} installs ASID {} with a flush owed and not made (stale mask)",
            hart::index(),
            asid
        );
    }
}

/// Before a dying address space's frames are freed (`release_owned_frames`): leave it if it is
/// this hart's, for the kernel's own, and then flush its ASID. A walker reading a freed table
/// through a live `satp` could cache a garbage leaf, and a garbage global leaf survives every ASID
/// flush.
pub fn leave(space: &MemoryMapping) {
    let current = satp::read().bits();
    if physmap::SATP.root(current) == physmap::SATP.root(space.satp) {
        write_satp(KERNEL_SATP.load(Ordering::Relaxed));
    }
    Space::of(space.satp).flush_asid();
}

/// The checked build's audit of flushes (kernel/memory-layout.md, "`satp`"): a log of the page-
/// table writes this hart has not flushed, which must be empty whenever the kernel returns to
/// user mode, and a walk of the current root that checks `G` is exactly on the shared kernel
/// half. It is the checked build's only, and outside the latency targets. Each hart keeps its own
/// log: a write is flushed on the hart that makes it, and the stale mask covers the others.
#[cfg(debug_assertions)]
pub mod audit {
    use super::super::physmap::tlb::{Flush, Stale, Unflushed};
    use super::super::physmap::{self, PteFlags};
    use super::{ROOT_KERNEL_START, ROOT_PROCESS_AREA, current_root, for_each_entry};
    use crate::arch::hart::{self, MAX_HARTS};
    use crate::cell::KernelCell;

    static LOG: KernelCell<[Unflushed<16>; MAX_HARTS]> =
        KernelCell::new([const { Unflushed::new() }; MAX_HARTS]);

    pub(super) fn wrote(stale: Stale) {
        if let Err(first) = LOG.with(|log| log[hart::index()].wrote(stale)) {
            panic!("ASID audit: more page-table writes unflushed than the log holds, the first: {}", first);
        }
    }

    pub(super) fn flushed(flush: Flush) { LOG.with(|log| log[hart::index()].flushed(flush)) }

    /// At every return from the trap handler, to user mode or to `kmain`: every write this hart
    /// made is flushed, or the kernel stops naming one.
    pub fn returning() {
        if let Some(stale) = LOG.with(|log| log[hart::index()].first()) {
            panic!("ASID audit: a page-table write is unflushed at a return from the kernel: {}", stale);
        }
    }

    /// `G` over the current root (kernel/memory-layout.md, "Entry bits"): every 4 KiB and
    /// root-level leaf of the shared kernel half is global, and nothing in the user half or on
    /// the per-process entry is, table pointers included. At boot and after each kernel-half
    /// mapping.
    pub(super) fn check_globals() {
        let root = current_root();
        for index in 0..physmap::ENTRIES {
            let pte = root.get(index);
            let base = index * physmap::leaf_size(physmap::LEVELS - 1);
            let shared = index >= ROOT_KERNEL_START && index != ROOT_PROCESS_AREA;
            let check = |virt: usize, global: bool, leaf: bool| {
                if shared && leaf {
                    assert!(global, "ASID audit: the shared kernel leaf at {:#x} is not global", virt);
                } else if !shared {
                    assert!(
                        !global,
                        "ASID audit: the entry at {:#x} is global outside the shared kernel half",
                        virt
                    );
                }
            };
            if pte.is_leaf() {
                check(base, pte.has(PteFlags::GLOBAL), true);
            } else if let Some(child) = root.child(index) {
                check(base, pte.has(PteFlags::GLOBAL), false);
                for_each_entry(child, physmap::LEVELS - 2, base, true, &mut |virt, pte| {
                    check(virt, pte.has(PteFlags::GLOBAL), pte.is_leaf())
                });
            }
        }
    }
}

/// Make this hart's instruction fetches see every store it made before (RISC-V `fence.i`,
/// Zifencei). Called after anything that makes memory executable for userspace: an image moved in
/// by `process_map`, and `map_anon`, `map_fixed`, `set_flags` or a demand-paged fault installing
/// X, and once at boot before the first user dispatch. It acts on this hart only: a hart also runs
/// it before it runs a process after the kernel's own thread (`sched.rs`), and a hart shot down
/// before it acknowledges ([`shot_down`]).
pub fn sync_icache() {
    // SAFETY: `fence.i` takes no operands, touches no memory the compiler tracks and changes no
    // register; it only orders this hart's later instruction fetches after its earlier stores.
    unsafe { core::arch::asm!("fence.i", options(nostack, preserves_flags)) };
}

/// First root entry belonging to the kernel half of the address space.
const ROOT_KERNEL_START: usize = physmap::ENTRIES / 2;
/// Root entry holding per-process kernel data. Everything else in the kernel half is shared.
const ROOT_PROCESS_AREA: usize = physmap::vpn(PROCESS_AREA, physmap::LEVELS - 1);
// `for_each_owned_frame` walks the user half and then this entry: each once.
const _: () = assert!(ROOT_PROCESS_AREA >= ROOT_KERNEL_START);

/// The root table of the address space that `satp` names.
fn root_of(satp: usize) -> Table {
    assert!(physmap::SATP.is_active(satp), "address space is not allocated");
    // SAFETY: a `satp` value with the mode bits set comes from the loader or from
    // `MemoryMapping::allocate()`, both of which store the address of a root page table.
    unsafe { Table::at(window(), physmap::SATP.root(satp)) }
}

fn current_root() -> Table { root_of(satp::read().bits()) }

/// Find the leaf (4 KiB) entry for `virt` under `root`. A missing table is `Unmapped`.
fn walk(root: Table, virt: usize) -> Result<Slot, PageError> {
    if !physmap::is_canonical(virt) {
        return Err(PageError::NonCanonical);
    }
    let mut table = root;
    for level in (1..physmap::LEVELS).rev() {
        // A missing table, or a superpage (the physmap), which is never edited at 4 KiB
        // granularity. A `match`, not `ok_or(..)?`: every system call's record copy walks here
        // twice a word, and the `Result` costs the release build two copies of the `Table` a
        // level (asid-cost).
        table = match table.child(physmap::vpn(virt, level)) {
            Some(child) => child,
            None => return Err(PageError::Unmapped),
        };
    }
    Ok(table.slot(physmap::vpn(virt, 0)))
}

/// [`walk`] in `space`, allocating each missing table on behalf of `pid`, and whether it linked
/// one: the caller's flush must then cover the whole ASID. If it fails after linking one, it
/// flushes that itself.
fn walk_making(
    space: Space,
    virt: usize,
    mm: &mut MemoryManager,
    pid: Pid,
) -> Result<(Slot, bool), PageError> {
    if !physmap::is_canonical(virt) {
        return Err(PageError::NonCanonical);
    }
    let mut linked = false;
    let mut table = space.root;
    for level in (1..physmap::LEVELS).rev() {
        let index = physmap::vpn(virt, level);
        table = match table.child(index) {
            Some(child) => child,
            None => {
                // A superpage (the physmap). These are never edited at 4 KiB granularity.
                let frame =
                    if table.get(index).is_leaf() { Err(PageError::Unmapped) } else { mm.alloc_page(pid) };
                let frame = frame.inspect_err(|_| {
                    if linked {
                        space.flush_tables(virt);
                    }
                })?;
                // SAFETY: `alloc_page` returns a RAM frame that was free until now.
                let child = unsafe { table.slot(index).install_table(frame) };
                wrote(space.tables(virt));
                linked = true;
                child
            }
        };
    }
    Ok((table.slot(physmap::vpn(virt, 0)), linked))
}

/// Map `phys` at `virt` in `space`, allocating tables on behalf of `pid`, and return whether a
/// table was linked. The caller flushes (`flush_map`); a failure has flushed what it linked.
fn map_page_in(
    space: Space,
    mm: &mut MemoryManager,
    pid: Pid,
    phys: usize,
    virt: usize,
    flags: PteFlags,
) -> Result<bool, PageError> {
    assert!(virt & (PAGE_SIZE - 1) == 0);
    assert!(phys & (PAGE_SIZE - 1) == 0);
    check_permissions(flags)?;
    let (slot, linked) = walk_making(space, virt, mm, pid)?;
    if is_occupied(slot.get()) {
        klog!("Page {:08x} already allocated!", virt);
        if linked {
            space.flush_tables(virt);
        }
        return Err(PageError::InUse);
    }
    space.write_leaf(slot, virt, Pte::leaf(phys, flags));
    Ok(linked)
}

/// The flush after a new mapping at `virt` in `space`: the leaf, or the whole ASID if a table
/// was linked on the way.
fn flush_map(space: Space, virt: usize, linked: bool) {
    if linked { space.flush_tables(virt) } else { space.flush_leaf(virt) }
}

/// A page that is mapped, or lent out (the `S` bit, with `VALID` cleared). A lent page's entry
/// is the lender's only record of the loan: the borrower's return restores it. So nothing but
/// that return may overwrite it: not a new mapping or an unmap.
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
/// fail, so a transfer of many pages can prepare them all before it changes anything. A table it
/// links is flushed here, since the mapping that follows links none.
pub fn prepare_map(
    mm: &mut MemoryManager,
    space: &MemoryMapping,
    pid: Pid,
    virt: usize,
) -> Result<(), PageError> {
    let space = Space::of(space.satp);
    let (slot, linked) = walk_making(space, virt, mm, pid)?;
    if linked {
        space.flush_tables(virt);
    }
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
    for_each_entry(table, level, base, false, f);
}

/// [`for_each_leaf`], and with `tables` also every entry that links a table below `table`, after
/// everything under it, so `f` may give that table's frame back as soon as it sees it: one
/// traversal for both.
fn for_each_entry(table: Table, level: usize, base: usize, tables: bool, f: &mut impl FnMut(usize, Pte)) {
    for index in 0..physmap::ENTRIES {
        let pte = table.get(index);
        let virt = base + index * physmap::leaf_size(level);
        if level == 0 {
            if pte.is_valid() || pte.has(PteFlags::S) {
                f(virt, pte);
            }
        } else if let Some(child) = table.child(index) {
            for_each_entry(child, level - 1, virt, tables, f);
            if tables {
                f(virt, pte);
            }
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
        write!(fmt, "(satp: {:#x}, root: {:#x})", self.satp, physmap::SATP.root(self.satp))
    }
}

/// Controls MMU configurations.
impl MemoryMapping {
    /// A boot process's space, with its PID as its ASID: the loader built it with ASID 0, and the
    /// boot's whole flush (`enter_kernel_asid`) came after every table it wrote.
    ///
    /// # Safety
    /// `init` must be a process description produced by the loader.
    pub unsafe fn from_init_process(&mut self, init: InitialProcess) {
        self.satp = physmap::SATP.make(physmap::SATP.root(init.satp), init.pid().get().into());
    }

    /// Allocate a brand-new memory mapping. The new address space contains:
    ///
    ///     1. Every shared kernel root entry (physmap and kernel), copied from the current root.
    ///     2. The header page at `PROCESS_AREA`, so the process can be run.
    ///
    /// All pages, including the page tables themselves, are owned by `pid`, so they are
    /// released along with everything else when the process is destroyed.
    ///
    /// Its ASID is `pid`, which an earlier process may have held and left translations under:
    /// the kernel does not track which PIDs have run, so every allocation flushes the whole ASID,
    /// after the last table write and before the space can first run.
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

        // From here the space names everything it takes, so a failure gives it all back by one
        // walk of its tables, and the space is whole or does not exist (`process_create`'s
        // rollback walks it, `release_owned_frames`).
        self.satp = physmap::SATP.make(root_phys, pid.get().into());
        let space = Space::of(self.satp);
        // Whatever the PID's last holder left, and the root entries just copied.
        wrote(Stale { asid: Some(space.asid), page: None });
        // A failure's release leaves the space and flushes its ASID (`leave`).
        if let Err(e) = Self::add_header_page(space, mm, pid) {
            mm.release_owned_frames(pid, self);
            self.satp = 0;
            return Err(e);
        }
        #[cfg(not(feature = "asid-no-reuse-flush"))]
        space.flush_asid();
        Ok(())
    }

    /// Back and map a new space's header page at `PROCESS_AREA`: a frame charged to the running
    /// budget (kernel/objects.md), named in the account once it is mapped. On failure the frame
    /// is given back, and any table the mapping took is in the space. Its flush is `allocate`'s.
    fn add_header_page(space: Space, mm: &mut MemoryManager, pid: Pid) -> Result<(), PageError> {
        let header_phys = mm.alloc_context_page(pid)?;
        // SAFETY: `alloc_context_page` returns a RAM frame that was free until now.
        unsafe { window().zero_frame(header_phys) };
        map_page_in(space, mm, pid, header_phys, PROCESS_AREA, PteFlags::R | PteFlags::W)
            .inspect_err(|_| mm.free_frame_of(header_phys, pid).expect("the frame just taken"))?;
        mm.set_header(pid, header_phys);
        Ok(())
    }

    /// Get the currently active memory mapping.
    pub fn current() -> MemoryMapping { MemoryMapping { satp: satp::read().bits() } }

    /// Set this mapping as the systemwide mapping.
    /// **Note:** This should only be called from an interrupt in the
    /// kernel, which should be mapped into every possible address space.
    /// As such, this will only have an observable effect once code returns
    /// to userspace.
    ///
    /// It flushes nothing: the space's cached translations carry its ASID, and every change to
    /// its tables was flushed when it was made.
    /// Install this space on this hart, flushing its ASID first if the hart owes it a flush.
    pub fn activate(self) {
        stale::installing(physmap::SATP.asid(self.satp));
        write_satp(self.satp);
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

    /// Call `f(phys, lent)` with every frame this address space's own entries name: its root,
    /// every table below it, and every occupied leaf, in the user half and in the process area
    /// (the kernel half's other entries are shared, never this space's). `lent` marks a page this
    /// space lent out (`S` without `VALID`); a borrowed page (`VALID | S`) is another's frame,
    /// which the caller tells apart by the ownership table. One walk, whose cost follows the
    /// tables the space has, not RAM. Each table comes after everything under it, and the root
    /// last, so `f` may give a frame back as soon as it sees it, and nothing is read from a frame
    /// after (`release_owned_frames`).
    pub fn for_each_owned_frame(&self, mut f: impl FnMut(usize, bool)) {
        let root = root_of(self.satp);
        for index in (0..ROOT_KERNEL_START).chain([ROOT_PROCESS_AREA]) {
            let Some(child) = root.child(index) else { continue };
            let base = index * physmap::leaf_size(physmap::LEVELS - 1);
            for_each_entry(child, physmap::LEVELS - 2, base, true, &mut |_virt, pte| {
                f(pte.phys(), pte.has(PteFlags::S) && !pte.is_valid())
            });
            f(root.get(index).phys(), false);
        }
        f(physmap::SATP.root(self.satp), false);
    }

    pub fn print_map(&self) {
        println!("Memory Maps for satp {:#x}:", self.satp);
        self.for_each_user_leaf(|virt, pte| {
            println!("    {:016x} -> {:010x} ({:?})", virt, pte.phys(), pte.flags());
        });
        println!("End of map");
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
    // A page of the shared kernel half (the PLIC, which the kernel maps at boot into tables the
    // loader shared) is global, like every leaf there.
    let flags = translate_flags(req_flags)
        | if map_user { PteFlags::USER } else { PteFlags::NONE }
        | if shared(virt) { PteFlags::GLOBAL } else { PteFlags::NONE };
    let space = Space::current();
    let linked = map_page_in(space, mm, pid, phys, virt, flags)?;
    flush_map(space, virt, linked);
    #[cfg(debug_assertions)]
    if shared(virt) {
        audit::check_globals();
    }
    Ok(())
}

/// Map device registers `phys` at kernel address `virt`, read-write and for the kernel alone (no
/// U bit), global like every shared kernel leaf: the DMA register window. The loader created and
/// shared the tables above it (`reserve_tables`), so nothing is allocated and every address space
/// sees the page; a missing table is a boot bug, and the kernel stops.
pub fn map_kernel_page(phys: usize, virt: usize) {
    let slot = walk(current_root(), virt).expect("the loader shares the window's page tables");
    assert!(!is_occupied(slot.get()), "kernel window page {:x} is already mapped", virt);
    let flags = translate_flags(MemFlags::READ | MemFlags::WRITE) | PteFlags::GLOBAL;
    Space::current().set_leaf(slot, virt, Pte::leaf(phys, flags));
    #[cfg(debug_assertions)]
    audit::check_globals();
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
    let space = Space::current();
    let slot = walk(space.root, virt)?;
    if slot.get().has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    let phys = slot.get().phys();
    // Test builds only, one recorded negative run: the unmap skips its flush, for the audit to
    // catch (`asid-reuse-stale`).
    if cfg!(feature = "asid-no-leaf-flush") {
        space.write_leaf(slot, virt, Pte::EMPTY);
    } else {
        space.set_leaf(slot, virt, Pte::EMPTY);
    }
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
    let (src_space, dest_space) = (Space::of(src_space.satp), Space::of(dest_space.satp));
    let (src_addr, dest_addr) = (src_addr as usize, dest_addr as usize);
    let src = walk(src_space.root, src_addr)?;
    let phys = src.get().phys();
    // Check both protected aliases and their frame identity before changing either. The
    // borrower's entry is `VALID | S`; the lender's is `S` without `VALID`.
    if !src.get().is_valid() || !src.get().has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    let dest = walk(dest_space.root, dest_addr).or(Err(PageError::Lent))?;
    if dest.get().is_valid() || !dest.get().has(PteFlags::S) || dest.get().phys() != phys {
        return Err(PageError::Lent);
    }
    src_space.set_leaf(src, src_addr, Pte::EMPTY);
    dest_space.set_leaf(dest, dest_addr, dest.get().without(PteFlags::S | PteFlags::P).with(PteFlags::VALID));
    Ok(phys)
}

/// Take `virt` out of `space`, remembering the loan: clear `VALID` so the lender cannot touch
/// the page, and set `S` so that its entry is the record of the loan (I9). Returns the frame.
/// It maps nothing into the receiver: a Redoubt message is taken out of its sender when it is
/// sent and mapped into its receiver only when someone takes it, which may be much later or
/// never (`message.rs`).
pub fn lend_out(space: &MemoryMapping, virt: usize) -> Result<usize, PageError> {
    let space = Space::of(space.satp);
    let slot = walk(space.root, virt)?;
    let pte = slot.get();
    if !pte.is_valid() || pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    space.set_leaf(slot, virt, pte.without(PteFlags::VALID).with(PteFlags::S));
    Ok(pte.phys())
}

/// The frame of `space`'s header page, mapped at `PROCESS_AREA` (the loader's, for `init`).
pub fn header_phys(space: &MemoryMapping) -> Option<usize> {
    let pte = walk(root_of(space.satp), PROCESS_AREA).ok()?.get();
    pte.is_valid().then(|| pte.phys())
}

/// The frame behind a page `space` lent out, from the lender's own entry.
pub fn lent_frame(space: &MemoryMapping, virt: usize) -> Option<usize> {
    let pte = walk(root_of(space.satp), virt).ok()?.get();
    (pte.has(PteFlags::S) && !pte.is_valid()).then(|| pte.phys())
}

/// Give a lent page back to its lender: `VALID` again, `S` cleared (`reply`, or a message that
/// never went through).
pub fn lend_back(space: &MemoryMapping, virt: usize) -> Result<(), PageError> {
    let space = Space::of(space.satp);
    let slot = walk(space.root, virt)?;
    let pte = slot.get();
    if pte.is_valid() || !pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    space.set_leaf(slot, virt, pte.without(PteFlags::S).with(PteFlags::VALID));
    Ok(())
}

/// The lender never gets this page back: a transfer (R4), or a lend whose call was abandoned
/// (R3). Its entry goes; the frame is the receiver's. Returns the frame.
pub fn drop_lent(space: &MemoryMapping, virt: usize) -> Result<usize, PageError> {
    let space = Space::of(space.satp);
    let slot = walk(space.root, virt)?;
    let pte = slot.get();
    if pte.is_valid() || !pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    space.set_leaf(slot, virt, Pte::EMPTY);
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
    let space = Space::of(space.satp);
    let linked = map_page_in(space, mm, pid, phys, virt, flags)?;
    flush_map(space, virt, linked);
    Ok(())
}

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
    let space = Space::of(space.satp);
    let linked = map_page_in(space, mm, pid, phys, virt, flags)?;
    flush_map(space, virt, linked);
    Ok(())
}

/// Whether `virt` is free in `space`, an address space that need not be the running one
/// (`process_map` looks into a child that has never run): its entry is empty.
pub fn address_available_in(space: &MemoryMapping, virt: usize) -> bool {
    debug_assert!(virt < redoubt_sys::USER_AREA_END, "process_map checks its range first");
    match walk(root_of(space.satp), virt) {
        // No leaf table yet, so nothing is mapped there. Inside user space the only other way
        // `walk` fails is a non-canonical address, which the caller has already ruled out.
        Err(_) => true,
        // Not just "not occupied": a reservation is not a mapping but it is a claim on the
        // address, and `process_map` never overwrites one.
        Ok(slot) => slot.get().is_empty(),
    }
}

/// Whether every page in `[addr, addr + len)` is free in `space` (`map_fixed`'s overlap check).
/// `addr` and `addr + len` are assumed already checked (page-aligned, in user space): this only
/// interprets what it finds in the page tables.
pub fn range_available_in(space: &MemoryMapping, addr: usize, len: usize) -> bool {
    first_occupied(root_of(space.satp), physmap::LEVELS - 1, addr, addr + len).is_none()
}

/// The first page of `[start, end)` that is occupied in the current address space, if any:
/// what the placement search (`MemoryManager::find_virtual_address`) skips past.
pub fn first_occupied_page(start: usize, end: usize) -> Option<usize> {
    debug_assert!(end <= USER_AREA_END, "the placement areas are in user space");
    first_occupied(current_root(), physmap::LEVELS - 1, start, end)
}

/// The first page in `[start, end)` that is occupied under `table`, a table at `level`: like
/// `address_available_in` for each page (any nonzero PTE, including a reservation or either side
/// of a loan, is occupied), but a missing subtree is skipped as a whole instead of walking it one
/// page at a time. So a range costs at most the root entries it spans plus `ENTRIES` (512 on
/// Sv39, 1024 on Sv32) per table actually present in it, never one walk per page, however long
/// the range is. `start` and `end` are page-aligned but need not be aligned to `level`'s span;
/// each entry's coverage is computed from its own index, not from `start`.
fn first_occupied(table: Table, level: usize, start: usize, end: usize) -> Option<usize> {
    if level == 0 {
        // A leaf table, reached with `[start, end)` inside it: one read per page. This loop is
        // the search's whole cost when the tables are there, so it does nothing else.
        let first = physmap::vpn(start, 0);
        let pages = (end - start) / PAGE_SIZE;
        return (first..first + pages)
            .find(|index| !table.get(*index).is_empty())
            .map(|index| start + (index - first) * PAGE_SIZE);
    }
    let span = physmap::leaf_size(level);
    let mut virt = start;
    while virt < end {
        // The next boundary strictly after `virt` at this level's granularity, clamped to `end`.
        let boundary = (virt | (span - 1)).wrapping_add(1);
        let next = boundary.min(end);
        let index = physmap::vpn(virt, level);
        let pte = table.get(index);
        if !pte.is_empty() {
            match table.child(index) {
                // A live subtree: search it over the clipped range.
                Some(child) => {
                    if let Some(page) = first_occupied(child, level - 1, virt, next) {
                        return Some(page);
                    }
                }
                // Nonempty but not a table: a leaf this high (user space has none on either
                // width) or a malformed entry. Fail closed: all of it counts as occupied.
                None => return Some(virt),
            }
        }
        // Empty entry: the whole `[virt, next)` gap is free (matches `walk`'s "missing table
        // means nothing is mapped" reading, used by `address_available_in`), so skip it.
        virt = next;
    }
    None
}

/// Free the page tables under `[start, end)` of `space`'s user half that map nothing any more,
/// bottom-up, and give each back to the budget that paid for it (R6; kernel/memory.md, "Page
/// tables"). A call that empties entries runs this once it has finished, never between preparing a
/// table and filling it: a table `prepare_map` made is empty until its page is mapped.
///
/// One leaf table per span, and the tables above it only when one below was freed: at most
/// `ENTRIES` reads for each table the range reaches, so the cost follows the pages the call
/// unmapped (R22). The root is never freed, nor anything in the kernel half.
pub fn free_empty_tables(mm: &mut MemoryManager, space: &MemoryMapping, start: usize, end: usize) {
    debug_assert!(end <= USER_AREA_END, "only the user half's tables are ever freed");
    let space = Space::of(space.satp);
    let root = space.root;
    let leaf_span = physmap::leaf_size(1);
    let mut virt = start & !(leaf_span - 1);
    while virt < end {
        // The tables on `virt`'s path, root first; `path[level]` is the table at `level`.
        let mut path = [None; physmap::LEVELS];
        path[physmap::LEVELS - 1] = Some(root);
        for level in (1..physmap::LEVELS).rev() {
            path[level - 1] = path[level].and_then(|t| t.child(physmap::vpn(virt, level)));
        }
        for level in 0..physmap::LEVELS - 1 {
            let (Some(table), Some(parent)) = (path[level], path[level + 1]) else { break };
            if (0..physmap::ENTRIES).any(|index| !table.get(index).is_empty()) {
                break;
            }
            let slot = parent.slot(physmap::vpn(virt, level + 1));
            let frame = slot.get().phys();
            // The table goes back to whoever paid for it (`walk` charges the PID it maps for),
            // as the ownership table records it. A table with no owner there is not one
            // `walk` made, and is left where it is.
            let Some(owner) = mm.ram_owner(frame) else { break };
            // Unlinked and its ASID flushed first, so no walk reaches a table that is free.
            slot.set(Pte::EMPTY);
            wrote(space.tables(virt));
            space.flush_tables(virt);
            mm.free_frame_of(frame, owner).expect("the owner just read releases its frame");
        }
        virt += leaf_span;
    }
}

/// Unmap only the protected borrower alias when a lend leaves the server.
pub fn unmap_from(space: &MemoryMapping, virt: usize) -> Result<usize, PageError> {
    let space = Space::of(space.satp);
    let slot = walk(space.root, virt)?;
    let pte = slot.get();
    if !pte.is_valid() || !pte.has(PteFlags::S) {
        return Err(PageError::Lent);
    }
    space.set_leaf(slot, virt, Pte::EMPTY);
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
    checked_phys(walk(current_root(), virt)?.get())
}

/// Back a reserved (demand-paged) address with a real, zeroed page.
///
/// Takes the memory manager rather than borrowing it: the lend and move paths reach here
/// while they already hold it, and a second borrow of the same `KernelCell` panics the
/// kernel (or, with `smp`, deadlocks). The page-fault handler borrows it for this call.
pub fn ensure_page_exists_inner(mm: &mut MemoryManager, address: usize) -> Result<usize, PageError> {
    // Disallow mapping memory outside of user land
    if crate::arch::current_pid() != redoubt_layout::KERNEL_PID && address >= USER_AREA_END {
        return Err(PageError::Unmapped);
    }
    let virt = address & !(PAGE_SIZE - 1);
    let space = Space::current();
    let slot = walk(space.root, virt).or(Err(PageError::Unmapped))?;
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
    // The leaf is the current space's own, flushed in its ASID. For the kernel PID above
    // `USER_AREA_END` it would be in a table every space shares: global, flushed in every ASID,
    // and checked by the `G` walk. No caller reaches that today: the loader reserves no page there.
    let global = if shared(virt) { PteFlags::GLOBAL } else { PteFlags::NONE };
    space.set_leaf(slot, virt, Pte::leaf(new_page, reservation.flags() | PteFlags::USER | global));
    #[cfg(debug_assertions)]
    if shared(virt) {
        audit::check_globals();
    }

    Ok(new_page)
}

/// The frame behind user address `virt` of the current address space, if the process may read
/// it (and, with `write`, write it) there: a system call about to copy a record in or a result
/// out. Anything else (unmapped, reserved but never touched, lent out, kernel, no permission)
/// is `InvalidArgument`: decoding never allocates (kernel/abi.md, "The record check"), so a
/// process touches its record buffers before a call. This also excludes a live `VALID | S`
/// borrower alias: userspace can access it normally, but cannot use it as syscall-owned RAM.
pub fn user_frame(virt: usize, write: bool) -> Result<usize, redoubt_sys::Error> {
    use redoubt_sys::Error;
    if virt >= USER_AREA_END {
        return Err(Error::InvalidArgument);
    }
    let page = virt & !(PAGE_SIZE - 1);
    let pte = walk(current_root(), page).map_err(|_| Error::InvalidArgument)?.get();
    // A writable record must be readable too (R11), so a write-only entry is never one.
    let wanted =
        PteFlags::VALID | PteFlags::USER | PteFlags::R | if write { PteFlags::W } else { PteFlags::NONE };
    if !pte.has(wanted) || pte.has(PteFlags::S) {
        return Err(Error::InvalidArgument);
    }
    Ok(pte.phys())
}

/// `set_flags` (kernel/memory.md, R11): give a mapped user page exactly the permissions
/// `flags` asks for, keeping everything else about the entry (its frame, `USER`, the
/// accessed and dirty bits). It may add a permission as well as drop one: a program maps a page writable,
/// writes code into it, and then makes it executable and not writable, which is what W^X asks of it.
/// `Pte::leaf` refuses the combination that would break W^X, as decoding already did.
///
/// A page that is not the caller's own unshared live mapping -- unmapped, reserved but never
/// touched, lent out, or a protected borrower alias -- is `BadAddress`, which the caller
/// reports as `InvalidArgument`.
pub fn set_user_page_flags(virt: usize, flags: MemFlags) -> Result<(), PageError> {
    let wanted = translate_flags(flags);
    check_permissions(wanted)?;
    let space = Space::current();
    let slot = walk(space.root, virt)?;
    let pte = slot.get();
    if !pte.is_valid() || pte.has(PteFlags::S) || !pte.has(PteFlags::USER) {
        return Err(PageError::Unmapped);
    }
    let keep = pte.flags() - (PteFlags::R | PteFlags::W | PteFlags::X);
    space.set_leaf(slot, virt, Pte::leaf(pte.phys(), keep | wanted));
    Ok(())
}

/// Whether `virt` is a live, user-visible mapping of the current address space that is not
/// either alias of a loan: what `unmap` and `set_flags` need before either changes one
/// (kernel/memory.md: check the whole range first). The frame it maps, for the caller to check
/// who owns it.
pub fn user_mapping(virt: usize) -> Option<usize> {
    let pte = walk(current_root(), virt).ok()?.get();
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
pub fn is_mapped(virt: usize) -> bool { walk(current_root(), virt).is_ok_and(|slot| slot.get().is_valid()) }

/// The last address this hart retried as a stale translation ([`retry_stale`]).
static RETRIED: crate::cell::KernelCell<[usize; crate::arch::hart::MAX_HARTS]> =
    crate::cell::KernelCell::new([usize::MAX; crate::arch::hart::MAX_HARTS]);

/// A user load (`write` false) or store at `virt` faulted, yet the current space's leaf for it is
/// valid, a user page and allows the access: this hart cached an older entry another hart has
/// since changed (kernel/memory.md, "Residual risks"). Flush that address in this ASID and say to
/// retry the instruction; but not twice running for one address on this hart, so a fault the
/// flush does not cure ends as an ordinary fault.
pub fn retry_stale(virt: usize, write: bool) -> bool {
    let need = if write { PteFlags::W } else { PteFlags::R };
    let allowed = walk(current_root(), virt).is_ok_and(|slot| {
        let pte = slot.get();
        pte.is_valid() && pte.has(PteFlags::USER) && pte.has(need)
    });
    let page = virt & !(PAGE_SIZE - 1);
    let again = RETRIED.with(|r| {
        let last = &mut r[crate::arch::hart::index()];
        let again = *last == page;
        *last = if allowed && !again { page } else { usize::MAX };
        again
    });
    if !allowed || again {
        return false;
    }
    Space::current().flush_leaf(page);
    true
}

/// This hart entered the kernel for anything but a page fault: it made progress since any retry.
pub fn retry_reset() { RETRIED.with(|r| r[crate::arch::hart::index()] = usize::MAX) }

/// The permissions of the page at `virt`, a page-aligned address: `None` if it has none or is
/// either alias of a loan.
pub fn page_flags(virt: usize) -> Option<MemFlags> {
    let pte = walk(current_root(), virt).ok()?.get();
    if pte.has(PteFlags::S) {
        return None;
    }
    let mut flags = MemFlags::NONE;
    let bits =
        [(PteFlags::R, MemFlags::READ), (PteFlags::W, MemFlags::WRITE), (PteFlags::X, MemFlags::EXECUTE)];
    for (bit, flag) in bits {
        if pte.has(bit) {
            flags = flags | flag;
        }
    }
    (flags != MemFlags::NONE).then_some(flags)
}
