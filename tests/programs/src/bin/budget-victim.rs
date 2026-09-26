//! Victim for the budget attack cases: lives in `system` beside the attacker, holds no handles,
//! and waits on the boot endpoint for the attacker's go. Then it maps and touches
//! `rd::victim::PAGES` pages, paid by `system`, and reports to the checker (`log-server`'s
//! `DONE`). It gets there only if the attack left `system`
//! alive (a forged handle that destroyed it would have killed this process) and its accounting
//! whole (an over-carve or an uncharged table would leave `system` without free pages, and the
//! first touch beyond them ends this process instead). The verdict is its report under its own
//! PID and the checker's power-off; the attacker can produce neither.

#![no_std]
#![no_main]

use redoubt_abi::MemoryFlags;
use test_programs::rd::{self, MessageKind, Received, victim};
use test_programs::{Logger, checker, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The bundle's second program: it holds the boot endpoint's receive right.
    loop {
        let Ok(Received::Message(m)) = rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0) else { continue };
        if let MessageKind::Call { .. } = m.kind {
            rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
            if m.body.words[0] == victim::GO {
                break;
            }
        }
    }
    let len = victim::PAGES * 4096;
    let range =
        redoubt_abi::map_memory(None, None, len, MemoryFlags::R | MemoryFlags::W).expect("map_memory");
    for page in 0..victim::PAGES {
        // SAFETY: `range` is this process's own fresh mapping of `len` bytes.
        unsafe { (range.as_mut_ptr().add(page * 4096) as *mut u64).write_volatile(page as u64) };
    }
    log!(logger, "[victim] mapped and touched {} pages in system after the attack", victim::PAGES);
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
