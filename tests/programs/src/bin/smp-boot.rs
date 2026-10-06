//! Every hart reaches the scheduler and runs user threads (`bench:smp-boot`): spinners in budgets
//! of their own, as many as the most harts the case boots, beside two budgets whose threads enter
//! the kernel without pause, so the harts contend for the kernel lock. The kernel's account at
//! `system_reset` says how many harts ran user code, and its lock checks that no hart waited behind
//! more sections than there are other harts (R78).
#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 1_000_000;
/// Spinners: the most harts the case boots.
const SPINNERS: usize = 4;
/// Budgets of threads that sleep 1 µs after every 20 µs of counting.
const GAMERS: usize = 2;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("smp");
    let tpu = b.tpu;
    for _ in 0..SPINNERS {
        let budget = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(budget, Role::Spin, &[], &[]);
    }
    for _ in 0..GAMERS {
        let budget = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(budget, Role::Gamer, &[20 * tpu, 1, 2], &[]);
    }
    b.go(50_000, WINDOW);
    let counts = b.collect(SPINNERS + GAMERS);
    for (i, count) in counts[..SPINNERS + GAMERS].iter().enumerate() {
        b.check(*count > 0, format_args!("runner {} counted {}", i, count));
    }
    b.finish("SMP-BOOT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("smp", info) }
