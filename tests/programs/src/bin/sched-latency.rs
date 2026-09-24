//! Latency under a named workload (WP-K5; plan section 10), in virtual (instruction) time.
//!
//! The workload, at INIT.md's weights: a driver stand-in (1000, from `system`) that waits for the
//! goldfish RTC's alarm through its device handles; a steward stand-in (1000, from `system`) that
//! sleeps on timeouts, destroys leases by hand and waits for leases' deadlines to destroy them;
//! and N spinning sessions (100 each, from `users`), for N = 1, 4 and 16. In the N = 16 run a
//! spinning server stand-in (1000, from `system`) joins them.
//!
//! Measured, each against its own clock (no difference across two clocks is taken):
//! - driver wake: the RTC's time when the driver runs again, less the alarm it set (RTC clock);
//! - steward timer wake: `time_now` when the steward runs again, less its timeout's deadline;
//! - `budget_destroy`, call to return: how long the steward's destroy of a one-process lease takes, R10's own
//!   cost (kernel time nothing preempts, which every other wake may wait behind), and, when the steward's
//!   slice ended during it, its wait for the CPU after;
//! - deadline notice: `time_now` when the steward receives a lease's `killed` notice, less the lease's
//!   deadline (the timer's lateness plus that destroy).
//!
//! Asserted: the RTC keeps virtual time (`-rtc clock=vm`), so its column is comparable; and in the
//! N = 16 run the server stand-in's share of the spinning CPU is at least its weight's,
//! 1000/2600, less a tolerance (the driver, the steward and this program sleep most of the time
//! and are in neither the numerator nor the denominator). The latencies are reported: they are
//! what a target is chosen from. The kernel's `sched-trace` ring feeds the bench's rank oracle
//! over the whole run.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, Stats, join, rtc};

/// Share tolerance, in thousandths.
const TOL: u64 = 30;
/// Driver alarms and steward timeouts per run.
const K: u64 = 40;
/// Leases per run the steward destroys by hand, and as many by deadline.
const LEASES: u64 = 8;
const WINDOW_US: u64 = 1_500_000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("latency");
    let Some((rtc_mmio, rtc_base, rtc_irq)) = rtc::find() else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCHED-LATENCY")
    };
    // The RTC must run on the virtual clock (`-rtc clock=vm`) for its column to be virtual time.
    let (r0, u0) = (rtc::now_ns(rtc_base), b.now_us());
    let _ = rd::receive(None, 100_000, 0);
    let (r1, u1) = (rtc::now_ns(rtc_base), b.now_us());
    let (rtc_us, us) = ((r1 - r0) / 1000, u1 - u0);
    b.check(
        rtc_us.abs_diff(us) * 100 <= us,
        format_args!("the RTC keeps virtual time: {} µs of RTC over {} µs of time_now", rtc_us, us),
    );
    b.note(format_args!("latencies in µs of virtual (instruction) time: p50 / p99 / max"));
    for n in [1u32, 4, 16] {
        let driver = b.budget(rd::SYSTEM, 1000, 1, rd::FOREVER);
        let steward = b.budget(rd::SYSTEM, 1000, 1, rd::FOREVER);
        // The steward's leases come from a sessions budget it holds.
        let leases = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let d = b.start(driver, Role::Driver, &[K], &[rtc_mmio, rtc_irq]);
        let s = b.start(steward, Role::Steward, &[K, LEASES], &[leases]);
        let mut spinners = [0usize; 16];
        let mut budgets = [0u32; 17];
        for i in 0..n as usize {
            budgets[i] = b.budget(rd::USERS, 100, 1, rd::FOREVER);
            spinners[i] = b.start(budgets[i], Role::Spin, &[], &[]);
        }
        let mut nb = n as usize;
        let server = (n == 16).then(|| {
            budgets[nb] = b.budget(rd::SYSTEM, 1000, 1, rd::FOREVER);
            nb += 1;
            b.start(budgets[nb - 1], Role::Spin, &[], &[])
        });
        b.go(50_000, WINDOW_US);
        let words = b.collect_words(n as usize + usize::from(server.is_some()) + 1 + 3);
        let count = |i: usize| join(words[i][0][0], words[i][0][1]);
        b.note(format_args!(
            "N={}: weights driver 1000, steward 1000, {} sessions x 100{}; spinning budgets {}",
            n,
            n,
            if server.is_some() { ", server 1000" } else { "" },
            nb
        ));
        for (i, tag, what) in [
            (d, Stats::DRIVER_WAKE, "driver wake"),
            (s, Stats::TIMER_WAKE, "steward timer wake"),
            (s, Stats::DESTROY, "budget_destroy, call to return"),
            (s, Stats::DEADLINE, "deadline notice"),
        ] {
            match words[i].iter().find(|w| w[3] & 0xff == tag) {
                Some(w) if w[3] >> 8 > 0 => b.note(format_args!(
                    "N={} {}: {} / {} / {} ({} samples)",
                    n,
                    what,
                    w[0],
                    w[1],
                    w[2],
                    w[3] >> 8
                )),
                _ => b.check(false, format_args!("N={} {}: no samples", n, what)),
            }
        }
        if let Some(sv) = server {
            let total: u64 = spinners[..n as usize].iter().map(|i| count(*i)).sum::<u64>() + count(sv);
            let share = count(sv) * 1000 / total.max(1);
            b.check(
                share + TOL >= 1000 * 1000 / 2600,
                format_args!(
                    "N=16: the 1000-weight server got {} of 1000 of the spinning CPU (its weight's share: 384)",
                    share
                ),
            );
        }
        for bud in budgets[..nb].iter().chain([driver, steward, leases].iter()) {
            let _ = rd::destroy(*bud);
        }
        while rd::receive(Some(b.exit_endpoint()), 0, 0).is_ok() {}
    }
    b.finish("SCHED-LATENCY")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("latency", info) }
