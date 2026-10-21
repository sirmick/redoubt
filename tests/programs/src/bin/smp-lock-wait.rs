//! A hart waiting for the kernel lock does not take the guest's time from the hart holding it
//! (kernel/scheduling.md, R78; `bench:smp-lock-wait`), at 2 harts under `icount`
//! ([`test_programs::lock_wait`]): with the spin the calls at once take a hundred times and more
//! what they take in turn; halted, about one and a half times on rv64 and two and a half on rv32,
//! every call here contended. The bound is three times.

#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! { test_programs::lock_wait::run("SMP-LOCK-WAIT", 2, 30) }

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("lock-wait", info) }
