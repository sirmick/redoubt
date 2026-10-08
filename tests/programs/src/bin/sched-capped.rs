//! R12 across harts, the capped budgets (kernel/scheduling.md, "The current minimum and ties"):
//! the four scenarios the model runs (kernel/model.md, "Scheduler scenarios"), on the machine.
//! Each runs two seconds, then a budget wakes or gains a thread, and every budget's share over the
//! two seconds after is judged by the post-check on the kernel's charges net of lock waits, against
//! water-filling at the harts the trace states (`HART-SHARE`):
//! - late join (two harts): A (900, one thread) and B (100, one) run; then C (100, one) wakes. A holds a
//!   hart, and B and C share the other. A floor that counted A would let C enter far behind B and take its
//!   hart until it caught up;
//! - second cap (three harts): A (1000, one), B (100, one), C and D (10, five threads each); then E (10, one)
//!   wakes. A and B each hold a hart, and C, D and E share the third;
//! - uncap (two harts): A (900, one), B and C (100, one each); then A gains a second thread and gets 1.64
//!   harts. A lift that let it keep its lag would give it both harts until it caught up;
//! - spread: A (100, four threads) and B (100, one): a hart each at two; at three and four, B one and A the
//!   rest.
//!
//! Every scenario runs at any hart count; late join and uncap are judged at two harts, second cap
//! at three, and spread at two and four (`HART-SHARE`'s `@`): elsewhere their shares are only
//! reported. At three and four harts the checked build's lock waits are 300 to 400 of 1000 and fall
//! unevenly across the harts, so a one-thread budget's share there moves with where it runs.
//! At four, late join and uncap have fewer threads than harts, so every budget holds a hart and
//! only the checked build's lock waits differ.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark, ticks};

/// Allowed error, in thousandths of the CPU charged (R12's 50).
const TOL: u64 = 50;
/// Before the change each scenario makes, µs.
const RUN_IN: u64 = 2_000_000;
/// After it, µs.
const JUDGED: u64 = 2_000_000;
/// From the change to the judged window: a slice and more.
const SETTLE: u64 = 10_000;
const LEAD: u64 = 50_000;

/// A budget of `weight` under `users`, marked with `mark_w`, running `role` with `params`.
fn spinner(b: &mut Bench, weight: u32, mark_w: u32, role: Role, params: &[u64]) -> usize {
    let budget = b.budget(rd::USERS, weight, 1, rd::FOREVER);
    mark(budget, mark_w);
    b.start(budget, role, params, &[])
}

/// When the scenario's change comes, in ticks, for a window opened now.
fn change_at(b: &Bench) -> u64 { ticks() + (LEAD + RUN_IN) * b.tpu }

/// Open the scenario's window and wait for its `n` children's reports; the judged window, µs.
fn run(b: &mut Bench, n: usize) -> (u64, u64) {
    let (start, end) = b.go(LEAD, RUN_IN + JUDGED);
    b.collect(n);
    (start + RUN_IN + SETTLE, end)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("capped");
    // Each scenario judged at the harts it is about.
    let (at2, at3, at4) = ((TOL, "@2"), (TOL, "@3"), (TOL, "@4"));

    // Late join.
    let at = change_at(&b);
    spinner(&mut b, 900, 2, Role::Spin, &[]);
    spinner(&mut b, 100, 3, Role::Spin, &[]);
    spinner(&mut b, 100, 4, Role::SpinFrom, &[at]);
    let w = run(&mut b, 3);
    b.hart_share("late-a", w, at2, (2, 1), &[(100, 1), (100, 1)]);
    b.hart_share("late-b", w, at2, (3, 1), &[(900, 1), (100, 1)]);
    b.hart_share("late-c", w, at2, (4, 1), &[(900, 1), (100, 1)]);

    // Second cap.
    let at = change_at(&b);
    spinner(&mut b, 1000, 5, Role::Spin, &[]);
    spinner(&mut b, 100, 6, Role::Spin, &[]);
    spinner(&mut b, 10, 7, Role::SpinThreads, &[5, 0, 0]);
    spinner(&mut b, 10, 8, Role::SpinThreads, &[5, 0, 0]);
    spinner(&mut b, 10, 9, Role::SpinFrom, &[at]);
    let w = run(&mut b, 5);
    b.hart_share("second-a", w, at3, (5, 1), &[(100, 1), (10, 5), (10, 5), (10, 1)]);
    b.hart_share("second-b", w, at3, (6, 1), &[(1000, 1), (10, 5), (10, 5), (10, 1)]);
    b.hart_share("second-c", w, at3, (7, 5), &[(1000, 1), (100, 1), (10, 5), (10, 1)]);
    b.hart_share("second-e", w, at3, (9, 1), &[(1000, 1), (100, 1), (10, 5), (10, 5)]);

    // Uncap.
    let at = change_at(&b);
    spinner(&mut b, 900, 10, Role::SpinThreads, &[1, 1, at]);
    spinner(&mut b, 100, 11, Role::Spin, &[]);
    spinner(&mut b, 100, 12, Role::Spin, &[]);
    let w = run(&mut b, 3);
    b.hart_share("uncap-a", w, at2, (10, 2), &[(100, 1), (100, 1)]);
    b.hart_share("uncap-b", w, at2, (11, 1), &[(900, 2), (100, 1)]);
    b.hart_share("uncap-c", w, at2, (12, 1), &[(900, 2), (100, 1)]);

    // Spread: no change, the whole window judged.
    spinner(&mut b, 100, 13, Role::SpinThreads, &[4, 0, 0]);
    spinner(&mut b, 100, 14, Role::Spin, &[]);
    let w = b.go(LEAD, JUDGED);
    b.collect(2);
    b.hart_share("spread-a", w, at2, (13, 4), &[(100, 1)]);
    b.hart_share("spread-b", w, at2, (14, 1), &[(100, 4)]);
    b.hart_share("spread-a-4", w, at4, (13, 4), &[(100, 1)]);
    b.hart_share("spread-b-4", w, at4, (14, 1), &[(100, 4)]);
    b.finish("SCHED-CAPPED")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("capped", info) }
