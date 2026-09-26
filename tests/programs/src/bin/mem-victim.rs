//! The victim and judge of the memory attack (`tests/mem-attack.toml`). It fills pages
//! with a secret and frees them, then checks every page the attacker lends it: each must be
//! all zero. The verdict is this program's line, which the attacker cannot print (log-server
//! marks every line with its writer's PID; README "Writing an attack case").

#![no_std]
#![no_main]

use test_programs::rd::{self, MessageKind, Received};
use test_programs::{Logger, log, mem};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    for _ in 0..mem::SECRET_PAGES {
        let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("couldn't map a page");
        // SAFETY: the kernel just mapped this page, writable, for this process alone.
        let bytes = unsafe { core::slice::from_raw_parts_mut(page as *mut u8, rd::PAGE_SIZE) };
        for chunk in bytes.chunks_mut(mem::SECRET.len()) {
            chunk.copy_from_slice(mem::SECRET);
        }
        rd::unmap(page, rd::PAGE_SIZE).expect("couldn't free a page");
    }
    log!(logger, "[mem-victim] left the secret in {} freed pages", mem::SECRET_PAGES);
    // The bundle's second program: it holds the boot endpoint's receive right. The attacker's
    // calls wait until this `receive`, after the pages are freed.
    let (mut checked, mut dirty) = (0, 0);
    loop {
        let Ok(Received::Message(m)) = rd::receive(Some(rd::BOOT_ENDPOINT), rd::FOREVER, 0) else { continue };
        let MessageKind::Call { lend } = m.kind else { continue };
        // The kernel writes the badge, the sender's PID; the attacker cannot choose it.
        let sender = m.badge;
        match (m.body.words[0], lend) {
            (mem::CHECK, Some(pages)) => {
                let len = pages.npages.get() * rd::PAGE_SIZE;
                // SAFETY: the kernel lends us these pages, readable, until the reply below.
                let bytes = unsafe { core::slice::from_raw_parts(pages.addr as *const u8, len) };
                checked += 1;
                if bytes.iter().any(|&b| b != 0) {
                    dirty += 1;
                    let secret = bytes.windows(mem::SECRET.len()).any(|w| w == mem::SECRET);
                    log!(
                        logger,
                        "[mem-victim] BREACH: a page from PID {} held data (my secret: {})",
                        sender,
                        secret
                    );
                }
                rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
            }
            (mem::DONE, None) => {
                if dirty == 0 {
                    log!(logger, "[mem-victim] checked {} pages from PID {}: all zero", checked, sender);
                } else {
                    log!(
                        logger,
                        "[mem-victim] BREACH: {} of {} pages from PID {} held data",
                        dirty,
                        checked,
                        sender
                    );
                }
                rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
                if dirty == 0 {
                    // Off the console too: the checker names this PID and powers off, so a
                    // forged verdict line alone cannot pass the case.
                    test_programs::checker::done();
                }
            }
            _ => {
                rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
