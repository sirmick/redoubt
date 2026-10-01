//! Budget destruction's cost does not grow with other budgets' objects or handles
//! (kernel/budgets.md, "Residual risks"; R10). A destruction walks the dying subtree and the
//! chains of the handles that depend on it, not every kernel-object frame or every live table, so
//! an unrelated budget's endpoints, and a live process's full table of handles to them, leave it
//! unchanged.
//!
//! Two medians of `budget_destroy`, call to return, of the same small two-level subtree: one on
//! a system holding almost nothing else, one while a filler budget owns thousands of endpoints and
//! its running filler holds a handle to each, a full table. The after time must stay within twice
//! the empty time and inside R10's 30 ms target; the old whole-frame scan, and the sweep of every
//! live table after it, grew with the filler and failed both. The case is a release build: a checked
//! build's post-walk index audit re-scans every object frame, which is what would be measured
//! (tests/scan-bounds.toml, "Checked builds").
//!
//! The launcher holds `root`, `system` and `users`; the filler is another copy of this program
//! (`test_programs::spawn`), running in its own budget.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::Bench;
use test_programs::spawn;

/// Destructions sampled before and after the fill; the median is judged.
const SAMPLES: usize = 9;
/// The filler budget's pages: more than a full table's endpoints, its 64 pages and the filler.
const FILL_PAGES: u64 = 8_000;
/// The least a full table holds, less the handles the filler was started with.
const MIN_ENDPOINTS: usize = 4_000;
/// R10's target (kernel/scheduling.md, "Responsiveness"), µs.
const TARGET_US: u64 = 30_000;
/// A floor under the empty time the ratio is taken against, so a few µs of noise on a
/// microsecond-scale measure do not decide it.
const FLOOR_US: u64 = 1_000;

/// The filler: endpoints in its own budget until its table is full, keeping each handle. It
/// reports how many it made on slot 1 and stays, its table live.
extern "C" fn filler(_: usize) -> ! {
    let mut made = 0;
    while rd::endpoint_create().is_ok() {
        made += 1;
    }
    let _ = rd::send(1, &rd::body([made, 0, 0, 0]), None, rd::FOREVER);
    test_programs::park()
}

/// Create a fresh two-level subtree under `users` and destroy it, returning how long the
/// destruction took (`budget_destroy`, call to return) in µs.
fn destroy_once() -> u64 {
    let victim = rd::create(rd::USERS, &rd::spec(8, 0, 0)).expect("the victim");
    let _grandchild = rd::create(victim, &rd::spec(4, 0, 0)).expect("the grandchild");
    let before = rd::time_now().unwrap();
    rd::destroy(victim).expect("the destruction");
    rd::time_now().unwrap() - before
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("destroy-growth");
    let mut empty = [0u64; SAMPLES];
    for sample in empty.iter_mut() {
        *sample = destroy_once();
    }
    let empty = median(&mut empty);
    b.note(format_args!("empty: budget_destroy {} us", empty));

    // The filler carves its own budget from `users` and fills its table with endpoints that
    // budget owns; it keeps running, so its full table is live while the subtree is destroyed.
    let report = rd::endpoint_create().expect("the report endpoint");
    let to_report = rd::mint_from_handle(report, 1, None).expect("a send right");
    let fill = rd::create(rd::USERS, &rd::spec(FILL_PAGES, 1, 100)).expect("the filler's budget");
    spawn::spawn(b.image(), fill, b.exit_endpoint(), filler as *const () as usize, &[], &[to_report])
        .expect("the filler");
    let made = match rd::receive(Some(report), 600_000_000, 0) {
        Ok(rd::Received::Message(m)) => m.body.words[0],
        _ => 0,
    };
    b.note(format_args!("the filler holds {} endpoints in another budget", made));
    b.check(made >= MIN_ENDPOINTS, format_args!("the filler filled its table with endpoints"));

    let mut full = [0u64; SAMPLES];
    for sample in full.iter_mut() {
        *sample = destroy_once();
    }
    let full = median(&mut full);
    let bound = 2 * empty.max(FLOOR_US);
    b.note(format_args!("filled: budget_destroy {} us (empty {} us, bound {})", full, empty, bound));
    b.check(
        full <= bound && full <= TARGET_US,
        format_args!("destruction does not grow with another budget's objects: {} us", full),
    );
    b.finish("BUDGET-DESTROY-GROWTH")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("destroy-growth", info) }
