//! A spinning budget cannot delay another beyond its weight (R12): three spinners in
//! budgets of weight 100, 100 and 300 each get their weight's share of the CPU the three counted
//! over a two-second window.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
/// Allowed error, in thousandths of the CPU counted.
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("share");
    let weights = [100u64, 100, 300];
    let total: u64 = weights.iter().sum();
    for w in weights {
        let budget = b.budget(rd::USERS, w as u32, 1, rd::FOREVER);
        b.start(budget, Role::Spin, &[], &[]);
    }
    let (start, end) = b.go(50_000, WINDOW);
    let counts = b.collect(weights.len());
    // Each slice's end costs kernel time (the switch, the reconcile and, in a checked build, the
    // audit after it) that no count measures and that every budget pays per slice it runs, so
    // each share is judged of what the three counted: the window's gross is noted beside.
    let counted: u64 = counts[..weights.len()].iter().sum();
    for (i, w) in weights.iter().enumerate() {
        let share = counts[i] * 1000 / counted.max(1);
        let want = w * 1000 / total;
        b.check(
            share.abs_diff(want) <= TOL,
            format_args!(
                "weight {} got {} of 1000 counted ({} of the window), want {} +- {}",
                w,
                share,
                b.share(counts[i], end - start),
                want,
                TOL
            ),
        );
    }
    // What the three counted of what the calibrated rate fills: the efficiency the slice ends
    // leave, reported with no verdict.
    b.note(format_args!("counted {} of the calibrated 1000", b.share(counted, end - start)));
    b.finish("SCHED-SHARE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("share", info) }
