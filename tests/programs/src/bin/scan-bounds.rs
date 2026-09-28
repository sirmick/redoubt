//! A call's kernel time does not grow with what other budgets hold (kernel/scheduling.md R12):
//! creating a process (the PID draw), taking its exit notice, and an interrupt finding its IRQ
//! object are lookups, not scans of every kernel-object frame. Measured on an empty system, then
//! after one budget has filled its page limit with endpoints, which raises the object frames by
//! that many; each must stay within twice its empty time and within the wake target.
//!
//! This program runs first, so it holds the goldfish RTC and its interrupt; every time is the
//! kernel's (`time_now`) or the RTC's, in virtual time.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, Received};
use test_programs::sched::{Bench, rtc};
use test_programs::spawn;

/// Samples of each measure; the median is judged.
const SAMPLES: usize = 16;
/// The filler budget's pages: that many endpoint frames, at most.
const FILL_PAGES: u64 = 20_000;
/// The wake target (kernel/scheduling.md, "Responsiveness"): p50 15 ms.
const TARGET_US: u64 = 15_000;
/// A floor under the empty time the ratio is taken against, so a few µs of noise on a
/// microsecond-scale measure do not decide it.
const FLOOR_US: u64 = 100;

/// A child that exits at once.
extern "C" fn exiter(_: usize) -> ! { rd::process_exit(0) }

/// The filler: endpoints in its own budget until it is refused, closing each handle (an endpoint
/// lives until its budget does). Exits with how many it made.
extern "C" fn filler(_: usize) -> ! {
    let mut made = 0;
    while let Ok(h) = rd::endpoint_create() {
        let _ = rd::close(h);
        made += 1;
    }
    rd::process_exit(made)
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// (process_create to its exit notice, interrupt delivery), medians in µs.
fn measure(b: &Bench, kids: u32, base: usize, irq: u32) -> (u64, u64) {
    let mut create = [0u64; SAMPLES];
    for sample in create.iter_mut() {
        let t0 = b.now_us();
        spawn::spawn(b.image(), kids, b.exit_endpoint(), exiter as *const () as usize, &[], &[])
            .expect("a child");
        match rd::receive(Some(b.exit_endpoint()), 5_000_000, 0) {
            Ok(Received::Exit(n)) if n.cause == Cause::Exited => {}
            other => panic!("expected the child's exit notice, got {:?}", other.map(|_| ())),
        }
        *sample = b.now_us() - t0;
    }
    let mut late = [0u64; SAMPLES];
    for sample in late.iter_mut() {
        let at = rtc::now_ns(base) + 1_000_000;
        rtc::alarm(base, at);
        let fired = matches!(rd::receive(Some(irq), 100_000, 0), Ok(Received::Interrupt));
        *sample = if fired { rtc::now_ns(base).saturating_sub(at) / 1000 } else { u64::MAX };
        rtc::clear(base);
    }
    (median(&mut create), median(&mut late))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::log_rx();
    let mut b = Bench::new("scan-bounds");
    let Some((_, base, irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCAN-BOUNDS")
    };
    // Each budget runs one process at a time, whose PID counts there until its notice is taken.
    let kids = rd::create(rd::USERS, &rd::spec(600, 1, 100)).expect("the children's budget");
    let (create0, irq0) = measure(&b, kids, base, irq);
    b.note(format_args!("empty: process_create to notice {} us, interrupt {} us", create0, irq0));

    let fill = rd::create(rd::USERS, &rd::spec(FILL_PAGES, 1, 100)).expect("the filler's budget");
    spawn::spawn(b.image(), fill, b.exit_endpoint(), filler as *const () as usize, &[], &[])
        .expect("the filler");
    let made = match rd::receive(Some(b.exit_endpoint()), 600_000_000, 0) {
        Ok(Received::Exit(n)) => n.code,
        _ => 0,
    };
    b.note(format_args!("the filler made {} endpoints", made));
    b.check(u64::from(made) > FILL_PAGES / 2, format_args!("the filler filled its budget with endpoints"));

    let (create1, irq1) = measure(&b, kids, base, irq);
    b.note(format_args!("filled: process_create to notice {} us, interrupt {} us", create1, irq1));
    for (what, empty, full) in [("process_create to notice", create0, create1), ("interrupt", irq0, irq1)] {
        let bound = 2 * empty.max(FLOOR_US);
        b.check(
            full <= bound && full <= TARGET_US,
            format_args!("{} stays bounded: {} us against {} us empty", what, full, empty),
        );
    }
    b.finish("SCAN-BOUNDS")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("scan-bounds", info) }
