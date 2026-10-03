//! The heap: a simple allocator over `map_anon`, obviously correct rather than fast (TENETS.md:
//! "Not fast", and tenet 1).
//!
//! - **Small blocks** (at most [`MAX_SMALL`] bytes and alignment): eight size classes, the powers of two from
//!   16 to 2048. A class's free blocks form a singly linked list threaded through the blocks themselves. An
//!   empty class maps one page and splits it into blocks. A block's size divides the page size and pages are
//!   page-aligned, so every block is aligned to its size, which is at least the requested alignment. Freed
//!   small blocks go back on their list; their pages are never returned to the kernel.
//! - **Large blocks**: whole pages from `map_anon`, unmapped when freed. Alignment above a page is refused
//!   (null), since `map_anon` promises only page alignment.
//! - **A fixed arena** ([`Heap::fix`]): a program that must bound its memory up front (`init`,
//!   kernel/budgets.md, "The tree from the boot manifest") maps one region once, and from then on every page,
//!   small classes' and large blocks', comes from it and never from `map_anon`. Freed large blocks go on a
//!   first-fit list of page runs instead of being unmapped; when the region is spent, an allocation fails
//!   (null) like an exhausted `map_anon`.
//!
//! One spin lock guards the lists. Contention costs spinning, never correctness.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use redoubt_sys::{Error, MemFlags, PAGE_SIZE};

use crate::handle::{map_anon, unmap};

/// The largest small block.
pub const MAX_SMALL: usize = 2048;
const MIN_SMALL: usize = 16;
// `words` and `set_words` touch two words at a free block's start: the smallest class holds them,
// aligned for them, on both widths (rt-build compiles rv32 and rv64).
const _: () = assert!(MIN_SMALL >= size_of::<[usize; 2]>() && MIN_SMALL % align_of::<[usize; 2]>() == 0);
const CLASSES: usize = 8; // 16, 32, ..., 2048

/// The allocator. On the machine it is the global allocator; tests make their own.
pub struct Heap {
    lock: AtomicBool,
    /// The first free block of each class; 0 = none. Changed only under `lock`.
    free: [AtomicUsize; CLASSES],
    /// The fixed arena's unused tail, `next..end`; `end` is 0 until [`Heap::fix`]. Changed only
    /// under `lock`.
    next: AtomicUsize,
    end: AtomicUsize,
    /// The arena's first freed run of pages; 0 = none. Each run holds its length and the next run
    /// at its start. Changed only under `lock`.
    runs: AtomicUsize,
}

impl Default for Heap {
    fn default() -> Self { Heap::new() }
}

/// The size class for `layout`, or `None` if it is large.
fn class(layout: Layout) -> Option<usize> {
    let size = layout.size().max(layout.align()).max(MIN_SMALL).checked_next_power_of_two()?;
    if size > MAX_SMALL {
        return None;
    }
    Some((size.trailing_zeros() - MIN_SMALL.trailing_zeros()) as usize)
}

fn class_size(class: usize) -> usize { MIN_SMALL << class }

/// A large block's length: its size rounded up to whole pages.
fn large_len(layout: Layout) -> Option<usize> { layout.size().checked_next_multiple_of(PAGE_SIZE) }

/// Holds the heap's lock until dropped.
struct Locked<'a>(&'a Heap);

impl Drop for Locked<'_> {
    fn drop(&mut self) { self.0.lock.store(false, Ordering::Release) }
}

impl Heap {
    pub const fn new() -> Heap {
        Heap {
            lock: AtomicBool::new(false),
            free: [const { AtomicUsize::new(0) }; CLASSES],
            next: AtomicUsize::new(0),
            end: AtomicUsize::new(0),
            runs: AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> Locked<'_> {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        Locked(self)
    }

    /// Maps one region of `pages` pages and takes every later page from it, never from `map_anon`
    /// again. Blocks allocated before keep working. Once only: a second call is `InvalidArgument`.
    pub fn fix(&self, pages: usize) -> Result<(), Error> {
        let _locked = self.lock();
        let len = pages.checked_mul(PAGE_SIZE).filter(|len| *len != 0).ok_or(Error::InvalidArgument)?;
        if self.end.load(Ordering::Relaxed) != 0 {
            return Err(Error::InvalidArgument);
        }
        let base = map_anon(len, MemFlags::READ | MemFlags::WRITE)?;
        self.next.store(base, Ordering::Relaxed);
        self.end.store(base + len, Ordering::Relaxed);
        Ok(())
    }

    /// `len` bytes of fresh pages (a multiple of the page size): the arena's, once fixed, first
    /// from a freed run and then from its tail; else `map_anon`'s. Needs the lock. 0 if memory is
    /// exhausted.
    fn pages(&self, locked: &Locked, len: usize) -> usize {
        let end = self.end.load(Ordering::Relaxed);
        if end == 0 {
            return map_anon(len, MemFlags::READ | MemFlags::WRITE).unwrap_or(0);
        }
        // First fit: `prev` is the run before `run`, 0 for the list's head.
        let (mut prev, mut run) = (0, self.runs.load(Ordering::Relaxed));
        while run != 0 {
            let [run_len, next] = self.words(locked, run);
            if run_len >= len {
                let rest = if run_len == len {
                    next
                } else {
                    self.set_words(locked, run + len, [run_len - len, next]);
                    run + len
                };
                if prev == 0 {
                    self.runs.store(rest, Ordering::Relaxed);
                } else {
                    let [prev_len, _] = self.words(locked, prev);
                    self.set_words(locked, prev, [prev_len, rest]);
                }
                return run;
            }
            (prev, run) = (run, next);
        }
        let next = self.next.load(Ordering::Relaxed);
        if end - next < len {
            return 0;
        }
        self.next.store(next + len, Ordering::Relaxed);
        next
    }

    /// Gives back a large block's `len` bytes at `addr`: to the arena's runs once fixed, else to
    /// the kernel. Needs the lock.
    fn free_pages(&self, locked: &Locked, addr: usize, len: usize) {
        if self.end.load(Ordering::Relaxed) == 0 {
            // The kernel refuses to unmap what is not ours, so a failure here would be a bug in
            // the caller, which GlobalAlloc's contract rules out; there is nothing to report to.
            let _ = unmap(addr, len);
            return;
        }
        self.set_words(locked, addr, [len, self.runs.load(Ordering::Relaxed)]);
        self.runs.store(addr, Ordering::Relaxed);
    }

    /// Puts the free block at `addr` on its class's list. Needs the lock.
    fn push(&self, locked: &Locked, class: usize, addr: usize) {
        let head = &self.free[class];
        self.set_words(locked, addr, [head.load(Ordering::Relaxed), 0]);
        head.store(addr, Ordering::Relaxed);
    }

    /// The first two words of the free block or run at `addr`, as `set_words` wrote them. Needs
    /// the lock.
    fn words(&self, _: &Locked, addr: usize) -> [usize; 2] {
        // SAFETY: as for `set_words`, which wrote these two words; nothing has written them since.
        unsafe { (addr as *const [usize; 2]).read() }
    }

    /// Writes the first two words of the free block or run at `addr`: a class's next block, or a
    /// run's length and next run. Needs the lock.
    fn set_words(&self, _: &Locked, addr: usize, words: [usize; 2]) {
        // SAFETY: `addr` starts a block in no live allocation (fresh, freed, or on a list), at
        // least the smallest class, 16 bytes, aligned to its size (a run is page-aligned), so two
        // words are in bounds and aligned on both widths (asserted beside `MIN_SMALL`). Who
        // guarantees it: the kernel mapped the page read-write (`map_anon`), this heap does not
        // unmap it while the block is free, and `Locked` keeps every other thread off it.
        unsafe { (addr as *mut [usize; 2]).write(words) };
    }

    /// Takes a free block of `class`, mapping a page for the class if it has none. Needs the
    /// lock. 0 if memory is exhausted.
    fn pop(&self, locked: &Locked, class: usize) -> usize {
        let head = &self.free[class];
        if head.load(Ordering::Relaxed) == 0 {
            let page = self.pages(locked, PAGE_SIZE);
            if page == 0 {
                return 0;
            }
            for addr in (page..page + PAGE_SIZE).step_by(class_size(class)).rev() {
                self.push(locked, class, addr);
            }
        }
        let addr = head.load(Ordering::Relaxed);
        let [next, _] = self.words(locked, addr);
        head.store(next, Ordering::Relaxed);
        addr
    }
}

// SAFETY: `alloc` returns null or a block of at least `layout.size()` bytes aligned to
// `layout.align()` (see the module docs) that no other live allocation overlaps: small blocks are
// on exactly one free list until taken, and large ones are fresh mappings, or a fixed arena's
// pages on no run list and below its tail. Who guarantees it: the heap's own lists, changed only
// under `Locked`, over pages the kernel mapped (`map_anon`); and the caller, by GlobalAlloc's
// contract, for `dealloc`'s `layout`, from which it gets back the class or page count.
unsafe impl GlobalAlloc for Heap {
    // SAFETY: GlobalAlloc's contract (a non-zero size); see the impl's comment for what it returns.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class(layout) {
            Some(class) => self.pop(&self.lock(), class) as *mut u8,
            None if layout.align() > PAGE_SIZE => core::ptr::null_mut(),
            None => large_len(layout).map_or(0, |len| self.pages(&self.lock(), len)) as *mut u8,
        }
    }

    // SAFETY: GlobalAlloc's contract: `ptr` came from `alloc` on this heap with this `layout`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match class(layout) {
            Some(class) => self.push(&self.lock(), class, ptr as usize),
            None => {
                if let Some(len) = large_len(layout) {
                    self.free_pages(&self.lock(), ptr as usize, len);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        let l = |size, align| Layout::from_size_align(size, align).unwrap();
        assert_eq!(class(l(1, 1)), Some(0));
        assert_eq!(class(l(16, 1)), Some(0));
        assert_eq!(class(l(17, 1)), Some(1));
        assert_eq!(class(l(8, 64)), Some(2));
        assert_eq!(class(l(2048, 8)), Some(7));
        assert_eq!(class(l(2049, 8)), None);
        assert_eq!(class(l(1, 4096)), None);
        assert_eq!(class(l(usize::MAX / 2, 1)), None);
        assert_eq!(large_len(l(1, 1)), Some(PAGE_SIZE));
        assert_eq!(large_len(l(PAGE_SIZE + 1, 1)), Some(2 * PAGE_SIZE));
        for c in 0..CLASSES {
            assert_eq!(PAGE_SIZE % class_size(c), 0);
            assert_eq!(class(l(class_size(c), 1)), Some(c));
        }
    }
}
