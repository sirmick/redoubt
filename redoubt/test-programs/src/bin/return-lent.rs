//! Attack test, lender role: lend a page mutably, and while it is lent (this thread blocked in
//! the lend), a second thread tries to unmap and to remap the lent address. Both must be
//! refused (the lent PTE is the lender's only record of the loan). When the borrower returns
//! the page, the return must not panic the kernel, and this thread must get its page back.
//!
//! On the old kernel, unmap cleared the lent PTE; the borrower's return then hit
//! `assert!(dest ... shared)` / an `expect` and halted the machine. See
//! `redoubt/tests/return-lent-unmapped.toml`. The verdict is survival only, via `attack-checker`.

#![no_std]
#![no_main]

use test_programs::return_lent::{self, LENT_ADDR};
use test_programs::{Logger, checker, log};
use xous::{MemoryAddress, MemoryFlags, MemoryRange, MemorySize, Message};

/// Second thread: once the page is lent, try to pull the lent address out from under the loan.
fn clobber(_arg: usize) -> ! {
    let mut logger = Logger::connect();
    test_programs::wait_ms(30);
    let range = unsafe { MemoryRange::new(LENT_ADDR, 4096) }.unwrap();
    let unmapped = xous::unmap_memory(range);
    let remapped =
        xous::map_memory(None, MemoryAddress::new(LENT_ADDR), 4096, MemoryFlags::R | MemoryFlags::W)
            .map(|_| ());
    log!(logger, "[lender] while lent: unmap {:?}, remap {:?}", unmapped, remapped);
    test_programs::park()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // Reserve the fixed address the clobber thread will attack, and touch it so it is backed.
    let page = xous::map_memory(None, MemoryAddress::new(LENT_ADDR), 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("map");
    let text = b"lender data";
    // SAFETY: `page` is mapped R|W here and is larger than `text`.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), page.as_mut_ptr(), text.len()) };

    let sid = xous::SID::from_bytes(return_lent::ADDRESS).unwrap();
    let borrower = xous::connect(sid).expect("connect to borrower");
    xous::create_thread_1(clobber, 0).expect("spawn clobber thread");

    // Mutable lend: blocks until the borrower returns it.
    let result =
        xous::send_message(borrower, Message::new_lend_mut(1, page, None, MemorySize::new(text.len())));
    // SAFETY: the page is back and mapped here again.
    let seen = unsafe { core::slice::from_raw_parts(page.as_ptr(), text.len()) };
    let intact = result.is_ok() && seen == text;
    log!(logger, "[lender] lend returned {:?}, page intact: {}", result, intact);

    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
