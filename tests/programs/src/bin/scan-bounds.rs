//! A call's kernel time does not grow with what other budgets hold (kernel/scheduling.md R12):
//! creating a process (the PID draw), taking its exit notice, and an interrupt finding its IRQ
//! object are lookups, not scans of every kernel-object frame; and taking or giving back a RAM
//! frame is no search of RAM, so a one-page `map_anon`, a `budget_create` and a `process_create`
//! that runs out of pages halfway through building the space (and is rolled back) cost the same
//! however much RAM is in use. Measured on an empty system, then after one budget has filled its
//! page limit with endpoints, which raises the object frames and the RAM in use by that many;
//! each must stay within twice its empty time and within the wake target.
//!
//! This program runs first, so it holds the goldfish RTC and its interrupt; every time is the
//! kernel's (`time_now`) or the RTC's, in virtual time.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, Error, Received};
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
/// lives until its budget does). It sends how many it made on the handle in its slot 1 and stays,
/// so its own space is never given back: a frame freed below the fill would let a first-fit search
/// of RAM stop short of it.
extern "C" fn filler(_: usize) -> ! {
    let mut made = 0;
    while let Ok(h) = rd::endpoint_create() {
        let _ = rd::close(h);
        made += 1;
    }
    let _ = rd::send(1, &rd::body([made, 0, 0, 0]), None, rd::FOREVER);
    test_programs::park()
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// The pages of the budget a rolled-back `process_create` builds in: the root table and a saved
/// context, not the rest of the space.
const TIGHT_PAGES: u64 = 2;

/// What is measured, in the order [`measure`] returns it.
const MEASURES: [&str; 5] = [
    "process_create to notice",
    "interrupt",
    "map_anon of a page",
    "budget_create",
    "rolled-back process_create",
];

/// Each of [`MEASURES`], its median in µs. `tight` is a budget of [`TIGHT_PAGES`].
fn measure(b: &mut Bench, kids: u32, base: usize, irq: u32, tight: u32) -> [u64; 5] {
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
    let mut map = [0u64; SAMPLES];
    for sample in map.iter_mut() {
        let t0 = b.now_us();
        let at = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("a page");
        *sample = b.now_us() - t0;
        rd::unmap(at, rd::PAGE_SIZE).expect("the page just mapped");
    }
    let mut budget = [0u64; SAMPLES];
    for sample in budget.iter_mut() {
        let t0 = b.now_us();
        let made = rd::create(rd::USERS, &rd::spec(1, 1, 1)).expect("a budget");
        *sample = b.now_us() - t0;
        rd::destroy(made).expect("the budget just made");
    }
    let (mut refused, mut wrong) = ([0u64; SAMPLES], 0);
    for sample in refused.iter_mut() {
        let t0 = b.now_us();
        let made = rd::process_create(tight, b.exit_endpoint());
        *sample = b.now_us() - t0;
        wrong += usize::from(made != Err(Error::OutOfMemory));
    }
    // The kernel's own count: every page the rolled-back creations took went back.
    let left = rd::usage(tight).map(|u| u.pages_usage);
    b.check(
        wrong == 0 && left == Ok(0),
        format_args!(
            "a rolled-back process_create is refused and costs nothing ({} not refused, {:?} pages left)",
            wrong, left
        ),
    );
    [median(&mut create), median(&mut late), median(&mut map), median(&mut budget), median(&mut refused)]
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::first_free();
    let mut b = Bench::new("scan-bounds");
    let Some((_, base, irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCAN-BOUNDS")
    };
    // Each budget runs one process at a time, whose PID counts there until its notice is taken.
    let kids = rd::create(rd::USERS, &rd::spec(600, 1, 100)).expect("the children's budget");
    let tight = rd::create(rd::USERS, &rd::spec(TIGHT_PAGES, 1, 1)).expect("the tight budget");
    let empty = measure(&mut b, kids, base, irq, tight);
    for (what, us) in MEASURES.iter().zip(empty) {
        b.note(format_args!("empty: {} {} us", what, us));
    }

    let fill = rd::create(rd::USERS, &rd::spec(FILL_PAGES, 1, 100)).expect("the filler's budget");
    let ep = rd::endpoint_create().expect("the filler's report endpoint");
    let tell = rd::mint_from_handle(ep, 1, None).expect("a send handle");
    spawn::spawn(b.image(), fill, b.exit_endpoint(), filler as *const () as usize, &[], &[tell])
        .expect("the filler");
    let made = match rd::receive(Some(ep), 600_000_000, 0) {
        Ok(Received::Message(m)) => m.body.words[0] as u64,
        _ => 0,
    };
    b.note(format_args!("the filler made {} endpoints", made));
    b.check(made > FILL_PAGES / 2, format_args!("the filler filled its budget with endpoints"));

    let filled = measure(&mut b, kids, base, irq, tight);
    for ((what, empty), full) in MEASURES.iter().zip(empty).zip(filled) {
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
