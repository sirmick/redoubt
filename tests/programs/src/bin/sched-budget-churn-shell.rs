//! Budget churn gains nothing, by carving (R12; kernel/scheduling.md, "Inheritance"): a shell that
//! keeps giving a budget half its weight and taking it back, with no run in between, leaves an
//! equal-weight victim at least its share of the harts, judged by the post-check on the kernel's
//! charges net of lock waits (`HART-SHARE`), and the oracle's recomputed lifts show that creating
//! and destroying moved nothing (the entry-wait double count grew such a parent's lead by half
//! again each time). The victim's share has no ceiling: the shell pays at its halved weight for
//! what it runs while carved, the create/destroy calls' own time included (kernel/scheduling.md,
//! "Running while carved down"), so how far above half the victim gets follows the kernel's
//! speed. Half, not most: carving 99 of 100 would make that window cost a hundred times over
//! whatever the rule for debt. `sched-budget-churn` runs the other variants.

#![no_std]
#![no_main]

use test_programs::sched::{Bench, churn_against_victim};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("budget-churn-shell");
    // (variant, the child's weight, name, what, the victim's mark: no child weighs one)
    churn_against_victim(&mut b, &[(4, 50, "shell", "shell giving and taking back half its weight", 2)]);
    b.finish("SCHED-BUDGET-CHURN-SHELL")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("budget-churn-shell", info) }
