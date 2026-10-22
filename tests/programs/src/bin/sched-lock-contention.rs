//! Fair kernel entry under contention (R78, kernel/scheduling.md), at two harts: one budget calls
//! in a loop the costliest call R12 bounds, a `map_fixed` of a range at the cap that makes every
//! page table of it afresh, and its `unmap` (kernel/memory.md, R22), beside `sched-latency`'s
//! driver stand-in on the goldfish RTC's alarm ([`test_programs::sched::lock_contention`]).
//!
//! The kernel's trace (`lock-trace`) records each wait for the kernel lock with its ticket, and the
//! post-check requires the waits to take the lock in ticket order, so none outlasts the sections
//! queued ahead of it. The driver's wakes are judged by the post-check net of the checked build's
//! audits against the responsiveness targets: the alarm's interrupt reaches the idle hart too, so a
//! wake waits out at most the section in progress (kernel/scheduling.md, R78).
//! `sched-lock-contention-4` runs three at four harts.

#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! { test_programs::sched::lock_contention(1) }

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("lock-contention", info) }
