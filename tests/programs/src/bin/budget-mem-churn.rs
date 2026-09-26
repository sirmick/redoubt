//! Memory churn against the ledger (R6, I5): 64 rounds of map, touch and unmap, and 64 lends to
//! this program's own log server thread and back (`logsrv::start_serving`), with `system`'s page usage read
//! after each and required to be exactly where it started. A frame charged and not uncharged (or the reverse)
//! on any of the legacy paths shows as drift. Must run as the loader's first program, which holds `system`.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Page, log, logsrv, op, rd};

const ROUNDS: usize = 64;
const PAGES: usize = 4;

fn map_touch_unmap() {
    let at = rd::map_anon(PAGES * rd::PAGE_SIZE, rd::rw()).expect("map");
    for page in 0..PAGES {
        rd::poke(at + page * rd::PAGE_SIZE, page as u64 + 1);
    }
    rd::unmap(at, PAGES * rd::PAGE_SIZE).expect("unmap");
}

fn lend(server: u32, page: &mut Page) {
    page.clear();
    page.write_str("churn").ok();
    let body = rd::body([op::UPPERCASE, page.bytes().len(), 0, 0]);
    rd::call_waiting(server, &body, page.pages(), rd::FOREVER).expect("lend");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();
    logsrv::start_serving();
    let server = logsrv::mint_child(logsrv::CHILD_BADGES).expect("a send on its own log endpoint");
    let mut page = Page::new();
    // Warm up: every page table and stack page these paths need, in this process and in its
    // server thread, exists before the first reading.
    log!(logger, "[churn] warming up");
    map_touch_unmap();
    lend(server, &mut page);
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
        lend(server, &mut page);
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
