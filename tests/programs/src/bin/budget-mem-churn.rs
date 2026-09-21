//! Memory churn against the ledger (R6, I5): 64 rounds of map, touch and unmap, and 64 lends to
//! log-server and back, with `system`'s page usage read after each and required to be exactly
//! where it started. A frame charged and not uncharged (or the reverse) on any of the legacy
//! paths shows as drift. Must run as the loader's first program, which holds `system`.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd;
use test_programs::{Logger, Page, log, op};
use redoubt_abi::{MemoryFlags, Message};

const ROUNDS: usize = 64;
const PAGES: usize = 4;

fn map_touch_unmap() {
    let range = redoubt_abi::map_memory(None, None, PAGES * 4096, MemoryFlags::R | MemoryFlags::W).expect("map");
    for page in 0..PAGES {
        // SAFETY: this program's own fresh mapping.
        unsafe { (range.as_mut_ptr().add(page * 4096) as *mut u64).write_volatile(page as u64 + 1) };
    }
    redoubt_abi::unmap_memory(range).expect("unmap");
}

fn lend(logger: &Logger, page: &mut Page) {
    page.clear();
    page.write_str("churn").ok();
    redoubt_abi::send_message(logger.cid, Message::new_lend_mut(op::UPPERCASE, page.range, None, page.valid()))
        .expect("lend");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let mut page = Page::new();
    // Warm up: every page table and stack page these paths need, in this process and in
    // log-server, exists before the first reading.
    log!(logger, "[churn] warming up");
    map_touch_unmap();
    lend(&logger, &mut page);
    test_programs::wait_ms(20);
    let base = rd::usage(rd::SYSTEM).expect("usage").pages_usage;
    let mut drift = None;
    for round in 0..ROUNDS {
        map_touch_unmap();
        let now = rd::usage(rd::SYSTEM).expect("usage").pages_usage;
        if now != base && drift.is_none() {
            drift = Some(("map", round, now));
        }
    }
    for round in 0..ROUNDS {
        lend(&logger, &mut page);
        let now = rd::usage(rd::SYSTEM).expect("usage").pages_usage;
        if now != base && drift.is_none() {
            drift = Some(("lend", round, now));
        }
    }
    match drift {
        None => {
            log!(logger, "[churn] system usage {} after {} map rounds and {} lends", base, ROUNDS, ROUNDS);
            log!(logger, "CHURN TEST PASSED");
        }
        Some((what, round, now)) => {
            log!(logger, "[churn] {} round {}: system usage {}, was {}", what, round, now, base);
            log!(logger, "CHURN TEST FAILED");
        }
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
