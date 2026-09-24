//! Budget churn gains nothing (WP-K5; pass inheritance, OWNER DECISION 6): an attacker that
//! creates a weight-1 child budget, lets it run a slice and destroys it, over and over, gets at
//! most its own weight against an equal-weight victim. Variants: the attacker blocked while the
//! child runs; spinning and destroying at the end of its own slice; the child destroyed by a
//! deadline just after the attacker's slice; a fresh intermediate budget for each child. And a
//! shell that keeps giving a budget half its weight and taking it back, with no run in between,
//! is neither starved nor favoured: creating and destroying moves nothing (OWNER DECISION 6; the
//! entry-wait double count grew such a parent's lead by half again each time). Half, not most:
//! a parent's own runtime while its weight is carved away is charged at what it kept (OWNER
//! DECISION 7), the create/destroy calls' own time included, so carving 99 of 100 would make that
//! window cost a hundred times over whatever the rule for debt.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("budget-churn");
    for (variant, weight, what) in [
        (0u64, 1u64, "blocking churner"),
        (1, 1, "spinning parent destroying at its slice's end"),
        (2, 1, "deadline just after the parent's slice"),
        (3, 1, "fresh intermediates"),
        (4, 50, "shell giving and taking back half its weight"),
    ] {
        // Room for the attacker, its children and (variant 3) intermediates.
        let attacker = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let a = b.start(attacker, Role::BudgetChurn, &[variant, weight], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let (start, end) = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        let (vs, as_) = (b.share(counts[v], end - start), b.share(counts[a], end - start));
        // The shell's own share also pays for its calls, which its count leaves out: judge it by
        // the victim, who gets neither more nor less than half.
        let ok = if variant == 4 { vs.abs_diff(500) <= TOL } else { vs + TOL >= 500 };
        b.check(ok, format_args!("{}: victim {} of 1000, attacker's subtree {}", what, vs, as_));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-BUDGET-CHURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("budget-churn", info) }
