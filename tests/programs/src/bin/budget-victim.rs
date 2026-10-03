//! Victim for the budget attack cases: a program the tester starts beside the attacker, in a
//! budget of its own under `system`, holding the budgets its case names (`system`, and `users`
//! where the attacker carves from it) from slot 4. It waits on the boot endpoint for the
//! attacker's go. Then it reads the usage of every budget it holds, its own included, maps and
//! touches `rd::victim::PAGES` pages of its own, and reports to the checker (`log-server`'s
//! `DONE`). It gets there only if the attack left `system` alive (destroying it, through any
//! handle, kills this process with it: R10) and the accounting whole (a carve beyond a parent's
//! limits would show as usage past a limit, I5, and the report says so). The verdict is its report under its
//! own place and the checker's power-off; the attacker can produce neither.

#![no_std]
#![no_main]

use test_programs::rd::{self, MessageKind, Received, victim};
use test_programs::{Logger, checker, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The first program the tester starts: it holds the boot endpoint's receive right.
    loop {
        let Ok(Received::Message(m)) = rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0) else { continue };
        if let MessageKind::Call { .. } = m.kind {
            rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
            if m.body.words[0] == victim::GO {
                break;
            }
        }
    }
    // Its own budget, then the ones it was given, up to the first index that is no budget.
    let within = (rd::OWN..).map_while(|h| rd::usage(h).ok()).all(|u| {
        u.pages_usage <= u.pages_limit
            && u.processes_usage <= u.processes_limit
            && u.weight_carved <= u.weight_limit
    });
    let at = rd::map_anon(victim::PAGES * rd::PAGE_SIZE, rd::rw()).expect("map_anon");
    for page in 0..victim::PAGES {
        rd::poke(at + page * rd::PAGE_SIZE, page as u64);
    }
    log!(
        logger,
        "[victim] mapped and touched {} pages after the attack; every budget it holds within its limits: {}",
        victim::PAGES,
        within
    );
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
