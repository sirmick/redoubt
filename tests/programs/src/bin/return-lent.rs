//! Attack test, lender role: lend a page writable, and while it is lent (this thread blocked in
//! the call), a second thread tries to unmap and to remap the lent address. Both must be
//! refused (the lent PTE is the lender's only record of the loan). When the borrower replies,
//! the return must not panic the kernel, and this thread must get its page back.
//!
//! On the old kernel, unmap cleared the lent PTE; the borrower's return then hit
//! `assert!(dest ... shared)` / an `expect` and halted the machine. See
//! `tests/return-lent-unmapped.toml`. The verdict is survival only, via log-server's `DONE`.

#![no_std]
#![no_main]

use test_programs::return_lent::LENT_ADDR;
use test_programs::{Logger, checker, log, rd};

/// Second thread: once the page is lent, try to pull the lent address out from under the loan.
fn clobber(_arg: usize) {
    let mut logger = Logger::connect();
    test_programs::wait_ms(30);
    let unmapped = rd::unmap(LENT_ADDR, rd::PAGE_SIZE);
    let remapped = rd::map_fixed(LENT_ADDR, rd::PAGE_SIZE, rd::rw());
    log!(logger, "[lender] while lent: unmap {:?}, remap {:?}", unmapped, remapped);
    test_programs::park()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The fixed address the clobber thread will attack, touched so it is backed.
    rd::map_fixed(LENT_ADDR, rd::PAGE_SIZE, rd::rw()).expect("map");
    let text = b"lender data";
    // SAFETY: `LENT_ADDR` is mapped read-write here and is larger than `text`.
    unsafe { core::ptr::copy_nonoverlapping(text.as_ptr(), LENT_ADDR as *mut u8, text.len()) };
    rd::thread(clobber, 0).expect("spawn clobber thread");
    // A writable lend on the boot endpoint, whose receive right the borrower holds: blocks
    // until the borrower replies.
    let result =
        rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([1, 0, 0, 0]), rd::pages(LENT_ADDR, 1), rd::FOREVER);
    // SAFETY: the page is back and mapped here again.
    let seen = unsafe { core::slice::from_raw_parts(LENT_ADDR as *const u8, text.len()) };
    let intact = result.is_ok() && seen == text;
    log!(
        logger,
        "[lender] lend returned {}, page intact: {}",
        if result.is_ok() { "Ok" } else { "an error" },
        intact
    );
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
