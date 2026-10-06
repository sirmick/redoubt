//! `heap-capped`: the child of `tests/heap-cap.toml`, launched with a heap cap in its startup
//! block. It takes whole pages from its heap, one fallible block at a time, until the runtime
//! refuses one, and tells its tester (`tester`) how many it got; once answered it asks fallibly
//! for `PAST` pages more and says whether that was refused. Then it asks for them infallibly, as
//! most of a server's allocations are: the runtime refuses again, Rust's allocation-failure handler
//! panics, and the runtime exits with its panic code. Its words are only traces: the case's
//! verdicts are the kernel's, its account of the budget and the exit notice, which the tester reads.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_rt::abi::{FOREVER, PAGE_SIZE};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// What it tells the tester, word 0: at its cap (word 1 the pages it got), then refused (word 1
/// the pages asked for, word 2 1 if the runtime refused them).
const AT_CAP: u64 = 1;
const REFUSED: u64 = 2;
/// Pages asked for past the cap, in one block: well inside its budget's room.
const PAST: usize = 64;
/// More one-page blocks than any cap the case gives.
const MOST: usize = 64;

/// Exit codes: what went wrong.
const NO_HANDLES: u32 = 2;
const NO_TESTER: u32 = 3;
const NEVER_REFUSED: u32 = 4;
const INFALLIBLE_GRANTED: u32 = 5;

fn run(startup: &Startup) -> u32 {
    let Some(tester) = startup.handle("tester") else { return NO_HANDLES };
    let tester = Endpoint::from_handle(tester);
    // Held in a fixed array, so nothing but the blocks themselves comes from the heap here.
    let mut blocks: [Option<Vec<u8>>; MOST] = [const { None }; MOST];
    let mut got = 0;
    for block in &mut blocks {
        let mut page = Vec::new();
        if page.try_reserve_exact(PAGE_SIZE).is_err() {
            break;
        }
        *block = Some(page);
        got += 1;
    }
    if got == MOST {
        return NEVER_REFUSED;
    }
    let told = |words: [u64; 4]| tester.call(&words, &[], None, FOREVER).into_result().is_ok();
    if !told([AT_CAP, got as u64, 0, 0]) {
        return NO_TESTER;
    }
    let mut past: Vec<u8> = Vec::new();
    let refused = past.try_reserve_exact(PAST * PAGE_SIZE).is_err();
    if !told([REFUSED, PAST as u64, u64::from(refused), 0]) {
        return NO_TESTER;
    }
    // Infallible: the refusal ends the process here, with `redoubt_rt::exit::PANIC`.
    let past: Vec<u8> = Vec::with_capacity(PAST * PAGE_SIZE);
    core::hint::black_box(&past);
    drop(blocks);
    INFALLIBLE_GRANTED
}
