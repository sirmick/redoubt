//! Budget churn gains nothing (R12; kernel/scheduling.md, "Inheritance"): an attacker that
//! creates a weight-1 child budget, lets it run a slice and destroys it, over and over, gets at
//! most its own weight against an equal-weight victim. Variants: the attacker blocked while the
//! child runs; spinning and destroying at the end of its own slice; the child destroyed by a
//! deadline just after the attacker's slice; a fresh intermediate budget for each child. And a
//! shell that keeps giving a budget half its weight and taking it back, with no run in between,
//! leaves the victim at least half, and the oracle's recomputed lifts show that creating and
//! destroying moved nothing (the entry-wait double count grew such a parent's lead by half again
//! each time). The victim's share has no ceiling: the shell pays at its halved weight for what it
//! runs while carved, the create/destroy calls' own time included (kernel/scheduling.md,
//! "Running while carved down"), so how far above half the victim gets follows the kernel's
//! speed. Half, not most: carving 99 of 100 would make that window cost a hundred times over
//! whatever the rule for debt.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("budget-churn");
    for (variant, weight, name, what) in [
        (0u64, 1u64, "blocking", "blocking churner"),
        (1, 1, "spinning-parent", "spinning parent destroying at its slice's end"),
        (2, 1, "deadline", "deadline just after the parent's slice"),
        (3, 1, "fresh-intermediates", "fresh intermediates"),
        (4, 50, "shell", "shell giving and taking back half its weight"),
    ] {
        // Room for the attacker, its children and (variant 3) intermediates.
        let attacker = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let a = b.start(attacker, Role::BudgetChurn, &[variant, weight], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        // Every variant is judged by the victim, who keeps at least half; the shell's own share
        // pays for its calls at its halved weight, so the victim may get more (no ceiling). The
        // post-check judges the victim's share net of the checked build's audits, which a release
        // build does not run, and recomputes every lift.
        let vs = b.judged_share(name, counts[v], window, (500 - TOL, 1000));
        let as_ = b.share(counts[a], window.1 - window.0);
        b.note(format_args!(
            "{}: victim {} of 1000, attacker's subtree {}: gross, audits included; net in the post-check",
            what, vs, as_
        ));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-BUDGET-CHURN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("budget-churn", info) }
