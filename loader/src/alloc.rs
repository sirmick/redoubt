//! Physical page allocator and the runtime page tracker (RPT).
//!
//! Pages are handed out from the top of RAM downwards. Every allocation is recorded in
//! the RPT, one byte per page holding the owning PID, which is handed to the kernel as
//! its initial allocation table.

use core::ops::Range;

use redoubt_sys::PAGE_SIZE;

pub type Pid = u8;
pub const KERNEL_PID: Pid = 1;

const MAX_RESERVED: usize = 8;

pub struct PageAllocator {
    ram: Range<usize>,
    /// Next candidate page. Everything at or above this address has been considered.
    next: usize,
    /// Ranges that must not be handed out while the loader runs (firmware, the loader
    /// itself, the device tree, the boot bundle).
    reserved: [Range<usize>; MAX_RESERVED],
    reserved_count: usize,
    rpt: &'static mut [Pid],
}

impl PageAllocator {
    pub fn new(ram: Range<usize>) -> Self {
        assert!(ram.start % PAGE_SIZE == 0 && ram.end % PAGE_SIZE == 0);
        PageAllocator {
            next: ram.end,
            ram,
            reserved: core::array::from_fn(|_| 0..0),
            reserved_count: 0,
            rpt: &mut [],
        }
    }

    pub fn ram(&self) -> Range<usize> { self.ram.clone() }

    /// Keep `range` out of the allocator's hands. Must be called before any allocation.
    pub fn reserve(&mut self, range: Range<usize>) {
        assert!(self.next == self.ram.end, "reserve() after alloc()");
        let aligned = (range.start & !(PAGE_SIZE - 1))..range.end.next_multiple_of(PAGE_SIZE);
        self.reserved[self.reserved_count] = aligned;
        self.reserved_count += 1;
    }

    fn overlaps_reserved(&self, range: &Range<usize>) -> Option<&Range<usize>> {
        self.reserved[..self.reserved_count].iter().find(|r| r.start < range.end && range.start < r.end)
    }

    /// Allocate `count` physically contiguous, zeroed pages owned by `owner`.
    pub fn alloc_contiguous(&mut self, count: usize, owner: Pid) -> usize {
        let size = count * PAGE_SIZE;
        let mut end = self.next;
        let start = loop {
            let start = end.checked_sub(size).filter(|s| *s >= self.ram.start).expect("out of memory");
            match self.overlaps_reserved(&(start..end)) {
                Some(reserved) => end = reserved.start,
                None => break start,
            }
        };
        self.next = start;
        // SAFETY: the range lies inside RAM (checked against `ram.start` above), outside every
        // reserved range (firmware, loader, device tree, bundle), and below every earlier
        // allocation, so nothing else is using it. Translation is off, so the physical
        // address is directly usable.
        unsafe { core::ptr::write_bytes(start as *mut u8, 0, size) };
        self.set_owner(start..start + size, owner);
        start
    }

    /// Allocate one zeroed page owned by `owner`.
    pub fn alloc(&mut self, owner: Pid) -> usize { self.alloc_contiguous(1, owner) }

    /// Create the RPT. Allocations made before this point are not tracked, so this must
    /// be the first allocation.
    pub fn init_rpt(&mut self) {
        assert!(self.next == self.ram.end, "the RPT must be the first allocation");
        let pages = self.ram.len() / PAGE_SIZE;
        let base = self.alloc_contiguous(pages.div_ceil(PAGE_SIZE), KERNEL_PID);
        // SAFETY: `base` is a fresh, zeroed allocation of at least `pages` bytes that is
        // never handed out again, so this is the only reference to it for the rest of the
        // loader's life. `Pid` is `u8`, for which all-zeroes is valid.
        self.rpt = unsafe { core::slice::from_raw_parts_mut(base as *mut Pid, pages) };
        let rpt_range = base..base + pages.next_multiple_of(PAGE_SIZE);
        self.set_owner(rpt_range, KERNEL_PID);
    }

    /// Physical address of the RPT, for the kernel.
    pub fn rpt_base(&self) -> usize { self.rpt.as_ptr() as usize }

    /// Permanently assign a physical range (e.g. SBI firmware) to `owner`.
    pub fn set_owner(&mut self, range: Range<usize>, owner: Pid) {
        if self.rpt.is_empty() {
            return;
        }
        let first = (range.start - self.ram.start) / PAGE_SIZE;
        let last = (range.end - self.ram.start).div_ceil(PAGE_SIZE);
        self.rpt[first..last].fill(owner);
    }

    pub fn free_bytes(&self) -> usize { self.rpt.iter().filter(|p| **p == 0).count() * PAGE_SIZE }
}
