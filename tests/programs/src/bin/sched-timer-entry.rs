//! What a timer interrupt costs the kernel (kernel/scheduling.md, "Fair kernel entry is bounded by
//! count"): two spinners in budgets of equal weight, and nothing else to do. Every timer interrupt
//! from user mode is a slice's end: on one hart it switches to the other budget, on two each hart
//! picks its own spinner again. The kernel's lock sections (`hold-trace`) give each one's length,
//! trap to return, and the post-check bounds the longest a timer interrupt caused.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("timer-entry");
    for _ in 0..2 {
        let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(spinner, Role::Spin, &[], &[]);
    }
    b.go(50_000, 300_000);
    let r = b.collect(2);
    b.note(format_args!("the spinners counted {} and {}", r[0], r[1]));
    b.finish("SCHED-TIMER-ENTRY")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("timer-entry", info) }
