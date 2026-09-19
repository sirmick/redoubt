//! Drives every Xous message type against `log-server`, and so every page-table
//! operation the kernel performs for IPC: lend, mutable lend, return, and move.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{log, op, Logger, Page};
use xous::{MemoryFlags, Message};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let cid = logger.cid;
    let mut failures = 0;
    log!(logger, "[ipc] PID {} connected", xous::current_pid().unwrap());

    // Scalar: fire and forget.
    xous::send_message(cid, Message::new_scalar(op::PRINT_SCALARS, 1, 2, 3, 4)).expect("scalar");

    // BlockingScalar: the reply comes back in registers. Use values that need 64 bits
    // where they are available.
    let big = 1usize << (usize::BITS - 8);
    let sum = match xous::send_message(cid, Message::new_blocking_scalar(op::SUM, big, big, 3, 4)) {
        Ok(xous::Result::Scalar1(sum)) => sum,
        other => {
            log!(logger, "[ipc] FAIL: unexpected SUM reply {:?}", other);
            0
        }
    };
    let ok = sum == 2 * big + 7;
    failures += !ok as usize;
    log!(logger, "[ipc] {}: blocking scalar sum = {:#x}", if ok { "ok" } else { "FAIL" }, sum);

    // MutableBorrow: the server edits our page in place, and we get it back.
    let mut scratch = Page::new();
    scratch.write_str("lent mutably across address spaces").ok();
    xous::send_message(cid, Message::new_lend_mut(op::UPPERCASE, scratch.range, None, scratch.valid()))
        .expect("couldn't lend_mut");
    let ok = scratch.bytes() == b"LENT MUTABLY ACROSS ADDRESS SPACES";
    failures += !ok as usize;
    log!(logger, "[ipc] {}: lend_mut returned {:?}", if ok { "ok" } else { "FAIL" }, core::str::from_utf8(scratch.bytes()));

    // Lend the same page many times: every round trip unmaps it here, maps it in the
    // server, and reverses that on return. The mapping must survive.
    let mut survived = true;
    for round in 0..1000 {
        xous::send_message(cid, Message::new_lend_mut(op::UPPERCASE, scratch.range, None, scratch.valid()))
            .expect("couldn't lend_mut repeatedly");
        if scratch.bytes()[0] != b'L' {
            survived = false;
            log!(logger, "[ipc] FAIL: page contents lost on round {}", round);
            break;
        }
    }
    failures += !survived as usize;
    if survived {
        log!(logger, "[ipc] ok: 1000 lend_mut round trips");
    }

    // Move: the page leaves our address space for good.
    let mut gift = Page::new();
    write!(gift, "this page now belongs to the server").ok();
    let gift_addr = gift.range.as_ptr() as usize;
    let message = xous::MemoryMessage { id: op::PRINT_AND_KEEP, buf: gift.range, offset: None, valid: gift.valid() };
    xous::send_message(cid, Message::Move(message)).expect("couldn't move");
    // The address must be unmapped here now, so mapping a fresh page there must succeed.
    let remap = xous::map_memory(None, xous::MemoryAddress::new(gift_addr), 4096, MemoryFlags::R | MemoryFlags::W);
    let ok = remap.is_ok();
    failures += !ok as usize;
    log!(logger, "[ipc] {}: moved page's address is free again", if ok { "ok" } else { "FAIL" });

    if failures == 0 {
        log!(logger, "IPC TEST PASSED");
    } else {
        log!(logger, "IPC TEST FAILED: {} failure(s)", failures);
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
