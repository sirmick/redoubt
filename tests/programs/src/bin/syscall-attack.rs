//! Attack test: a lend that once reached a kernel panic must now be a clean error. See
//! `tests/syscall-attack.toml`; the case's verdict is survival only, from log-server's `DONE`,
//! which this program reports once it is done.

#![no_std]
#![no_main]

use test_programs::{Logger, checker, log, op, rd};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();

    // Lend more than a lend may hold (`MAX_LEND_PAGES`). Finding room for a lend this size
    // in the destination once computed the region end minus the size, which underflowed; the
    // kernel now refuses it while decoding.
    const BIG: usize = 5 << 20;
    let big = rd::map_anon(BIG, rd::rw()).expect("couldn't map 5 MiB");
    let lend = rd::call_waiting(
        rd::LOG,
        &rd::body([op::PRINT, 0, 0, 0]),
        rd::pages(big, BIG / rd::PAGE_SIZE),
        rd::FOREVER,
    );
    rd::unmap(big, BIG).expect("couldn't unmap the 5 MiB range");
    log!(logger, "[syscall] oversized lend -> {:?}", lend.map(|_| ()));

    // Survived every attempt; the checker powers off under its own PID.
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
