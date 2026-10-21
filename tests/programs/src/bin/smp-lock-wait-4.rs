//! `smp-lock-wait` at four harts where the harts run at once (`bench:smp-lock-wait-4-mttcg`;
//! [`test_programs::lock_wait`]): four threads' calls at once against the same calls in turn on
//! one hart. Alone on the quiet cores, twenty runs a width, they take 1.74 to 1.85 times as long
//! with the release waking the hart it serves, and 2.51 to 2.70 with the release waking every
//! waiting hart: so 2.2, which the second fails.

#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! { test_programs::lock_wait::run("SMP-LOCK-WAIT", 4, 22) }

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("lock-wait", info) }
