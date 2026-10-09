//! A spinning budget cannot delay another beyond its weight (R12): three spinners in
//! budgets of weight 100, 100 and 300 each get their water-filling share of the harts, judged by
//! the post-check on the kernel's charges over a two-second window. At one hart that is their
//! weight's share; at two the 300 can use one hart, and the others share the second. Each
//! spinner's count of what the three counted is noted beside.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark};

const WINDOW: u64 = 2_000_000;
/// Allowed error, in thousandths of the CPU charged (R12's 50).
const TOL: u64 = 50;
/// The weights of the empty children that mark each spinner's budget in the kernel's trace.
const MARKS: [u32; 3] = [2, 3, 4];

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("share");
    let weights = [100u32, 100, 300];
    for (w, m) in weights.iter().zip(MARKS) {
        let budget = b.budget(rd::USERS, *w, 1, rd::FOREVER);
        mark(budget, m);
        b.start(budget, Role::Spin, &[], &[]);
    }
    let window = b.go(50_000, WINDOW);
    let counts = b.collect(weights.len());
    let counted: u64 = counts[..weights.len()].iter().sum();
    for (i, w) in weights.iter().enumerate() {
        let others: [(u32, u32); 2] = core::array::from_fn(|j| (weights[(i + 1 + j) % 3], 1));
        b.hart_share(&["share-a", "share-b", "share-c"][i], window, (TOL, ""), (MARKS[i], 1), &others);
        // Under icount a count is the machine's instructions, not the hart's time: a note.
        b.note(format_args!(
            "weight {} counted {} of 1000 of what the three counted ({} of the window)",
            w,
            counts[i] * 1000 / counted.max(1),
            b.share(counts[i], window.1 - window.0)
        ));
    }
    // What the three counted of what the calibrated rate fills: the efficiency the slice ends
    // leave, reported with no verdict.
    b.note(format_args!("counted {} of the calibrated 1000", b.share(counted, window.1 - window.0)));
    b.finish("SCHED-SHARE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("share", info) }
