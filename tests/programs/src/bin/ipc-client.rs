//! Drives every kind of message against `log-server` on the log endpoint, and so every
//! page-table operation the kernel performs for IPC: a call with words, a lend and a writable
//! lend (and their return), and a send with a transfer.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{Logger, Page, log, op, rd};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let mut failures = 0;
    log!(logger, "[ipc] connected");

    // A call with words: the reply's words come back. Use values that need 64 bits where they
    // are available.
    let big = 1usize << (usize::BITS - 8);
    let sum = match rd::call_waiting(rd::LOG, &rd::body([op::SUM, big, big, 3]), None, rd::FOREVER) {
        Ok(reply) => reply.words[0],
        Err(e) => {
            log!(logger, "[ipc] FAIL: SUM -> {:?}", e);
            0
        }
    };
    let ok = sum == 2 * big + 3;
    failures += !ok as usize;
    log!(logger, "[ipc] {}: call sum = {:#x}", if ok { "ok" } else { "FAIL" }, sum);

    // A writable lend: the server edits our page in place, and we get it back at the reply.
    let mut scratch = Page::new();
    scratch.write_str("lent mutably across address spaces").ok();
    let upper = rd::body([op::UPPERCASE, scratch.bytes().len(), 0, 0]);
    rd::call_waiting(rd::LOG, &upper, scratch.pages(), rd::FOREVER).expect("couldn't lend writable");
    let ok = scratch.bytes() == b"LENT MUTABLY ACROSS ADDRESS SPACES";
    failures += !ok as usize;
    log!(
        logger,
        "[ipc] {}: writable lend returned {:?}",
        if ok { "ok" } else { "FAIL" },
        core::str::from_utf8(scratch.bytes())
    );

    // Lend the same page many times: every round trip maps it in the server and unmaps it
    // there at the reply. The mapping here must survive.
    let mut survived = true;
    for round in 0..1000 {
        rd::call_waiting(rd::LOG, &upper, scratch.pages(), rd::FOREVER)
            .expect("couldn't lend writable repeatedly");
        if scratch.bytes()[0] != b'L' {
            survived = false;
            log!(logger, "[ipc] FAIL: page contents lost on round {}", round);
            break;
        }
    }
    failures += !survived as usize;
    if survived {
        log!(logger, "[ipc] ok: 1000 writable lend round trips");
    }

    // A transfer: the page leaves our address space for good.
    let mut gift = Page::new();
    write!(gift, "this page now belongs to the server").ok();
    let keep = rd::body([op::PRINT_AND_KEEP, gift.bytes().len(), 0, 0]);
    rd::send_waiting(rd::LOG, &keep, gift.pages(), rd::FOREVER).expect("couldn't transfer");
    // A send does not wait for the server; a call after it does, so the server has printed
    // the transferred page before the lines below.
    rd::call_waiting(rd::LOG, &rd::body([op::SUM, 0, 0, 0]), None, rd::FOREVER).expect("sync");
    // The address must be unmapped here now, so mapping a fresh page there must succeed.
    let ok = rd::map_fixed(gift.addr, rd::PAGE_SIZE, rd::rw()).is_ok();
    failures += !ok as usize;
    log!(logger, "[ipc] {}: transferred page's address is free again", if ok { "ok" } else { "FAIL" });

    if failures == 0 {
        log!(logger, "IPC TEST PASSED");
    } else {
        log!(logger, "IPC TEST FAILED: {} failure(s)", failures);
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
