//! Sleeping and waking cannot buy more than a budget's weight (R12): against an
//! equal-weight spinner, a gamer whose two threads each run nearly a slice (or a few
//! microseconds) and then sleep a microsecond, and one that sleeps long and wakes for bursts,
//! get at most their share of the harts; the victim at least its share. The shares are the
//! post-check's, of the kernel's charges net of lock waits (`HART-SHARE`); the counts are noted
//! beside.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US, mark};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("gaming");
    let tpu = b.tpu;
    // (burst in ticks, nap in µs, threads, what, the victim's and the gamer's share names and
    // marks)
    let phases = [
        (
            (SLICE_US - SLICE_US / 10) * tpu,
            1,
            2,
            "near-slice bursts, 1 us naps",
            ("near-slice-victim", "near-slice-gamer"),
            (2, 5),
        ),
        (20 * tpu, 1, 2, "20 us bursts, 1 us naps", ("20us-victim", "20us-gamer"), (3, 6)),
        (
            100_000 * tpu,
            300_000,
            1,
            "100 ms bursts after 300 ms sleeps",
            ("100ms-victim", "100ms-gamer"),
            (4, 7),
        ),
    ];
    for (burst, nap, threads, what, (vn, gn), (vm, gm)) in phases {
        let gamer = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        mark(victim, vm);
        mark(gamer, gm);
        let g = b.start(gamer, Role::Gamer, &[burst, nap, threads], &[]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        let k = threads as u32;
        b.hart_share(vn, window, (TOL, "+"), (vm, 1), &[(100, k)]);
        b.hart_share(gn, window, (TOL, "-"), (gm, k), &[(100, 1)]);
        b.note(format_args!(
            "{}: the gamer counted {} of 1000 of the window, the victim {}",
            what,
            b.share(counts[g], window.1 - window.0),
            b.share(counts[v], window.1 - window.0)
        ));
        rd::destroy(gamer).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.finish("SCHED-SLEEP-GAMING")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("gaming", info) }
