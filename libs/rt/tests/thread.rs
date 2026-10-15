//! `thread::spawn`: a closure runs on a new thread of the same process, on a painted stack above an
//! unmapped guard page; one the kernel refuses to start is dropped without running, with its
//! stack, and one whose start cannot be read is left alone, with its stack.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, PAGE_SIZE};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::thread::{STACKS_BASE, STACKS_END, THREAD_PAINT, spawn, thread_paint};

/// The closure runs once, as the process that spawned it (it uses a handle of that process's),
/// and what it captured is dropped once. It never returns: the fake kernel does not model
/// `thread_exit`, so a thread ending is the machine's to show; it drops its captures itself.
#[test]
fn a_spawned_thread_runs_its_closure_as_this_process() {
    let f = fake();
    let pid = f.process(1001, &[]);
    let runs = Arc::new(AtomicUsize::new(0));
    let minted = Arc::new(AtomicBool::new(false));
    let token = Arc::new(());
    let (counted, seen, held) = (Arc::clone(&runs), Arc::clone(&minted), Arc::clone(&token));
    f.as_process(pid, move || {
        let inbox = Endpoint::create().unwrap();
        let body = move || {
            seen.store(inbox.mint(core::num::NonZeroU64::MIN, None).is_ok(), Ordering::SeqCst);
            drop(held);
            counted.fetch_add(1, Ordering::SeqCst);
            loop {
                std::thread::park();
            }
        };
        spawn(Box::new(body), 1).unwrap();
    });
    for _ in 0..2000 {
        if runs.load(Ordering::SeqCst) > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the closure ran once");
    assert!(minted.load(Ordering::SeqCst), "the closure ran as the process that spawned it");
    assert_eq!(Arc::strong_count(&token), 1, "its captures were dropped once");
}

/// A refused start drops the closure, and what it holds, without running it, and its stack.
#[test]
fn a_refused_thread_drops_its_closure_unrun() {
    let f = fake();
    let pid = f.process(1001, &[]);
    let token = Arc::new(());
    let held = Arc::clone(&token);
    f.refuse(pid, "thread_create", Error::OutOfMemory);
    let got = f.as_process(pid, move || spawn(Box::new(move || drop(held)), 1));
    assert_eq!(got, Err(Error::OutOfMemory));
    assert_eq!(Arc::strong_count(&token), 1, "the closure was dropped, not leaked");
    assert_eq!(f.held(pid).1, 0, "its stack was unmapped");
}

/// A start whose result cannot be read may have started the thread: the closure and its stack
/// are left alone, never freed under a thread that may be running on them.
#[test]
fn an_unreadable_start_leaves_its_closure_alone() {
    let f = fake();
    let pid = f.process(1001, &[]);
    let token = Arc::new(());
    let held = Arc::clone(&token);
    f.refuse(pid, "thread_create", Error::InvalidArgument);
    let got = f.as_process(pid, move || spawn(Box::new(move || drop(held)), 1));
    assert_eq!(got, Err(Error::InvalidArgument));
    assert_eq!(Arc::strong_count(&token), 2, "the closure was neither run nor freed");
    assert_eq!(f.held(pid).1, 1, "its stack is still mapped");
}

/// A spawned stack lies in the thread-stack area, mapped by `map_fixed` one page above the slot's
/// start, and the page below it, the guard, is mapped to nothing: an overflow faults there
/// (`thread-stack-guard` on the machine) instead of writing over what lies below.
#[test]
fn a_spawned_stack_sits_above_an_unmapped_guard_page() {
    let f = fake();
    let pid = f.process(1001, &[]);
    // Three pages: no other test of this binary spawns with three, so the stack is found by size.
    f.as_process(pid, || {
        spawn(
            Box::new(|| {
                loop {
                    std::thread::park()
                }
            }),
            3,
        )
    })
    .unwrap();
    let mappings = f.mappings(pid);
    let &(stack, len) = mappings
        .iter()
        .find(|(addr, len)| (STACKS_BASE..STACKS_END).contains(addr) && *len == 3 * PAGE_SIZE)
        .expect("the stack is mapped in the thread-stack area");
    assert_eq!(stack % PAGE_SIZE, 0, "the stack starts on a page");
    let guard = stack - PAGE_SIZE;
    assert!(guard >= STACKS_BASE, "the guard page is in the area too");
    assert!(
        mappings.iter().all(|&(addr, len)| addr >= guard + PAGE_SIZE || addr + len <= guard),
        "the page below the stack is unmapped: {mappings:x?}"
    );
    assert_eq!(f.calls(pid).iter().filter(|c| **c == "map_fixed").count(), 1, "placed by one map_fixed");
    let _ = len;
}

/// Before its thread starts, a spawned stack is painted from the top with the spawn's number and
/// each unit's index, for the bench's memory scan; the thread's frames overwrite the top.
#[test]
fn a_spawned_stack_is_painted_from_the_top_for_the_scan() {
    let f = fake();
    let pid = f.process(1001, &[]);
    // Four pages: unique to this test, as above.
    f.as_process(pid, || {
        spawn(
            Box::new(|| {
                loop {
                    std::thread::park()
                }
            }),
            4,
        )
    })
    .unwrap();
    let &(stack, len) = f
        .mappings(pid)
        .iter()
        .find(|(addr, len)| (STACKS_BASE..STACKS_END).contains(addr) && *len == 4 * PAGE_SIZE)
        .expect("the stack is mapped in the thread-stack area");
    let units = len / 8;
    // The two lowest units, which no frame of a parked thread reaches.
    let unit = |from_top: usize| {
        let at = stack + (units - 1 - from_top) * 8;
        // SAFETY (host test): `at` is inside the stack the fake kernel mapped for this process,
        // which stays mapped for the test's life, and the parked thread does not write this deep.
        unsafe { (at as *const u64).read_volatile() }
    };
    let bottom = unit(units - 1);
    assert_eq!((bottom >> 32) as u32, THREAD_PAINT, "the bottom unit carries the thread paint");
    assert_eq!((bottom >> 24) & 0xff, 0, "the host has no launch tag");
    let thread = ((bottom >> 16) & 0xff) as u8;
    assert_ne!(thread, 0, "the spawn is numbered from 1");
    assert_eq!(bottom, thread_paint(0, thread, (units - 1) as u16), "the bottom unit is the last index");
    assert_eq!(unit(units - 2), thread_paint(0, thread, (units - 2) as u16), "indexed from the top");
}
