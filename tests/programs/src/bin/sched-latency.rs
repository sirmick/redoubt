//! Latency under a named workload (kernel/scheduling.md, "Responsiveness"), in virtual
//! (instruction) time.
//!
//! The workload, at the weights in servers/init.md: a driver stand-in (1000, from `system`) that
//! waits for the goldfish RTC's alarm through its device handles; a steward stand-in (1000, from
//! `system`) that sleeps on timeouts, destroys leases by hand after a timeout (its decision) and
//! waits for leases' deadlines to destroy them; and N spinning sessions (100 each, from `users`),
//! for N = 1, 4 and 16. In the N = 16 run a spinning server stand-in (1000, from `system`) joins
//! them.
//!
//! Measured, each against its own clock (no difference across two clocks is taken), `K` wakes and
//! `LEASES` destructions of each kind per N:
//! - driver wake: the RTC's time when the driver runs again, less the alarm it set (RTC clock);
//! - steward timer wake: `time_now` when the steward runs again, less its timeout's deadline; and the same
//!   for the timeout after which it decides to destroy a lease (its decision wake);
//! - `budget_destroy`, call to return: R10's own cost (kernel time nothing preempts) and, when the steward's
//!   slice ended during it, its wait for the CPU after. Recorded, not asserted: its bound is one round, R10
//!   plus (runnable budgets + 2) slices. R10's kernel time alone is in the kernel's trace (records `X` and
//!   `Y`), and the bench's post-check asserts its p99;
//! - deadline notice: `time_now` when the steward receives a lease's `killed` notice, less the lease's
//!   deadline;
//! - R10's own kernel time, and the object frames it walks (its cost grows with them), are in the kernel's
//!   trace (records `X`, `Y`, `Z`); the bench's post-check reports and bounds them.
//!
//! The targets (kernel/scheduling.md, virtual time, N <= 16, this workload): driver and steward
//! wakes p50 <= 15 ms and p99 <= 50 ms; deadline notice p99 <= 30 ms; R10 kernel time p99 <= 30 ms
//! (the post-check); a lease's termination from the steward's decision, decision wake + R10, p99 <=
//! 80 ms; the 1000-weight server's share of the spinning CPU at N = 16 at least its weight's less
//! 30/1000. Each is printed as `met` or `missed`; the virtual-time case requires `met`, and the
//! plain-TCG reference case only reports. Adding objects moves the R10 terms
//! (docs/todo/budget-destroy-cost.md).

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US, Stats, join, rtc};

/// Share tolerance, in thousandths.
const TOL: u64 = 30;
/// Driver alarms and steward timeouts per run (p99 is then the second largest).
const K: u64 = 200;
/// Leases per run the steward destroys by hand, and as many by deadline.
const LEASES: u64 = 50;
const WINDOW_US: u64 = 16_000_000;
/// The targets, µs.
const WAKE_P50: usize = 15_000;
const WAKE_P99: usize = 50_000;
const NOTICE_P99: usize = 30_000;
const R10_P99: usize = 30_000;

fn verdict(b: bool) -> &'static str { if b { "met" } else { "missed" } }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::log_rx();
    let mut b = Bench::new("latency");
    let Some((rtc_mmio, rtc_base, rtc_irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCHED-LATENCY")
    };
    // The RTC must run on the virtual clock (`-rtc clock=vm`) for its column to be virtual time.
    let (r0, u0) = (rtc::now_ns(rtc_base), b.now_us());
    let _ = rd::receive(None, 100_000, 0);
    let (r1, u1) = (rtc::now_ns(rtc_base), b.now_us());
    let (rtc_us, us) = ((r1 - r0) / 1000, u1 - u0);
    b.note(format_args!(
        "the RTC keeps virtual time: {} ({} µs of RTC over {} µs of time_now)",
        verdict(rtc_us.abs_diff(us) * 100 <= us),
        rtc_us,
        us
    ));
    b.note(format_args!("latencies in µs of virtual (instruction) time: p50 / p99 / max (samples)"));
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
        // The spinners' counts, the driver's two reports and the steward's four.
        let words = b.collect_words(n as usize + usize::from(server.is_some()) + 2 + 4);
        let count = |i: usize| join(words[i][0][0], words[i][0][1]);
        b.note(format_args!(
            "N={}: weights driver 1000, steward 1000, {} sessions x 100{}; spinning budgets {}",
            n,
            n,
            if server.is_some() { ", server 1000" } else { "" },
            nb
        ));
        let stat =
            |i: usize, tag: usize| words[i].iter().find(|w| w[3] & 0xff == tag && w[3] >> 8 > 0).copied();
        for (i, tag, what, p50, p99) in [
            (d, Stats::DRIVER_WAKE, "driver wake", Some(WAKE_P50), WAKE_P99),
            (s, Stats::TIMER_WAKE, "steward timer wake", Some(WAKE_P50), WAKE_P99),
            (s, Stats::DECISION_WAKE, "steward decision wake", Some(WAKE_P50), WAKE_P99),
            (s, Stats::DEADLINE, "deadline notice", None, NOTICE_P99),
        ] {
            match stat(i, tag) {
                Some(w) => {
                    let met = p50.is_none_or(|t| w[0] <= t) && w[1] <= p99;
                    match p50 {
                        Some(t) => b.note(format_args!(
                            "N={} {}: {} / {} / {} ({}): target {} (p50 <= {}, p99 <= {})",
                            n,
                            what,
                            w[0],
                            w[1],
                            w[2],
                            w[3] >> 8,
                            verdict(met),
                            t,
                            p99
                        )),
                        None => b.note(format_args!(
                            "N={} {}: {} / {} / {} ({}): target {} (p99 <= {})",
                            n,
                            what,
                            w[0],
                            w[1],
                            w[2],
                            w[3] >> 8,
                            verdict(met),
                            p99
                        )),
                    }
                }
                None => b.check(false, format_args!("N={} {}: no samples", n, what)),
            }
        }
        if let Some(w) = stat(d, Stats::DRIVER_LOST).filter(|w| w[0] > 0) {
            b.note(format_args!("N={} driver: {} alarms never delivered (R5; K3's follow-up)", n, w[0]));
        }
        // Recorded, not judged: one round, R10 plus (runnable budgets + 2) slices.
        match stat(s, Stats::DESTROY) {
            Some(w) => b.note(format_args!(
                "N={} budget_destroy, call to return: {} / {} / {} ({}); recorded, bound one round: {} µs",
                n,
                w[0],
                w[1],
                w[2],
                w[3] >> 8,
                R10_P99 as u64 + (nb as u64 + 2 + 2) * SLICE_US
            )),
            None => b.check(false, format_args!("N={} budget_destroy: no samples", n)),
        }
        if let Some(sv) = server {
            let total: u64 = spinners[..n as usize].iter().map(|i| count(*i)).sum::<u64>() + count(sv);
            let share = count(sv) * 1000 / total.max(1);
            b.note(format_args!(
                "N=16: the 1000-weight server got {} of 1000 of the spinning CPU: target {} (weight share 384, less {})",
                share,
                verdict(share + TOL >= 1000 * 1000 / 2600),
                TOL
            ));
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
