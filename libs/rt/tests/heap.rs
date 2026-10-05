//! The heap against the fake kernel's `map_anon`: its free lists and its fixed arena's runs,
//! which keep their links in the free blocks themselves; its cap, which refuses before the kernel
//! is asked; and the record the bench reads. Fast enough for Miri (`rt-miri`).

use std::alloc::{GlobalAlloc, Layout};

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, PAGE_SIZE};
use redoubt_rt::heap::{Heap, RECORD_MAGIC, Record};

#[test]
fn heap_over_map_anon() {
    let f = fake();
    let pid = f.process(0, &[]);
    f.as_process(pid, || {
        let heap = Heap::new();
        let mut live: Vec<(*mut u8, Layout, u8)> = Vec::new();
        let mut x = 0x9e37_79b9_u64;
        // 1,000 rounds under Miri, which checks every access: every size and alignment is taken
        // and freed many times, and the file stays under a minute in `rt-miri`.
        let rounds = if cfg!(miri) { 1_000 } else { 20_000u32 };
        for round in 0..rounds {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if !x.is_multiple_of(3) || live.is_empty() {
                let size = [1, 8, 16, 17, 100, 2048, 2049, 5000, 3 * PAGE_SIZE][(x % 9) as usize];
                let align = [1, 8, 64, 4096][((x >> 8) % 4) as usize];
                let layout = Layout::from_size_align(size, align).unwrap();
                // SAFETY: the layout has a non-zero size.
                let ptr = unsafe { heap.alloc(layout) };
                assert!(!ptr.is_null());
                assert_eq!(ptr as usize % align, 0, "{layout:?}");
                let tag = round as u8;
                // SAFETY: the heap returned `size` writable bytes at `ptr`.
                unsafe { ptr.write_bytes(tag, size) };
                live.push((ptr, layout, tag));
            } else {
                let (ptr, layout, tag) = live.swap_remove((x as usize >> 3) % live.len());
                // SAFETY: `ptr` is live, from this heap, with this layout; no block overlapped it,
                // so its bytes are still the tag written at allocation.
                unsafe {
                    assert!(std::slice::from_raw_parts(ptr, layout.size()).iter().all(|b| *b == tag));
                    heap.dealloc(ptr, layout);
                }
            }
        }
        // Large blocks go back to the kernel; small ones stay on their lists.
        for (ptr, layout, _) in live.drain(..) {
            // SAFETY: as above.
            unsafe { heap.dealloc(ptr, layout) };
        }
        let small_pages = f.held(pid).1;
        let big = Layout::from_size_align(5 * PAGE_SIZE, 8).unwrap();
        // SAFETY: non-zero size; freed with the same layout.
        unsafe {
            let p = heap.alloc(big);
            assert_eq!(f.held(pid).1, small_pages + 5);
            heap.dealloc(p, big);
        }
        assert_eq!(f.held(pid).1, small_pages);
        // Alignment above a page cannot be had from map_anon.
        // SAFETY: non-zero size.
        assert!(unsafe { heap.alloc(Layout::from_size_align(8, 2 * PAGE_SIZE).unwrap()) }.is_null());
    });
}

#[test]
fn heap_in_a_fixed_arena() {
    let f = fake();
    let pid = f.process(0, &[]);
    f.as_process(pid, || {
        let heap = Heap::new();
        let before = f.held(pid).1;
        heap.fix(8).unwrap();
        assert_eq!(f.held(pid).1, before + 8, "the arena is mapped once, whole");
        assert_eq!(heap.fix(8), Err(Error::InvalidArgument), "and only once");
        let pages = |n| Layout::from_size_align(n * PAGE_SIZE, 8).unwrap();
        let small = Layout::from_size_align(16, 8).unwrap();
        // SAFETY: every layout has a non-zero size, and each block is freed with its layout.
        unsafe {
            // A small class's page and two large blocks, all from the arena's tail.
            let s = heap.alloc(small);
            let a = heap.alloc(pages(3));
            let b = heap.alloc(pages(2));
            assert_eq!((a as usize - s as usize, b as usize - a as usize), (PAGE_SIZE, 3 * PAGE_SIZE));
            // A freed run is reused first fit, and split.
            heap.dealloc(a, pages(3));
            assert_eq!(heap.alloc(pages(2)), a);
            assert_eq!(heap.alloc(pages(1)) as usize, a as usize + 2 * PAGE_SIZE);
            // The two pages left, then nothing: never another map_anon.
            assert_eq!(heap.alloc(pages(2)) as usize, b as usize + 2 * PAGE_SIZE);
            assert!(heap.alloc(pages(1)).is_null());
            heap.dealloc(b, pages(2));
            assert_eq!(heap.alloc(pages(3)), core::ptr::null_mut(), "a run shorter than asked is skipped");
            assert_eq!(heap.alloc(small), s.add(16), "small blocks still come from their page");
            assert_eq!(heap.alloc(Layout::from_size_align(64, 64).unwrap()), b, "a class page from a run");
        }
        assert_eq!(f.held(pid).1, before + 8);
    });
}

#[test]
fn capped_heap_refuses_before_the_kernel() {
    let f = fake();
    let pid = f.process(0, &[]);
    f.as_process(pid, || {
        let heap = Heap::new();
        assert_eq!(heap.start(Some(0), 1), Err(Error::InvalidArgument), "no cap of 0");
        heap.start(Some(4), 1).unwrap();
        assert_eq!(heap.start(Some(8), 1), Err(Error::InvalidArgument), "once only");
        assert_eq!(heap.fix(8), Err(Error::InvalidArgument), "a capped heap is never fixed");
        let before = f.held(pid).1;
        let pages = |n| Layout::from_size_align(n * PAGE_SIZE, 8).unwrap();
        let small = Layout::from_size_align(16, 8).unwrap();
        // SAFETY: every layout has a non-zero size, and each block is freed with its layout.
        unsafe {
            let s = heap.alloc(small);
            let a = heap.alloc(pages(2));
            assert!(!s.is_null() && !a.is_null());
            assert_eq!(f.held(pid).1, before + 3);
            // Two more pages would hold five: refused, and the kernel was not asked.
            assert!(heap.alloc(pages(2)).is_null());
            assert_eq!(f.held(pid).1, before + 3);
            // A freed large block's pages count no longer.
            heap.dealloc(a, pages(2));
            let b = heap.alloc(pages(3));
            assert!(!b.is_null());
            assert_eq!(f.held(pid).1, before + 4);
            // Full: even a small class's page is refused.
            assert!(heap.alloc(Layout::from_size_align(64, 8).unwrap()).is_null());
            heap.dealloc(b, pages(3));
        }
        let record = heap.record();
        assert_eq!((record.get(Record::CAP), record.get(Record::PEAK)), (4, 4));
    });
}

#[test]
fn a_fixed_heap_is_never_capped() {
    let f = fake();
    let pid = f.process(0, &[]);
    f.as_process(pid, || {
        let heap = Heap::new();
        heap.fix(2).unwrap();
        assert_eq!(heap.start(Some(8), 1), Err(Error::InvalidArgument));
        assert_eq!(heap.record().get(Record::MAGIC), 0, "a refused start marks nothing");
        heap.start(None, 1).unwrap();
    });
}

#[test]
fn the_record_is_zero_until_marked() {
    let f = fake();
    let pid = f.process(0, &[]);
    f.as_process(pid, || {
        let heap = Heap::new();
        let words = |heap: &Heap| {
            [Record::MAGIC, Record::TAG, Record::CAP, Record::PEAK].map(|i| heap.record().get(i))
        };
        assert_eq!(words(&heap), [0; 4]);
        assert_eq!(size_of::<Record>(), 32);
        assert_eq!(align_of::<Record>(), 32);
        heap.start(None, 7).unwrap();
        // SAFETY: the layout has a non-zero size; freed with it.
        unsafe {
            let big = Layout::from_size_align(3 * PAGE_SIZE, 8).unwrap();
            let p = heap.alloc(big);
            heap.dealloc(p, big);
            let q = heap.alloc(Layout::from_size_align(PAGE_SIZE, 8).unwrap());
            heap.dealloc(q, Layout::from_size_align(PAGE_SIZE, 8).unwrap());
        }
        // Uncapped: the peak is the most held at once, not the sum.
        assert_eq!(words(&heap), [RECORD_MAGIC, 7, 0, 3]);
        // The bench reads the bytes: little-endian words.
        // SAFETY: a Record is 32 bytes of plain integers.
        let bytes = unsafe { std::slice::from_raw_parts(heap.record() as *const Record as *const u8, 32) };
        assert_eq!(bytes[..8], RECORD_MAGIC.to_le_bytes());
        assert_eq!(bytes[8..16], 7u64.to_le_bytes());
    });
}
