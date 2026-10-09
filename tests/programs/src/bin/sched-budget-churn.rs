//! Budget churn gains nothing (R12; kernel/scheduling.md, "Inheritance"): an attacker that
//! creates a weight-1 child budget, lets it run a slice and destroys it, over and over, gets at
//! most its own weight against an equal-weight victim: the victim keeps at least its share of the
//! harts, judged by the post-check on the kernel's charges net of lock waits (`HART-SHARE`). Variants: the
//! attacker blocked while the child runs; spinning and destroying at the end of its own slice; the child
//! destroyed by a deadline just after the attacker's slice; a fresh intermediate budget for each child. The
//! shell's variant is `sched-budget-churn-shell`.

#![no_std]
#![no_main]

use test_programs::sched::{Bench, churn_against_victim};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("budget-churn");
    // (variant, the child's weight, name, what, the victim's mark: no child weighs one)
    churn_against_victim(
        &mut b,
        &[
            (0, 1, "blocking", "blocking churner", 2),
            (1, 1, "spinning-parent", "spinning parent destroying at its slice's end", 3),
            (2, 1, "deadline", "deadline just after the parent's slice", 4),
            (3, 1, "fresh-intermediates", "fresh intermediates", 5),
        ],
    );
    b.finish("SCHED-BUDGET-CHURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("budget-churn", info) }
