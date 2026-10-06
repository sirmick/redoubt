//! A common timeout release of sixteen separate weight-100 spinner budgets, with a busy server
//! and 200 RTC/timeout wakes each from weight-1000 stand-ins, on one kernel-clock plan (H, R = H +
//! 50 ms, F) sent from one `time_now` reading. H only anchors R: the server counts from its go to
//! F, the spinners from R to F. The independent host post-check proves the actual W cluster and
//! rank/debt categories, then judges each attempt's envelope `[L, U)`.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::console::Console;
use test_programs::rd;
use test_programs::sched::{self, Bench, Role, Stats, join, rtc};

const SAMPLES: usize = 200;
const SPINNERS: usize = 16;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("cluster");
    let Some((rtc_mmio, _, rtc_irq)) = rtc::find(rd::OTHER_DEVICES..rd::first_free()) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCHED-CLUSTER")
    };
    b.set_entry(sched::cluster_child);
    // Readiness is acknowledged before the next start. Thus the first W of each newly created
    // one-thread budget is isolated and has the fixed order validated by the host trace check.
    let mut ids = [0usize; SPINNERS + 3];
    let mut budgets = [0u32; SPINNERS + 3];
    // Empty zero-weight scopes produce two existing X/Y trace fences only after measurement.
    // Root is not a measured parent; neither scope holds a thread, page or deadline.
    let markers = [
        rd::create(rd::ROOT, &rd::spec(0, 0, 0)).expect("driver trace fence"),
        rd::create(rd::ROOT, &rd::spec(0, 0, 0)).expect("timer trace fence"),
    ];
    for i in 0..ids.len() {
        let (parent, weight, role, extra) = if i == 0 {
            (rd::SYSTEM, 1000, Role::ClusterServer, &[][..])
        } else if i <= SPINNERS {
            (rd::USERS, 100, Role::ClusterSpinner, &[][..])
        } else if i == SPINNERS + 1 {
            (rd::SYSTEM, 1000, Role::ClusterDriver, &[rtc_mmio, rtc_irq, markers[0]][..])
        } else {
            (rd::SYSTEM, 1000, Role::ClusterTimer, &[markers[1]][..])
        };
        budgets[i] = b.budget(parent, weight, 1, rd::FOREVER);
        ids[i] = b.start(budgets[i], role, &[], extra);
        let ready = b.receive_report();
        b.check(
            ready.is_some_and(|(who, words)| who == ids[i] && words == [0; 4]),
            format_args!("child {i} acknowledged its isolated setup wake"),
        );
        let _ = writeln!(Console, "CLUSTER-READY {i}");
    }
    let (start, release, end) = b.go_cluster();
    let _ = writeln!(Console, "CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000");
    let _ = writeln!(Console, "CLUSTER-WINDOW {start} {release} {end}");
    let (words, seen) = b.collect_cluster_words(ids.len());
    for (i, &idx) in ids.iter().enumerate() {
        if i == SPINNERS + 1 || i == SPINNERS + 2 {
            let raw = words[idx][0];
            let _ = writeln!(
                Console,
                "CLUSTER-RAW role={} child={} seen={} words={} {} {} {} tag={} count={}",
                i,
                idx,
                seen[idx],
                raw[0],
                raw[1],
                raw[2],
                raw[3],
                raw[3] & 0xff,
                raw[3] >> 8
            );
            if raw[2] & 0x8000_0000 != 0 {
                let _ = writeln!(
                    Console,
                    "CLUSTER-FAIL role={} index={} branch={} result={} target_us={} current_us={}",
                    i,
                    (raw[2] >> 16) & 0xff,
                    (raw[2] >> 8) & 0xff,
                    raw[2] & 0xff,
                    raw[0] as u32 as i32,
                    raw[1] as u32 as i32,
                );
            }
        }
        if seen[idx] != 1 {
            b.check(false, format_args!("child {i} has {} reports (expected one)", seen[idx]));
        }
        if i == SPINNERS + 1 || i == SPINNERS + 2 {
            let tag = if i == SPINNERS + 1 { Stats::DRIVER_WAKE } else { Stats::TIMER_WAKE };
            b.check(
                seen[idx] == 1
                    && words[idx][0][2] & 0x8000_0000 == 0
                    && words[idx][0][3] & 0xff == tag
                    && words[idx][0][3] >> 8 == SAMPLES,
                format_args!("stand-in {i} took all {SAMPLES} real waits"),
            );
        } else {
            let work = join(words[idx][0][0], words[idx][0][1]);
            b.check(work != 0 && work != u64::MAX, format_args!("spinner/server {i} ran"));
        }
    }
    let server = ids[0];
    let server_work = join(words[server][0][0], words[server][0][1]);
    let spinner_work: u64 =
        ids[1..=SPINNERS].iter().map(|&idx| join(words[idx][0][0], words[idx][0][1])).sum();
    // Each count's own span: nothing waits for H, so the server's starts at its go.
    let _ = writeln!(
        Console,
        "CLUSTER-WORK server go..{end} {server_work} spinners {release}..{end} {spinner_work}"
    );
    for (idx, tag) in [(ids[SPINNERS + 1], Stats::DRIVER_WAKE), (ids[SPINNERS + 2], Stats::TIMER_WAKE)] {
        b.cluster_samples(idx, words[idx][0][3] >> 8, tag);
    }
    b.finish("SCHED-CLUSTER")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("cluster", info) }
