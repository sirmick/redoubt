//! The backstop behind `ipd`'s heap term (servers/ipd.md, "Sizing"): a socket whose buffers the
//! heap cannot hold is refused `too_many` and noted, never a panic, and a `listen` that cannot make
//! its backlog changes nothing. This test's allocator refuses a socket buffer's size on demand.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use redoubt_ipd::stack::{BUFFER, CtlError};

/// While set, an allocation of a socket buffer's size fails.
static SHORT: AtomicBool = AtomicBool::new(false);

struct Short;

unsafe impl GlobalAlloc for Short {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == BUFFER && SHORT.load(Ordering::SeqCst) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's contract for `layout` is passed through unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` with this `layout` (a refused one is null and
        // never freed).
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Short = Short;

#[test]
fn a_socket_the_heap_cannot_hold_is_refused_and_noted() {
    let mut w = World::unmetered(64);
    let who = owner(&caller(22, 0, &[]));
    let n = w.nine.fs.stack.allocate(who, 22, 16).unwrap();
    assert!(!w.nine.fs.stack.take_heap_short());

    SHORT.store(true, Ordering::SeqCst);
    assert_eq!(w.nine.fs.stack.allocate(who, 22, 16), Err(CtlError::TooMany));
    // A listen whose backlog cannot be made is refused whole: the socket stays fresh and nothing
    // more is charged.
    let refused = w.nine.fs.stack.listen(who, n, &listen_scope(22, 22), 22, 4, 16);
    SHORT.store(false, Ordering::SeqCst);
    assert_eq!(refused, Err(CtlError::TooMany));
    assert_eq!(w.nine.fs.stack.charged(who.key), 1);
    assert_eq!(w.nine.fs.stack.live(), 1);
    assert!(w.nine.fs.stack.take_heap_short(), "noted for the program to say");
    assert!(!w.nine.fs.stack.take_heap_short(), "once per noting");

    // With the heap back, the same socket listens and a new one is made.
    w.nine.fs.stack.listen(who, n, &listen_scope(22, 22), 22, 4, 16).unwrap();
    assert_eq!(w.nine.fs.stack.charged(who.key), 4);
    assert!(w.nine.fs.stack.allocate(who, 22, 16).is_ok());
}
