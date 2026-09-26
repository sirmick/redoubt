//! Attack test, attacker role: map and touch far more anonymous memory than the machine has
//! RAM, in ever smaller chunks, until the kernel refuses even one page. Running out must be this
//! process's `OutOfMemory`, not a kernel panic. It then gives everything back and exits; the
//! verdict is that another process is still served afterwards (see
//! `touch-beyond-ram-survivor`).

#![no_std]
#![no_main]

use test_programs::{Logger, log, rd};

/// More pages than the case's small guest has RAM (see the toml's `memory_mib`).
const PAGES: usize = 16 * 1024; // 64 MiB, above the 32 MiB machine.
/// The first chunk; each refusal halves it.
const CHUNK: usize = 1024;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[attacker] mapping and touching up to {} pages", PAGES);
    let mut regions = [(0usize, 0usize); 64];
    let (mut count, mut total, mut chunk) = (0, 0, CHUNK);
    let mut refusal = None;
    while chunk > 0 && total < PAGES && count < regions.len() {
        match rd::map_anon(chunk * rd::PAGE_SIZE, rd::rw()) {
            Ok(at) => {
                for page in 0..chunk {
                    rd::poke(at + page * rd::PAGE_SIZE, 0xa5);
                }
                regions[count] = (at, chunk * rd::PAGE_SIZE);
                count += 1;
                total += chunk;
            }
            Err(e) => {
                refusal = Some(e);
                chunk /= 2;
            }
        }
    }
    for &(at, len) in &regions[..count] {
        rd::unmap(at, len).expect("give the pages back");
    }
    if total >= PAGES {
        log!(logger, "[attacker] BREACH: mapped all {} pages", PAGES);
    } else {
        log!(logger, "[attacker] refused after {} pages: {:?}", total, refusal);
    }
    rd::process_exit(0)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
