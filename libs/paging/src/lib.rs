//! Typed RISC-V page tables, for both Sv32 (rv32) and Sv39 (rv64).
//!
//! The two modes share the low ten PTE flag bits and the PPN position (bit 10); they
//! differ only in level count, entries per table, VPN width, and the `satp` fields, which
//! are `cfg(target_pointer_width)` constants below.
//!
//! This crate is the only code that touches page-table memory, for both the loader and
//! the kernel. Everything above it manipulates `Pte` values through `Table` and `Slot`
//! in safe code.
//!
//! Page tables hold physical addresses, so whoever edits them needs a way to reach a
//! physical frame. A `Window` describes that: the loader runs with the MMU off
//! (`Window::IDENTITY`), and the kernel reaches RAM through its physmap
//! (`Window::offset(PHYSMAP_BASE, PHYSMAP_SIZE)`).
//!
//! # Why the unsafe code here is sound
//!
//! 1. *The window is real.* Whoever constructs a `Window` promises (it is an `unsafe fn`) that `base + phys`
//!    is a valid, writable address for every RAM frame below `size`, for as long as any `Table` made from it
//!    is in use.
//! 2. *A `Table` is always a page table.* One is only created by `Table::at`, whose caller vouches for the
//!    frame (a root named by `satp`), by `Table::child`, which follows a valid non-leaf entry, or by
//!    `Slot::install_table`. Non-leaf entries are written only by `install_table`, which zeroes the frame
//!    first, or takes it zero from a caller that vouches for it; `Slot::set` refuses them. So every non-leaf
//!    entry names a page table, by induction. Tables must not be freed while an entry still points at them;
//!    that is the caller's obligation and is stated on `at`.
//! 3. *Accesses do not race.* Entries are read and written whole, with volatile accesses through a raw
//!    pointer; no reference to table memory is ever formed. The callers are single-threaded (the loader) or
//!    run with interrupts off on one hart (the kernel). SMP must put address-space edits under a lock before
//!    this stops being true.

#![no_std]

use core::ptr::NonNull;

use redoubt_sys::PAGE_SIZE;

pub mod tlb;

// Mode-specific parameters. Sv32: 2 levels of 1024 entries, 10 VPN bits. Sv39: 3 levels of 512
// entries, 9 VPN bits. Each width's `satp` is laid out as `SV32_SATP` or `SV39_SATP` says.
#[cfg(target_pointer_width = "32")]
mod mode {
    pub const LEVELS: usize = 2;
    pub const ENTRIES: usize = 1024;
    pub const VPN_BITS: usize = 10;
    pub const SATP: super::SatpLayout = super::SV32_SATP;
}
#[cfg(target_pointer_width = "64")]
mod mode {
    pub const LEVELS: usize = 3;
    pub const ENTRIES: usize = 512;
    pub const VPN_BITS: usize = 9;
    pub const SATP: super::SatpLayout = super::SV39_SATP;
}
pub use mode::*;

/// Where one width's `satp` fields lie (the privileged spec's `satp`): the mode, the address-space
/// id (ASID) and the root table's physical page number. A 64-bit build has both layouts, so the
/// host tests check each; a 32-bit build has Sv32's alone.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SatpLayout {
    /// The mode field holding this width's paging mode, in place.
    pub mode: usize,
    /// The ASID field's lowest bit.
    pub asid_shift: u32,
    /// The ASID field's width: 9 bits in Sv32, 16 in Sv39.
    pub asid_bits: u32,
    /// The root's physical page number, in the low bits.
    pub ppn_mask: usize,
}

/// Sv32: mode bit 31, ASID bits 22 to 30, PPN bits 0 to 21.
pub const SV32_SATP: SatpLayout =
    SatpLayout { mode: 1 << 31, asid_shift: 22, asid_bits: 9, ppn_mask: (1 << 22) - 1 };
/// Sv39: mode 8 in bits 60 to 63, ASID bits 44 to 59, PPN bits 0 to 43.
#[cfg(target_pointer_width = "64")]
pub const SV39_SATP: SatpLayout =
    SatpLayout { mode: 8 << 60, asid_shift: 44, asid_bits: 16, ppn_mask: (1 << 44) - 1 };

impl SatpLayout {
    const fn asid_mask(self) -> usize { ((1 << self.asid_bits) - 1) << self.asid_shift }

    /// Paging on, the root table at `root_phys`, and `asid`, which must fit the field: the kernel
    /// gives each process its PID, and the loader 0 for what it builds (kernel/memory-layout.md,
    /// "`satp`").
    pub fn make(self, root_phys: usize, asid: usize) -> usize {
        assert!(asid < 1 << self.asid_bits, "ASID {asid} is wider than satp's field");
        self.mode | (asid << self.asid_shift) | (root_phys >> 12)
    }

    /// The root table's physical address.
    pub fn root(self, satp: usize) -> usize { (satp & self.ppn_mask) << 12 }

    /// The address space's ASID.
    pub fn asid(self, satp: usize) -> usize { (satp & self.asid_mask()) >> self.asid_shift }

    /// Whether `satp` names an allocated (paging-enabled) address space.
    pub fn is_active(self, satp: usize) -> bool { satp & self.mode != 0 }

    /// `satp` with its ASID field all ones: what the boot writes to find the hart's field.
    pub fn with_asid_ones(self, satp: usize) -> usize { satp | self.asid_mask() }

    /// The boot's decision on the ASID field (kernel/boot.md, R17), from the `satp` the hart
    /// read back after a write of `with_asid_ones`. A hart implements the field's low bits and
    /// holds the rest at zero, so its width is the run of ones from the lowest bit. `Ok(width)`
    /// if that holds `needed` bits, or else `Err(width)`: the boot is refused.
    pub fn asid_width(self, readback: usize, needed: u32) -> Result<u32, u32> {
        let width = self.asid(readback).trailing_ones();
        if width >= needed { Ok(width) } else { Err(width) }
    }
}

pub const ENTRIES: usize = mode::ENTRIES;
pub const LEVELS: usize = mode::LEVELS;
/// Bytes mapped by one root entry, which is also the size of the largest leaf.
pub const LARGEST_LEAF: usize = leaf_size(LEVELS - 1);

bitflags::bitflags! {
    /// The low ten bits of an entry. `S` and `P` are the two bits the architecture leaves
    /// to software; Redoubt uses them for lent and swapped pages.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct PteFlags: usize {
        const VALID  = 1 << 0;
        const R      = 1 << 1;
        const W      = 1 << 2;
        const X      = 1 << 3;
        const USER   = 1 << 4;
        const GLOBAL = 1 << 5;
        const A      = 1 << 6;
        const D      = 1 << 7;
        const S      = 1 << 8;
        const P      = 1 << 9;
    }
}

impl PteFlags {
    pub const NONE: PteFlags = PteFlags::empty();
    const PERMISSIONS: PteFlags = PteFlags::R.union(PteFlags::W).union(PteFlags::X);
}

/// One page table entry.
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct Pte(usize);

impl Pte {
    pub const EMPTY: Pte = Pte(0);

    /// A valid mapping of the frame (or, in a root table, gigapage) at `phys`. Accessed and
    /// Dirty are set up front, because not every hart updates them in hardware.
    pub fn leaf(phys: usize, flags: PteFlags) -> Pte {
        assert!(flags.intersects(PteFlags::PERMISSIONS), "a leaf needs at least one permission");
        // W^X: nothing is ever mapped both writable and executable.
        assert!(!flags.contains(PteFlags::W | PteFlags::X), "refusing a writable and executable mapping");
        // R11: nothing is ever mapped writable without being readable; the privileged
        // architecture reserves that encoding.
        assert!(
            !flags.contains(PteFlags::W) || flags.contains(PteFlags::R),
            "refusing a writable and unreadable mapping"
        );
        Pte(((phys >> 12) << 10) | (flags | PteFlags::VALID | PteFlags::A | PteFlags::D).bits())
    }

    /// A reservation for demand paging: permissions, but no frame and not valid.
    pub fn reservation(flags: PteFlags) -> Pte { Pte((flags - PteFlags::VALID).bits()) }

    /// A pointer to a next-level table: valid, with no permissions.
    fn table(phys: usize) -> Pte { Pte(((phys >> 12) << 10) | PteFlags::VALID.bits()) }

    pub fn is_empty(self) -> bool { self.0 == 0 }

    pub fn is_valid(self) -> bool { self.has(PteFlags::VALID) }

    /// A valid entry that maps memory, as opposed to pointing at another table.
    pub fn is_leaf(self) -> bool { self.is_valid() && self.flags().intersects(PteFlags::PERMISSIONS) }

    pub fn is_table(self) -> bool { self.is_valid() && !self.flags().intersects(PteFlags::PERMISSIONS) }

    pub fn has(self, flags: PteFlags) -> bool { self.flags().contains(flags) }

    pub fn flags(self) -> PteFlags { PteFlags::from_bits_truncate(self.0) }

    pub fn phys(self) -> usize { (self.0 >> 10) << 12 }

    #[must_use]
    pub fn with(self, flags: PteFlags) -> Pte { Pte(self.0 | flags.bits()) }

    #[must_use]
    pub fn without(self, flags: PteFlags) -> Pte { Pte(self.0 & !flags.bits()) }
}

/// How the current code reaches physical frames: `virtual = base + physical`.
#[derive(Copy, Clone)]
pub struct Window {
    /// `virt = offset.wrapping_add(phys)`.
    offset: usize,
    /// Physical addresses this window can reach: `[phys_start, phys_end)`.
    phys_start: usize,
    phys_end: usize,
}

impl Window {
    /// A window mapping physical range `phys` at `virt = offset + phys`.
    ///
    /// # Safety
    /// For every frame in `phys`, `offset.wrapping_add(frame)` must be a valid, writable
    /// address of that frame for as long as any `Table` from this window is used.
    pub const unsafe fn new(offset: usize, phys: core::ops::Range<usize>) -> Window {
        Window { offset, phys_start: phys.start, phys_end: phys.end }
    }

    /// # Safety
    /// Address translation must be off, so that physical addresses can be used directly.
    pub const unsafe fn identity() -> Window { Window { offset: 0, phys_start: 0, phys_end: usize::MAX } }

    fn frame(&self, phys: usize) -> NonNull<u8> {
        assert!(
            (self.phys_start..self.phys_end).contains(&phys) && phys % PAGE_SIZE == 0,
            "not a reachable frame"
        );
        NonNull::new(self.offset.wrapping_add(phys) as *mut u8).unwrap()
    }

    /// Fill the RAM frame at `phys` with zeroes.
    ///
    /// # Safety
    /// `phys` must be a RAM frame that nothing else is using: a freshly allocated page.
    pub unsafe fn zero_frame(self, phys: usize) {
        // SAFETY: the window reaches every RAM frame (module docs, 1), and the caller
        // guarantees exclusive use of this one.
        unsafe { self.frame(phys).write_bytes(0, PAGE_SIZE) };
    }
}

/// A page-table page.
#[derive(Copy, Clone)]
pub struct Table {
    entries: NonNull<Pte>,
    window: Window,
}

impl Table {
    /// The table in the RAM frame at `phys`.
    ///
    /// # Safety
    /// The frame must hold a page table and keep doing so while this `Table`, or any
    /// table reached from it, is in use (module docs, 2).
    pub unsafe fn at(window: Window, phys: usize) -> Table {
        Table { entries: window.frame(phys).cast(), window }
    }

    /// Turn the freshly allocated RAM frame at `phys` into an empty table.
    ///
    /// # Safety
    /// Nothing else may be using the frame. It is zeroed here.
    pub unsafe fn new_in(window: Window, phys: usize) -> Table {
        // SAFETY: forwarded from the caller. A zeroed frame is a valid, empty page table.
        unsafe {
            window.zero_frame(phys);
            Table::at(window, phys)
        }
    }

    pub fn get(self, index: usize) -> Pte {
        assert!(index < ENTRIES);
        // SAFETY: `entries` points at a live page table of `ENTRIES` entries (invariant of
        // `Table::at`), `index` is in bounds, and accesses do not race (module docs, 3).
        unsafe { self.entries.add(index).read_volatile() }
    }

    fn set(self, index: usize, pte: Pte) {
        assert!(index < ENTRIES);
        // SAFETY: as for `get`.
        unsafe { self.entries.add(index).write_volatile(pte) };
    }

    pub fn slot(self, index: usize) -> Slot { Slot { table: self, index } }

    /// The next-level table that entry `index` points at, if it points at one.
    pub fn child(self, index: usize) -> Option<Table> {
        let pte = self.get(index);
        // SAFETY: a valid non-leaf entry always names a page table (module docs, 2).
        pte.is_table().then(|| unsafe { Table::at(self.window, pte.phys()) })
    }
}

/// One entry of one table: the typed replacement for a `*mut usize` into a page table.
#[derive(Copy, Clone)]
pub struct Slot {
    table: Table,
    index: usize,
}

impl Slot {
    pub fn get(self) -> Pte { self.table.get(self.index) }

    /// Write a leaf, a reservation, or `Pte::EMPTY`. Table pointers go through `install_table`.
    pub fn set(self, pte: Pte) {
        assert!(!pte.is_table(), "use install_table() to link page tables");
        self.table.set(self.index, pte);
    }

    /// Copy an entry verbatim from another root, to share kernel mappings between address spaces.
    pub fn copy_from(self, other: Slot) { self.table.set(self.index, other.get()); }

    /// Point this entry at a new next-level table in the frame at `phys`, and return it. The frame
    /// is zeroed here unless `already_zero` (the kernel's allocator gives only zero frames), so it
    /// is not zeroed twice.
    ///
    /// # Safety
    /// `phys` must be a freshly allocated RAM frame that nothing else uses, and with
    /// `already_zero`, every byte of it zero.
    pub unsafe fn install_table(self, phys: usize, already_zero: bool) -> Table {
        // SAFETY: forwarded from the caller: a frame zeroed here, or zero already, is an empty table.
        let table = unsafe {
            if already_zero {
                Table::at(self.table.window, phys)
            } else {
                Table::new_in(self.table.window, phys)
            }
        };
        self.table.set(self.index, Pte::table(phys));
        table
    }
}

/// Bytes mapped by a leaf in a table at `level` (0 = 4 KiB).
pub const fn leaf_size(level: usize) -> usize { PAGE_SIZE << (VPN_BITS * level) }

/// Index into the table at `level` (root = LEVELS-1) for `virt`.
pub const fn vpn(virt: usize, level: usize) -> usize { (virt >> (12 + VPN_BITS * level)) & (ENTRIES - 1) }

/// Whether `virt` is a usable virtual address. Sv39 requires sign-extension above bit 38;
/// every 32-bit address is valid under Sv32.
#[cfg(target_pointer_width = "64")]
pub fn is_canonical(virt: usize) -> bool {
    let upper = virt >> 38;
    upper == 0 || upper == (1 << 26) - 1
}
#[cfg(target_pointer_width = "32")]
pub fn is_canonical(_virt: usize) -> bool { true }

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUTS: [SatpLayout; 2] = [SV32_SATP, SV39_SATP];

    #[test]
    fn satp_round_trips_on_both_layouts() {
        for layout in LAYOUTS {
            let top_asid = (1 << layout.asid_bits) - 1;
            let top_root = layout.ppn_mask << 12;
            for (root, asid) in [(0x8020_0000, 0), (0x8020_0000, 1), (0x8020_0000, 511), (top_root, top_asid)]
            {
                let satp = layout.make(root, asid);
                assert!(layout.is_active(satp), "{layout:?}");
                assert_eq!(layout.root(satp), root, "{layout:?}");
                assert_eq!(layout.asid(satp), asid, "{layout:?}");
            }
            assert!(!layout.is_active(0));
        }
    }

    #[test]
    fn a_full_asid_leaves_the_root_and_mode_alone() {
        for layout in LAYOUTS {
            let satp = layout.make(0x8020_0000, 0);
            let ones = layout.with_asid_ones(satp);
            assert_eq!(layout.root(ones), 0x8020_0000);
            assert!(layout.is_active(ones));
            assert_eq!(layout.asid(ones), (1 << layout.asid_bits) - 1);
        }
    }

    #[test]
    #[should_panic(expected = "wider than satp's field")]
    fn an_asid_past_the_field_is_refused() { SV32_SATP.make(0x8020_0000, 512); }

    #[test]
    fn the_asid_width_decision() {
        for layout in LAYOUTS {
            let full = layout.asid_bits;
            let satp = layout.make(0x8020_0000, 0);
            let read_back = |width: u32| satp | (((1 << width) - 1) << layout.asid_shift);
            // Every bit stuck.
            assert_eq!(layout.asid_width(layout.with_asid_ones(satp), full), Ok(full), "{layout:?}");
            // Exactly the width needed.
            assert_eq!(layout.asid_width(read_back(9), 9), Ok(9), "{layout:?}");
            // One bit short: refused, naming what the hart has.
            assert_eq!(layout.asid_width(read_back(full - 1), full), Err(full - 1), "{layout:?}");
            // None: a hart without ASIDs.
            assert_eq!(layout.asid_width(satp, full), Err(0), "{layout:?}");
            // A bit that sticks above a gap does not widen the field.
            let gapped = satp | (0b101 << layout.asid_shift);
            assert_eq!(layout.asid_width(gapped, 2), Err(1), "{layout:?}");
        }
    }
}
