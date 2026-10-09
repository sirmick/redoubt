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
//! timer wakes p50 <= 15 ms and p99 <= 50 ms; the steward's decision wake p50 <= 25 ms and p99 <=
//! 95 ms (from the fourth seed sweep); deadline notice p99 <= 40 ms and R10 kernel time p99 <=
//! 30 ms; a lease's termination from the steward's decision, decision wake + R10, p99 <= 125 ms;
//! the 1000-weight server's count of the spinning CPU at N = 16 is noted beside its weight's 384,
//! with no verdict: under `icount` a count is the machine's instructions, and the steward, which
//! pays for the deadlines' destructions, runs in bursts no weight's share describes;
//! `sched-share` and `sched-large-weight` judge weighted shares from the kernel's charges. The latency
//! targets count the kernel a release build runs, so the program judges none of them: it prints each sample's
//! window (`LATENCY-SAMPLE`), and the bench's post-check subtracts the checked build's audit time inside each
//! window and judges the rest (the bounds are tests/sched-latency.toml's). Destruction follows
//! the dying subtree, so adding objects to another budget moves no R10 term
//! (docs/kernel/budgets.md, "Residual risks").
//!
//! The program in `init`'s place runs in `root` at `init`'s weight, 1,000, which is not part of
//! the workload: measuring from there puts the workload's parent behind 17 spinners whenever it
//! has setup to do, and the N = 16 wakes miss their targets for it. So it carves a budget from
//! `system` at the weight the measurer had when it ran in `system` itself (`MEASURER_WEIGHT`),
//! starts a copy of itself there holding its whole table in the same slots, and that copy
//! measures; it ends the case with the Reset right, as before.

#![no_std]
#![no_main]

use test_programs::rd::{self, MAX_START_HANDLES, ResetKind};
use test_programs::sched::{Bench, K, LEASES, R10_P99, Role, SLICE_US, Stats, WINDOW_US, join, rtc};
use test_programs::spawn;

/// The measurer's weight: about what it had when it ran in `system` itself, `system`'s 250,000
/// less the workload's carves, which `system` keeps room for (kernel/scheduling.md).
const MEASURER_WEIGHT: u32 = 240_000;

fn verdict(b: bool) -> &'static str { if b { "met" } else { "missed" } }

/// In `init`'s place: start the measurer in a budget of its own under `system`, with this
/// program's whole table (`root`, `system`, `users`, Reset, the console, the devices) in the same
/// slots, and power off if it ends without doing so itself.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut handles = [0u32; MAX_START_HANDLES];
    let n = rd::first_free() as usize - 1;
    for (slot, handle) in handles[..n].iter_mut().enumerate() {
        *handle = slot as u32 + 1;
    }
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let pages = rd::free(rd::SYSTEM) / 4;
    let budget = rd::create(rd::SYSTEM, &rd::spec(pages, 1, MEASURER_WEIGHT)).expect("the measurer's budget");
    spawn::spawn(&spawn::image(), budget, exit, measure as *const () as usize, &[], &handles[..n])
        .expect("the measurer");
    let _ = rd::receive(Some(exit), rd::FOREVER, 0);
    let _ = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    test_programs::park()
}

/// The measurer, in its own budget under `system`.
extern "C" fn measure(_: usize) -> ! {
    let devices = rd::OTHER_DEVICES..rd::first_free();
    let mut b = Bench::new("latency");
    let Some((rtc_mmio, rtc_base, rtc_irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCHED-LATENCY")
    };
    // The RTC must run on the virtual clock (`-rtc clock=vm`) for its column to be virtual time.
    let (r0, u0, w0) = rtc::with_time_now(rtc_base);
    let _ = rd::receive(None, 100_000, 0);
    let (r1, u1, w1) = rtc::with_time_now(rtc_base);
    let (rtc_us, us) = ((r1 - r0) / 1000, u1 - u0);
    b.note(format_args!(
        "the RTC keeps virtual time: {} ({} µs of RTC over {} µs of time_now, read within {} and {} µs)",
        verdict(rtc_us.abs_diff(us) * 100 <= us),
        rtc_us,
        us,
        w0,
        w1
    ));
    b.note(format_args!("latencies in µs of virtual (instruction) time: p50 / p99 / max (samples)"));
    for n in [1u32, 4, 16] {
        let driver = b.budget(rd::SYSTEM, 1000, 1, rd::FOREVER);
        let steward = b.budget(rd::SYSTEM, 1000, 1, rd::FOREVER);
        // The steward's leases come from a sessions budget it holds.
        let leases = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        // Both hold their samples' windows until asked, below.
        let d = b.start(driver, Role::Driver, &[K, 1], &[rtc_mmio, rtc_irq]);
        let s = b.start(steward, Role::Steward, &[K, LEASES, 1], &[leases]);
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
        // Gross, as measured; the post-check judges each net of the audits inside its windows.
        for (i, tag, what) in [
            (d, Stats::DRIVER_WAKE, "driver wake"),
            (s, Stats::TIMER_WAKE, "steward timer wake"),
            (s, Stats::DECISION_WAKE, "steward decision wake"),
            (s, Stats::DEADLINE, "deadline notice"),
        ] {
            match stat(i, tag) {
                Some(w) => b.note(format_args!(
                    "N={} {}: {} / {} / {} ({}): gross, audits included; net in the post-check",
                    n,
                    what,
                    w[0],
                    w[1],
                    w[2],
                    w[3] >> 8
                )),
                None => b.check(false, format_args!("N={} {}: no samples", n, what)),
            }
        }
        if let Some(w) = stat(d, Stats::DRIVER_LOST).filter(|w| w[0] > 0) {
            b.note(format_args!(
                "N={} driver: {} alarms never delivered (R5; todo/irq-level-latch.md)",
                n, w[0]
            ));
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
        b.samples(d, &words[d], format_args!("N={}", n));
        b.samples(s, &words[s], format_args!("N={}", n));
        if let Some(sv) = server {
            let total: u64 = spinners[..n as usize].iter().map(|i| count(*i)).sum::<u64>() + count(sv);
            b.note(format_args!(
                "N=16: the 1000-weight server counted {} of 1000 of the spinning CPU (its weight's 384)",
                count(sv) * 1000 / total.max(1)
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
