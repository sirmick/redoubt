//! A device interrupt only wakes, and its handling is its device's owner's (kernel/scheduling.md,
//! "Preemption points" and "Charging"): the driver it wakes runs when the queue next picks it,
//! never by taking the hart from the thread it interrupted; and the kernel's time from the claim
//! to the bill is charged to the budget the device object is charged to (`system`, which the boot
//! charges every device to and gives `init`), never to the budget interrupted.
//!
//! A driver stand-in on the goldfish RTC's alarm, each alarm a little over 2 ms ahead at phases
//! spread over a millisecond, and a spinner in an equal-weight budget, which runs whenever the
//! driver waits: every alarm interrupts the spinner mid-slice. The verdicts are the post-check's,
//! from the trace: `irq_wake_no_preempt=N` needs N device interrupts that woke the driver and
//! returned to the spinner, which ran to its slice's end before the driver was picked;
//! `irq_billed_to=<mark>` needs every interrupt that claimed its source to charge the marked
//! budget, `system`, for its handling. One hart: on two the idle hart takes the wake, and there
//! is nothing to preempt.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, Stats, mark, rtc};

/// Alarms the driver waits for.
const ALARMS: u64 = 60;
/// The mark on the devices' owner, `system`.
const OWNER_MARK: u32 = 2;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::first_free();
    let mut b = Bench::new("irq-wake");
    let Some((rtc_mmio, _, rtc_irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("SCHED-IRQ-WAKE")
    };
    // The trace names the devices' owner by this mark: a carve under `system`, destroyed.
    mark(rd::SYSTEM, OWNER_MARK);
    let driver = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    let d = b.start(driver, Role::Driver, &[ALARMS, 0], &[rtc_mmio, rtc_irq]);
    let s = b.start(spinner, Role::Spin, &[], &[]);
    let (start, end) = b.go(50_000, 400_000);
    // The spinner's count, the driver's wake summary and its lost alarms.
    let words = b.collect_words(1 + 2);
    let stat = |i: usize, tag: usize| words[i].iter().find(|w| w[3] & 0xff == tag && w[3] >> 8 > 0).copied();
    let delivered = stat(d, Stats::DRIVER_WAKE).map_or(0, |w| w[3] >> 8);
    let lost = stat(d, Stats::DRIVER_LOST).map_or(0, |w| w[0]);
    b.check(
        delivered as u64 >= ALARMS,
        format_args!(
            "the driver took {} alarms ({} never delivered: R5; todo/irq-level-latch.md)",
            delivered, lost
        ),
    );
    let count = words[s][0][0] as u32 as u64 | (words[s][0][1] as u32 as u64) << 32;
    b.note(format_args!(
        "the spinner counted {} of 1000 of the window; which budget each interrupt woke and billed is the post-check's",
        b.share(count, end - start)
    ));
    rd::destroy(driver).unwrap();
    rd::destroy(spinner).unwrap();
    b.finish("SCHED-IRQ-WAKE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("irq-wake", info) }
