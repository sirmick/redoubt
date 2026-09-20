//! Use-after-free attack, holder role. Receives a page lent by the victim and never
//! returns it, keeping a raw pointer to where it is mapped. After the victim dies and
//! the grabber has run, it re-reads that pointer: if it now sees the grabber's data, the
//! physical frame was freed and reused while still mapped here — a cross-process
//! use-after-free.

#![no_std]
#![no_main]

use test_programs::uaf::*;
use test_programs::{Logger, log};
use xous::Message;

static mut HELD_PTR: *mut u8 = core::ptr::null_mut();

fn read8(ptr: *const u8) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, b) in out.iter_mut().enumerate() {
        *b = unsafe { ptr.add(i).read_volatile() };
    }
    out
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let sid = xous::create_server_with_address(HOLDER_ADDRESS).expect("couldn't create holder server");
    log!(logger, "[holder] PID {} up", xous::current_pid().unwrap());

    loop {
        let envelope = xous::receive_message(sid).expect("holder receive failed");
        let sender = envelope.sender;
        match &envelope.body {
            Message::MutableBorrow(m) if m.id == HOLD => {
                let ptr = m.buf.as_mut_ptr();
                log!(logger, "[holder] holding page, victim wrote {:?}", core::str::from_utf8(&read8(ptr)));
                unsafe { HELD_PTR = ptr };
                // Never drop the envelope: the borrow is never returned, so the victim's
                // lending thread stays blocked and the page stays mapped here.
                core::mem::forget(envelope);
            }
            Message::BlockingScalar(m) if m.id == SYNC => {
                xous::return_scalar(sender, 1).ok();
            }
            Message::BlockingScalar(m) if m.id == CHECK => {
                if unsafe { HELD_PTR }.is_null() {
                    log!(logger, "UAF TEST FAILED: holder never received the lend (test setup)");
                    xous::return_scalar(sender, 0).ok();
                    continue;
                }
                let seen = read8(unsafe { HELD_PTR });
                log!(logger, "[holder] page now reads {:?}", core::str::from_utf8(&seen));
                if &seen == GRABBER_SENTINEL {
                    log!(logger, "UAF TEST FAILED: use-after-free, holder sees the grabber's data");
                } else {
                    log!(logger, "UAF TEST PASSED: freed frame was not reused under the holder");
                }
                xous::return_scalar(sender, (&seen == VICTIM_SENTINEL) as usize).ok();
                if &seen != GRABBER_SENTINEL {
                    // Off the console too: the checker names this PID and powers off, so a
                    // forged verdict line alone cannot pass the case.
                    test_programs::checker::done();
                }
            }
            other => log!(logger, "[holder] unexpected {:?}", other),
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
