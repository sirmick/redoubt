//! A spinning budget cannot delay another beyond its weight (R12): three spinners in
//! budgets of weight 100, 100 and 300 each get their weight's share of a two-second window.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
/// Allowed error, in thousandths of the window.
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("share");
    let weights = [100u64, 100, 300];
    let total: u64 = weights.iter().sum();
    for w in weights {
        let budget = b.budget(rd::USERS, w as u32, 1, rd::FOREVER);
        b.start(budget, Role::Spin, &[], &[]);
    }
    let (start, end) = b.go(50_000, WINDOW);
    let counts = b.collect(weights.len());
    for (i, w) in weights.iter().enumerate() {
        let share = b.share(counts[i], end - start);
        let want = w * 1000 / total;
        b.check(
            share.abs_diff(want) <= TOL,
            format_args!("weight {} got {} of 1000, want {} +- {}", w, share, want, TOL),
        );
    }
    b.finish("SCHED-SHARE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("share", info) }
