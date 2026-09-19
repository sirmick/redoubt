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
//!
//! One spin lock guards the lists. Contention costs spinning, never correctness.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use redoubt_sys::{MemFlags, PAGE_SIZE};

use crate::handle::{map_anon, unmap};

/// The largest small block.
pub const MAX_SMALL: usize = 2048;
const MIN_SMALL: usize = 16;
const CLASSES: usize = 8; // 16, 32, ..., 2048

/// The allocator. On the machine it is the global allocator; tests make their own.
pub struct Heap {
    lock: AtomicBool,
    /// The first free block of each class; 0 = none. Changed only under `lock`.
    free: [AtomicUsize; CLASSES],
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
        Heap { lock: AtomicBool::new(false), free: [const { AtomicUsize::new(0) }; CLASSES] }
    }

    fn lock(&self) -> Locked<'_> {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        Locked(self)
    }

    /// Puts the free block at `addr` on its class's list. Needs the lock. Private, and called
    /// only by `pop` (a fresh page's blocks) and `dealloc` (a block of this class being freed),
    /// which is what makes the write below sound.
    fn push(&self, _: &Locked, class: usize, addr: usize) {
        let head = &self.free[class];
        // SAFETY: `addr` is a block of `class_size(class)` (at least 16) bytes, aligned to that
        // size, in a page this heap mapped read-write and never unmaps, and no longer in use (it
        // is fresh from a new page in `pop` or was freed by `dealloc`); so writing one `usize` at its start
        // is in bounds, aligned, and aliases nothing live.
        unsafe { (addr as *mut usize).write(head.load(Ordering::Relaxed)) };
        head.store(addr, Ordering::Relaxed);
    }

    /// Takes a free block of `class`, mapping a page for the class if it has none. Needs the
    /// lock. 0 if memory is exhausted.
    fn pop(&self, locked: &Locked, class: usize) -> usize {
        let head = &self.free[class];
        if head.load(Ordering::Relaxed) == 0 {
            let Ok(page) = map_anon(PAGE_SIZE, MemFlags::READ | MemFlags::WRITE) else { return 0 };
            for addr in (page..page + PAGE_SIZE).step_by(class_size(class)).rev() {
                self.push(locked, class, addr);
            }
        }
        let addr = head.load(Ordering::Relaxed);
        // SAFETY: `addr` is the head of the list, so `push` wrote a `usize` link at its start
        // (see there) and nothing has written to the block since.
        let next = unsafe { (addr as *const usize).read() };
        head.store(next, Ordering::Relaxed);
        addr
    }
}

// SAFETY: `alloc` returns null or a block of at least `layout.size()` bytes aligned to
// `layout.align()` (see the module docs) that no other live allocation overlaps: small blocks are
// on exactly one free list until taken, and large ones are fresh mappings. `dealloc` gets back
// the class or page count from the same `layout`, as GlobalAlloc's contract guarantees.
unsafe impl GlobalAlloc for Heap {
    // SAFETY: GlobalAlloc's contract (a non-zero size); see the impl's comment for what it returns.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match class(layout) {
            Some(class) => self.pop(&self.lock(), class) as *mut u8,
            None if layout.align() > PAGE_SIZE => core::ptr::null_mut(),
            None => large_len(layout)
                .and_then(|len| map_anon(len, MemFlags::READ | MemFlags::WRITE).ok())
                .map_or(core::ptr::null_mut(), |addr| addr as *mut u8),
        }
    }

    // SAFETY: GlobalAlloc's contract: `ptr` came from `alloc` on this heap with this `layout`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match class(layout) {
            Some(class) => self.push(&self.lock(), class, ptr as usize),
            // The kernel refuses to unmap what is not ours, so a failure here would be a bug in
            // the caller, which GlobalAlloc's contract rules out; there is nothing to report to.
            None => {
                if let Some(len) = large_len(layout) {
                    let _ = unmap(ptr as usize, len);
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
