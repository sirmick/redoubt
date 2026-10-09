//! A hart waiting for the kernel lock does not take the guest's time from the hart holding it
//! (kernel/scheduling.md, R78; `bench:smp-lock-wait`), at 2 harts under `icount`.
//!
//! Two threads of one process make `CALLS` system calls each, at once, one on each hart, so every
//! call but the first waits for the other's to leave the kernel; then one thread makes all `2 x
//! CALLS` alone, the other hart idle. Under `icount` QEMU runs the harts in turn and its clock
//! counts every running hart's instructions, so a hart that spins for the lock spends the turns
//! and the time the holder needs: with the spin (`sched-spin-entry`) the calls at once take a
//! hundred times and more what they take in turn. A hart that halts gives its turns up, and pays
//! only for being woken, an interrupt through the firmware at each hand-off: about one and a half
//! times on rv64 and two and a half on rv32, every call here contended. The bound is three times.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use test_programs::rd;
use test_programs::sched::Bench;

/// System calls per thread.
const CALLS: usize = 20_000;
/// The most the calls at once may take, in multiples of what they take in turn.
const BOUND: u64 = 3;

/// Set by the second thread when its calls are done.
static DONE: AtomicUsize = AtomicUsize::new(0);

fn calls(n: usize) {
    for _ in 0..n {
        rd::time_now().expect("time_now");
    }
}

fn second(_: usize) {
    calls(CALLS);
    DONE.store(1, SeqCst);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("lock-wait");

    let start = b.now_us();
    rd::thread(second, 0).expect("the second thread");
    calls(CALLS);
    // Wait halted, not spinning: a spinning thread here would take the other hart's turns too.
    while DONE.load(SeqCst) == 0 {
        rd::receive(None, 100, 0).ok();
    }
    let at_once = b.now_us() - start;

    let start = b.now_us();
    calls(2 * CALLS);
    let in_turn = b.now_us() - start;

    b.check(
        at_once <= in_turn * BOUND,
        format_args!(
            "{} calls on two harts at once took {} µs, {} in turn on one (at most {} times it)",
            2 * CALLS,
            at_once,
            in_turn,
            BOUND
        ),
    );
    b.finish("SMP-LOCK-WAIT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("lock-wait", info) }
