//! The victim and judge of the memory attack (`redoubt/tests/mem-attack.toml`). It fills pages
//! with a secret and frees them, then checks every page the attacker lends it: each must be
//! all zero. The verdict is this program's line, which the attacker cannot print (log-server
//! marks every line with its writer's PID; README "Writing an attack case").

#![no_std]
#![no_main]

use test_programs::{log, mem, Logger};
use xous::{MemoryFlags, Message};


#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    for _ in 0..mem::SECRET_PAGES {
        let page = xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W).expect("couldn't map a page");
        // SAFETY: the kernel just mapped this page, writable, for this process alone.
        let bytes = unsafe { core::slice::from_raw_parts_mut(page.as_mut_ptr(), page.len()) };
        for chunk in bytes.chunks_mut(mem::SECRET.len()) {
            chunk.copy_from_slice(mem::SECRET);
        }
        xous::unmap_memory(page).expect("couldn't free a page");
    }
    log!(logger, "[mem-victim] left the secret in {} freed pages", mem::SECRET_PAGES);

    // The server exists only now, so the attacker's first request comes after the pages are freed.
    let sid = xous::create_server_with_address(mem::VICTIM_ADDRESS).expect("couldn't create the victim's server");
    let (mut checked, mut dirty) = (0, 0);
    loop {
        let envelope = xous::receive_message(sid).expect("couldn't receive");
        // The kernel names the sender; the attacker cannot choose it.
        let sender = envelope.sender.pid().map_or(0, |pid| pid.get());
        match &envelope.body {
            Message::Borrow(m) if m.id == mem::CHECK => {
                // SAFETY: the kernel lends us this range, readable, until we drop the envelope.
                let bytes = unsafe { core::slice::from_raw_parts(m.buf.as_ptr(), m.buf.len()) };
                checked += 1;
                if bytes.iter().any(|&b| b != 0) {
                    dirty += 1;
                    let secret = bytes.windows(mem::SECRET.len()).any(|w| w == mem::SECRET);
                    log!(logger, "[mem-victim] BREACH: a page from PID {} held data (my secret: {})", sender, secret);
                }
            }
            Message::BlockingScalar(m) if m.id == mem::DONE => {
                if dirty == 0 {
                    log!(logger, "[mem-victim] checked {} pages from PID {}: all zero", checked, sender);
                } else {
                    log!(logger, "[mem-victim] BREACH: {} of {} pages from PID {} held data", dirty, checked, sender);
                }
                xous::return_scalar(envelope.sender, 0).ok();
                if dirty == 0 {
                    // Off the console too: the checker names this PID and powers off, so a
                    // forged verdict line alone cannot pass the case.
                    test_programs::checker::done();
                }
            }
            _ => {}
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
