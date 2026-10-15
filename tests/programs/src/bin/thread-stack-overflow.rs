//! `thread-stack-overflow`: the child of `tests/thread-stack-guard.toml`, launched by its tester
//! through the stub, on the runtime. It tells the tester it is about to spawn, then spawns a
//! helper thread on a two-page stack that recurses without end: each frame stores below the last
//! until a store lands in the unmapped guard page the runtime placed under the stack
//! (userland/native.md, "redoubt-rt", `thread`), and the kernel ends the process there. The main
//! thread waits for a message nobody sends. Its words are only traces: the case's verdicts are the
//! kernel's, its fault line naming the guard page and the exit notice the tester reads.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;

use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// What it tells the tester, word 0.
const SPAWNING: u64 = 1;
/// The helper's stack: small, so the overflow is quick, and whole pages above a guard page.
const HELPER_PAGES: usize = 2;

/// Exit codes: what went wrong. Any of them ends the process `Exited`, which the tester refuses.
const NO_HANDLES: u32 = 2;
const NO_TESTER: u32 = 3;
const NO_THREAD: u32 = 4;
const SURVIVED: u32 = 5;

/// Recurses without end, each frame a store below the last, so the first store under the stack's
/// lowest page is in the guard page. The array is forced into the frame and the sum keeps the call
/// from being a jump.
#[allow(unconditional_recursion)]
#[inline(never)]
fn descend(depth: usize) -> usize {
    let mut frame = [depth; 8];
    core::hint::black_box(&mut frame);
    descend(depth + 1) + frame[depth % 8]
}

fn run(startup: &Startup) -> u32 {
    let Some(tester) = startup.handle("tester") else { return NO_HANDLES };
    let tester = Endpoint::from_handle(tester);
    let Ok(parked) = Endpoint::create() else { return NO_HANDLES };
    if tester.call(&[SPAWNING, 0, 0, 0], &[], None, FOREVER).into_result().is_err() {
        return NO_TESTER;
    }
    let helper = Box::new(|| {
        core::hint::black_box(descend(0));
    });
    if redoubt_rt::thread::spawn(helper, HELPER_PAGES).is_err() {
        return NO_THREAD;
    }
    // The helper's fault ends the process while this thread waits here, for a message nobody
    // sends; a message, or an error, would mean the process outlived its helper's overflow.
    let _ = parked.receive(FOREVER, 0);
    SURVIVED
}
