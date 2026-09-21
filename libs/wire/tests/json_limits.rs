//! The resources a worst-case file costs the JSON parser, measured so that `init` (which
//! parses the boot manifest before anything else runs) can size its heap and stack from
//! them. The bounds are stated in `json.rs`'s module docs; these tests hold them.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use redoubt_wire::json::{self, MAX_DEPTH, MAX_LEN};

/// Counts live and peak heap bytes, per thread, so parallel tests do not disturb it.
struct Counting;

thread_local! {
    static LIVE: AtomicIsize = const { AtomicIsize::new(0) };
    static PEAK: AtomicIsize = const { AtomicIsize::new(0) };
}

// SAFETY: forwards every call to `System` unchanged; the counters only observe sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = LIVE.try_with(|live| {
            // Signed: a thread may free what another allocated; only differences matter.
            let size = layout.size() as isize;
            let now = live.fetch_add(size, Ordering::Relaxed) + size;
            let _ = PEAK.try_with(|peak| peak.fetch_max(now, Ordering::Relaxed));
        });
        // SAFETY: the caller's contract for `alloc` is passed on unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = LIVE.try_with(|live| live.fetch_sub(layout.size() as isize, Ordering::Relaxed));
        // SAFETY: the caller's contract for `dealloc` is passed on unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Peak heap bytes above the starting point while parsing `input`.
fn peak_heap(input: &[u8]) -> usize {
    let base = LIVE.with(|l| l.load(Ordering::Relaxed));
    PEAK.with(|p| p.store(base, Ordering::Relaxed));
    let value = json::parse(input).expect("worst cases are valid files");
    let peak = PEAK.with(|p| p.load(Ordering::Relaxed)) - base;
    drop(value);
    usize::try_from(peak).unwrap()
}

/// A file of `MAX_LEN` bytes made of `unit` repeated inside one container.
fn fill(open: &str, unit: &str, close: &str) -> Vec<u8> {
    let n = (MAX_LEN - open.len() - close.len()) / (unit.len() + 1);
    let body = vec![unit; n].join(",");
    format!("{open}{body}{close}").into_bytes()
}

/// Every shape tried here is at or under this many bytes of heap per input byte.
const HEAP_PER_BYTE: usize = 32;

#[test]
fn heap_is_bounded() {
    let mut keys = String::from("{");
    let mut i = 0;
    while keys.len() < MAX_LEN - 16 {
        keys.push_str(&format!("\"{i:x}\":0,"));
        i += 1;
    }
    keys.pop();
    keys.push('}');
    let shapes: [(&str, Vec<u8>); 5] = [
        ("[0,0,...]", fill("[", "0", "]")),
        ("[[],[],...]", fill("[", "[]", "]")),
        ("[{},{},...]", fill("[", "{}", "]")),
        ("[\"\\n\",...] (owned strings)", fill("[", "\"\\n\"", "]")),
        ("{\"0\":0,...}", keys.into_bytes()),
    ];
    for (name, input) in shapes {
        assert!(input.len() <= MAX_LEN);
        let peak = peak_heap(&input);
        println!("{name:28} {:6} bytes in, {:8} peak heap, {:5.1}x", input.len(), peak, peak as f64 / input.len() as f64);
        assert!(peak <= HEAP_PER_BYTE * MAX_LEN, "{name}: {peak} bytes of heap");
    }
}

/// The deepest valid files, on a thread with a small stack.
#[test]
fn stack_is_bounded() {
    let arrays = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
    let objects = r#"{"a":"#.repeat(MAX_DEPTH) + "1" + &"}".repeat(MAX_DEPTH);
    let too_deep = "[".repeat(MAX_LEN);
    for input in [arrays, objects, too_deep] {
        let ok = std::thread::Builder::new()
            .stack_size(STACK)
            .spawn(move || {
                let _ = json::parse(input.as_bytes());
            })
            .unwrap()
            .join();
        assert!(ok.is_ok());
    }
}

/// The stack the deepest file needs: measured at about 56 KiB unoptimised (roughly 1.7 KiB
/// per nesting level) and within 16 KiB optimised, the smallest thread stack Linux gives,
/// so the release bound is an upper bound. Run with --release to check it.
const STACK: usize = if cfg!(debug_assertions) { 64 * 1024 } else { 16 * 1024 };
