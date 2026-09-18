//! Typed RISC-V Sv39 page tables.
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
//! 1. *The window is real.* Whoever constructs a `Window` promises (it is an `unsafe fn`)
//!    that `base + phys` is a valid, writable address for every RAM frame below `size`,
//!    for as long as any `Table` made from it is in use.
//! 2. *A `Table` is always a page table.* One is only created by `Table::at`, whose caller
//!    vouches for the frame (a root named by `satp`), by `Table::child`, which follows a
//!    valid non-leaf entry, or by `Slot::install_table`. Non-leaf entries are written only
//!    by `install_table`, which zeroes the frame first; `Slot::set` refuses them. So every
//!    non-leaf entry names a page table, by induction. Tables must not be freed while an
//!    entry still points at them; that is the caller's obligation and is stated on `at`.
//! 3. *Accesses do not race.* Entries are read and written whole, with volatile accesses
//!    through a raw pointer; no reference to table memory is ever formed. The callers are
//!    single-threaded (the loader) or run with interrupts off on one hart (the kernel).
//!    SMP must put address-space edits under a lock before this stops being true.

#![no_std]

use core::ptr::NonNull;

pub const PAGE_SIZE: usize = 4096;
pub const ENTRIES: usize = 512;
pub const LEVELS: usize = 3;
/// Bytes mapped by one root entry, which is also the size of the largest leaf.
pub const GIGAPAGE: usize = 1 << 30;

bitflags::bitflags! {
    /// The low ten bits of an entry. `S` and `P` are the two bits the architecture leaves
    /// to software; Xous uses them for lent and swapped pages.
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
    base: usize,
    size: usize,
}

impl Window {
    /// # Safety
    /// For every RAM frame `phys < size`, `base + phys` must be a valid, writable address
    /// of that frame for as long as any `Table` created through this window is used.
    pub const unsafe fn offset(base: usize, size: usize) -> Window { Window { base, size } }

    /// # Safety
    /// Address translation must be off, so that physical addresses can be used directly.
    pub const unsafe fn identity() -> Window { Window { base: 0, size: usize::MAX } }

    fn frame(self, phys: usize) -> NonNull<u8> {
        assert!(phys < self.size && phys % PAGE_SIZE == 0 && phys != 0, "not a reachable frame");
        NonNull::new((self.base + phys) as *mut u8).unwrap()
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
    pub unsafe fn at(window: Window, phys: usize) -> Table { Table { entries: window.frame(phys).cast(), window } }

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

    /// Point this entry at a new next-level table in the frame at `phys`, and return it.
    ///
    /// # Safety
    /// `phys` must be a freshly allocated RAM frame that nothing else uses. It is zeroed here.
    pub unsafe fn install_table(self, phys: usize) -> Table {
        // SAFETY: forwarded from the caller.
        let table = unsafe { Table::new_in(self.table.window, phys) };
        self.table.set(self.index, Pte::table(phys));
        table
    }
}

/// Index into the table at `level` (2 = root) for `virt`.
pub const fn vpn(virt: usize, level: usize) -> usize { (virt >> (12 + 9 * level)) & (ENTRIES - 1) }

/// Sv39 addresses are 39 bits, sign-extended.
pub fn is_canonical(virt: usize) -> bool {
    let upper = virt >> 38;
    upper == 0 || upper == (1 << 26) - 1
}
