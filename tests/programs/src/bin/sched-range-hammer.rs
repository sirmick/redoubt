//! A range call holds the kernel at most about a slice (kernel/memory.md, R22): a budget maps and
//! unmaps, in a loop, the longest range the kernel takes, each time making every page table of it
//! afresh, beside `sched-latency`'s driver stand-in on the goldfish RTC's alarm
//! ([`test_programs::sched::range_contention`]). Asked for more than a range call may name, the
//! kernel refuses it (`TooLarge`) and the budget falls back to the most it may name.
//!
//! The kernel's trace (`hold-trace`) records each lock section with its cause, and the post-check
//! holds every range call's section in the window to one slice, net of the checked build's audits;
//! the driver's wakes are judged against the responsiveness targets. A kernel that took the first
//! ask whole held the lock for about 100 ms on rv32 at each call.

#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! { test_programs::sched::range_contention() }

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("range-hammer", info) }
