//! A hart waiting for the kernel lock halts, and costs the hart holding it nothing but its wake
//! (kernel/scheduling.md, R78; `bench:smp-lock-wait` at two harts under `icount`,
//! `bench:smp-lock-wait-4-mttcg` at four where the harts run at once).
//!
//! `threads` threads of one process make `CALLS` system calls each, at once, one on each hart, so
//! nearly every call waits for another's to leave the kernel; then one thread makes all `threads x
//! CALLS` alone, the other harts idle. Under `icount` QEMU runs the harts in turn and its clock
//! counts every running hart's instructions, so a hart that spins for the lock spends the turns
//! and the time the holder needs: with the spin (`sched-spin-entry`) the calls at once take a
//! hundred times and more what they take in turn. A hart that halts gives its turns up, and pays
//! only for being woken, an interrupt through the firmware at each hand-off. Where the harts run
//! at once, every hand-off is a halted hart's wake, and a release that woke every waiting hart
//! would wake two that halt again for each it serves at four harts.

use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use crate::rd;
use crate::sched::Bench;

/// System calls per thread.
const CALLS: usize = 20_000;

/// The threads other than the first whose calls are done.
static DONE: AtomicUsize = AtomicUsize::new(0);

fn calls(n: usize) {
    for _ in 0..n {
        rd::time_now().expect("time_now");
    }
}

fn other(_: usize) {
    calls(CALLS);
    DONE.fetch_add(1, SeqCst);
}

/// Run the calls of `threads` threads at once, then in turn on one, and require the first to take
/// at most `tenths` tenths of the second; `name` ends the run (`<name> TEST PASSED`).
pub fn run(name: &str, threads: usize, tenths: u64) -> ! {
    let mut b = Bench::new("lock-wait");

    let start = b.now_us();
    for _ in 1..threads {
        rd::thread(other, 0).expect("another thread");
    }
    calls(CALLS);
    // Wait halted, not spinning: a spinning thread here would take the other harts' turns too.
    while DONE.load(SeqCst) < threads - 1 {
        rd::receive(None, 100, 0).ok();
    }
    let at_once = b.now_us() - start;

    let start = b.now_us();
    calls(threads * CALLS);
    let in_turn = b.now_us() - start;

    b.check(
        at_once * 10 <= in_turn * tenths,
        format_args!(
            "{} calls on {} harts at once took {} µs, {} in turn on one (at most {}.{} times it)",
            threads * CALLS,
            threads,
            at_once,
            in_turn,
            tenths / 10,
            tenths % 10
        ),
    );
    b.finish(name)
}
