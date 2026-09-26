//! Attack test: anonymous memory (`map_anon`) is zeroed before a process sees it.
//!
//! `mem-victim` has just freed pages holding its secret. This program takes twice as many
//! anonymous pages and lends every one to the victim, untouched. The victim, not this program,
//! says whether any held data (README "Writing an attack case"); this program's own reports are
//! progress only. There is no call that maps RAM by physical address to try.
//!
//! What this shows is that every page handed out is zero when mapped. It does not show that
//! the victim's freed frames were among them: this program chooses what to lend and the kernel
//! chooses which frames to hand out. Reuse of freed frames needs the kernel's knowledge of
//! physical addresses (the model's invariant I9; kernel cases come with WP-K1/K2).

#![no_std]
#![no_main]

use test_programs::{Logger, log, mem, rd};

/// Anonymous pages to take: twice what the victim freed.
const PAGES: usize = 2 * mem::SECRET_PAGES;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // Slot 1: a send on the boot endpoint, whose receive right the victim holds.
    let mut lent = 0;
    for _ in 0..PAGES {
        match rd::map_anon(rd::PAGE_SIZE, rd::rw()) {
            Ok(page) => {
                // Lend it untouched: the kernel backs anonymous pages on first use, and lending
                // one it has never touched must be served, not a kernel panic (WP-K0,
                // lend-untouched-page). The victim sees zeroes either way.
                let body = rd::body([mem::CHECK, 0, 0, 0]);
                rd::call_waiting(rd::BOOT_ENDPOINT, &body, rd::pages(page, 1), rd::FOREVER)
                    .expect("couldn't lend to the victim");
                lent += 1;
            }
            Err(e) => log!(logger, "[mem-attack] anonymous map failed: {:?}", e),
        }
    }
    log!(logger, "[mem-attack] lent {} anonymous pages to the victim", lent);
    rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([mem::DONE, 0, 0, 0]), None, rd::FOREVER)
        .expect("couldn't reach the victim");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
