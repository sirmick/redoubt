//! `sched-lock-contention` at four harts: three budgets call the costliest call R12 bounds in a
//! loop, one on every hart but one, beside the driver stand-in
//! ([`test_programs::sched::lock_contention`]). The waits must take the kernel lock in ticket
//! order; the driver's wake is gated on its p50 at four harts, its p99 recorded.

#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! { test_programs::sched::lock_contention(3) }

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("lock-contention", info) }
