//! Sleeping and waking cannot buy more than a budget's weight (WP-K5; R12): against an
//! equal-weight spinner, a gamer whose two threads each run nearly a slice (or a few
//! microseconds) and then sleep a microsecond, and one that sleeps long and wakes for bursts,
//! get at most half; the victim at least half.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("gaming");
    let tpu = b.tpu;
    // (burst in ticks, nap in µs, threads, what)
    let phases = [
        ((SLICE_US - SLICE_US / 10) * tpu, 1, 2, "near-slice bursts, 1 us naps"),
        (20 * tpu, 1, 2, "20 us bursts, 1 us naps"),
        (100_000 * tpu, 300_000, 1, "100 ms bursts after 300 ms sleeps"),
    ];
    for (burst, nap, threads, what) in phases {
        let gamer = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let g = b.start(gamer, Role::Gamer, &[burst, nap, threads], &[]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let (start, end) = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        let (gs, vs) = (b.share(counts[g], end - start), b.share(counts[v], end - start));
        b.check(
            vs + TOL >= 500 && gs <= 500 + TOL,
            format_args!("{}: gamer {} of 1000, victim {}", what, gs, vs),
        );
        rd::destroy(gamer).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-SLEEP-GAMING")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("gaming", info) }
