//! A budget that sleeps in short timeouts wakes no hart but its own for them (kernel/timer.md,
//! "The hart timer"): A and B, one thread each in budgets of their own, each sleep a thousand
//! times for 1 ms on two harts, so each hart re-arms its timer a thousand times a second as its
//! sleeper blocks and is woken, as the harts of a machine at rest do for the system's own periodic
//! sleeps. A timeout's payer runs nowhere while its one thread sleeps, so it is armed on one hart,
//! the first to re-arm: the hart its thread left. The post-check counts the kernel sections
//! entered from the idle that found nothing to do, from A's first run on: a kernel that arms
//! every hart for every item (`timer-any-hart`) wakes each hart for the other's naps too, a
//! section each that finds the item served and nothing runnable, and fails the bound.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark};

/// Each sleeper's naps and how many: about a second of sleeping, a timeout a millisecond each.
const NAP_US: u64 = 1_000;
const NAPS: u64 = 1_000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("timer-flood-at-rest");
    let a = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    mark(a, 2);
    let wa = b.start(a, Role::WakeDelay, &[NAP_US, NAPS, 0], &[]);
    let other = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let wb = b.start(other, Role::WakeDelay, &[NAP_US, NAPS, 0], &[]);
    b.go(50_000, NAPS * NAP_US);
    let r = b.collect(2);
    b.note(format_args!(
        "two sleepers, {} naps of {} µs each; the shortest delay from a nap's start to the sleeper's run was {} and {} µs",
        NAPS, NAP_US, r[wa], r[wb]
    ));
    b.finish("TIMER-FLOOD-AT-REST")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("timer-flood-at-rest", info) }
