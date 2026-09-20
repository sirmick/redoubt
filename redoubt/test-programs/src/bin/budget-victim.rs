//! Victim for the budget attack cases: lives in `system` beside the attacker, holds no handles,
//! and waits for the attacker's go. Then it maps and touches `rd::victim::PAGES` pages, paid by
//! `system`, and reports to `attack-checker`. It gets there only if the attack left `system`
//! alive (a forged handle that destroyed it would have killed this process) and its accounting
//! whole (an over-carve or an uncharged table would leave `system` without free pages, and the
//! first touch beyond them ends this process instead). The verdict is its report under its own
//! PID and the checker's power-off; the attacker can produce neither.

#![no_std]
#![no_main]

use test_programs::rd::victim;
use test_programs::{Logger, checker, log};
use xous::{MemoryFlags, Message};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let sid = xous::create_server_with_address(victim::ADDRESS).expect("couldn't create the victim's server");
    loop {
        let envelope = xous::receive_message(sid).expect("couldn't receive");
        if let Message::BlockingScalar(m) = &envelope.body {
            if m.id == victim::GO {
                xous::return_scalar(envelope.sender, 0).ok();
                break;
            }
        }
    }
    let len = victim::PAGES * 4096;
    let range = xous::map_memory(None, None, len, MemoryFlags::R | MemoryFlags::W).expect("map_memory");
    for page in 0..victim::PAGES {
        // SAFETY: `range` is this process's own fresh mapping of `len` bytes.
        unsafe { (range.as_mut_ptr().add(page * 4096) as *mut u64).write_volatile(page as u64) };
    }
    log!(logger, "[victim] mapped and touched {} pages in system after the attack", victim::PAGES);
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
