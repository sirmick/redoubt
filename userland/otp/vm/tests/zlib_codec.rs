//! A zlib stream counts its codec's state as its holder's memory (docs/userland/beamlet.md,
//! "Limits inside one VM"): a deflater's inline part and what `miniz_oxide` keeps behind boxes of
//! its own, `bif::DEFLATE_BOXED`, written out from the crate's private constants. This test
//! measures both codecs with an allocator that counts what this thread asks for, so a
//! `miniz_oxide` whose buffers change fails here, not at the heap limit.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use beamlet_vm::bif::DEFLATE_BOXED;
use miniz_oxide::DataFormat;
use miniz_oxide::deflate::core::{CompressorOxide, create_comp_flags_from_zip_params};
use miniz_oxide::inflate::stream::InflateState;

/// The system's allocator, counting the bytes this thread allocates while it is asked to.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every call is passed straight to the system's allocator; the count only reads the
// layout's size.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            let _ = BYTES.try_with(|b| b.set(b.get() + layout.size()));
        }
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as for `alloc`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// What `f` allocates on this thread, and its value.
fn allocated<T>(f: impl FnOnce() -> T) -> (usize, T) {
    BYTES.with(|b| b.set(0));
    COUNTING.with(|c| c.set(true));
    let v = f();
    COUNTING.with(|c| c.set(false));
    (BYTES.with(Cell::get), v)
}

#[test]
fn a_deflaters_boxed_state_is_the_constant() {
    let flags = create_comp_flags_from_zip_params(6, 15, 0);
    let (bytes, compressor) = allocated(|| CompressorOxide::new(flags));
    drop(compressor);
    assert_eq!(bytes, DEFLATE_BOXED);
}

#[test]
fn an_inflater_holds_nothing_beyond_its_own_box() {
    let (bytes, state) = allocated(|| InflateState::new_boxed(DataFormat::Raw));
    drop(state);
    assert_eq!(bytes, core::mem::size_of::<InflateState>());
}
