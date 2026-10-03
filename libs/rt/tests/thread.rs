//! `thread::spawn`: a closure runs on a new thread of the same process; one the kernel refuses to
//! start is dropped without running, with its stack, and one whose start cannot be read is left
//! alone, with its stack.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::Error;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::thread::spawn;

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
