//! A process's exit recomputes the timer's hint once, however many of its waits held it
//! (kernel/timer.md, "The hart timer"), so its kernel time does not grow with the processes that
//! exist (kernel/scheduling.md, R12). A child with `MAX_THREADS` threads exits with all but its
//! first thread waiting, with deadlines that rise by thread, every one earlier than any other
//! timeout in the system; the exit ends the waits lowest thread first, so each one it ends held the
//! hint. It exits once on an empty system, and again after this program has filled every other PID
//! with a child in a budget of its own, waiting with a timeout later than all of the exiting
//! child's, so that the hint's recomputation reads one cache per PID and lowers the hint by each.
//! The bench's post-check (`EXIT-SECTIONS`, from a `hold-trace` kernel) holds the second exit's
//! kernel lock section, net of the checked build's audits, to the first's plus two slices; a hint
//! recomputed at each wait's end reads every process's cache 254 times. This program's own checks
//! say only that the processes and the waits were all there.

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use redoubt_sys::MAX_THREADS;
use test_programs::console::Console;
use test_programs::rd::{self, Cause, Received};
use test_programs::sched::{Bench, SLICE_US};
use test_programs::spawn;

/// `system`'s and `users'` processes at boot (kernel/budgets.md, "The tree from the boot
/// manifest"): with this program's one in `root`, every PID but the kernel's. The last of `users'`
/// is the exiting child's.
const SYSTEM: usize = 127;
const USERS: usize = 382;
/// How far ahead the first wait's deadline is, µs: an hour, after every wait has blocked and the
/// process has exited, however long its threads' start takes. Each next thread's is a millisecond
/// later.
const AHEAD: u64 = 3_600_000_000;
/// How long a child's start, or the exit, may take: under icount, seconds.
const REPORT: u64 = 600_000_000;
/// A filler's timeout, µs: two hours, later than every wait of the exiting child's, so each filler's
/// cache is one the hint's recomputation weighs.
const LATER: u64 = 7_200_000_000;
/// The exiting child's wait for its threads to block, µs. At 1 ms the case stalls at full occupancy
/// on a kernel whose expiry reads every timed wait of the process: the sleep's own call holds the
/// kernel 1.4 ms (0.87 ms of it a checked build's audit), so its timeout is due when the one thread
/// not yet run is picked, and the timer interrupt's expiry, 1,327 µs over the 253 waits, falls in
/// that thread's slice, which then ends after 19 µs of its own, every turn (kernel/scheduling.md,
/// R12). At 10 ms the guard reaches its verdict there too, and passes.
const POLL: u64 = 10_000;
/// Pages a filler's budget has above the probe's usage, for what its start takes and gives back.
const SPARE: u64 = 16;
/// The exiting child's threads that have taken their deadline, about to wait.
static WAITING: AtomicUsize = AtomicUsize::new(0);
/// The first wait's deadline, µs, set by the exiting child before it starts its threads.
static FIRST: AtomicUsize = AtomicUsize::new(0);

/// A filler: a process in a PID, waiting until [`LATER`] from now, and again.
extern "C" fn filler(_: usize) -> ! {
    loop {
        let _ = rd::receive(None, LATER, 0);
    }
}

/// One of the exiting child's threads: a wait until its own deadline, `i` ms after the first.
extern "C" fn waiter(i: usize) -> ! {
    let first = FIRST.load(SeqCst) as u64;
    WAITING.fetch_add(1, SeqCst);
    let now = rd::time_now().expect("the clock");
    let _ = rd::receive(None, (first + i as u64 * 1_000).saturating_sub(now), 0);
    test_programs::park()
}

/// The exiting child: every other thread waiting, then its exit. It has no console: a thread it
/// cannot make, or a deadline already passed, exits with a code the parent reports.
extern "C" fn exiter(_: usize) -> ! {
    let first = rd::time_now().expect("the clock") + AHEAD;
    FIRST.store(first as usize, SeqCst);
    let stacks = match rd::map_anon_run((MAX_THREADS - 1) * rd::PAGE_SIZE, rd::rw()) {
        Ok(at) => at,
        Err(e) => rd::process_exit(1000 + e as u32),
    };
    for i in 1..MAX_THREADS {
        let stack = stacks + i * rd::PAGE_SIZE - 16;
        if let Err(e) = rd::thread_create(waiter as *const () as usize, stack, i - 1) {
            rd::process_exit(2000 + e as u32);
        }
    }
    // Every thread has its deadline; a sleep, earlier than all of them, lets the last block.
    while WAITING.load(SeqCst) < MAX_THREADS - 1 {
        let _ = rd::receive(None, POLL, 0);
    }
    let _ = rd::receive(None, POLL, 0);
    let late = rd::time_now().expect("the clock") >= first;
    rd::process_exit(if late { 3 } else { 0 })
}

/// Start `entry` in `budget` and close the handle to it.
fn start(b: &Bench, budget: u32, entry: extern "C" fn(usize) -> !) {
    let c = spawn::spawn(b.image(), budget, b.exit_endpoint(), entry as *const () as usize, &[], &[])
        .expect("a child");
    rd::close(c.process).expect("close the child's handle");
}

/// Start the exiting child in `room` and take its notice: the window, in the kernel's ticks counted
/// from its boot as the sections are, and its exit code.
fn exit_once(b: &Bench, room: u32) -> (u64, u64, u32) {
    let start_at = b.now_us() * b.tpu;
    start(b, room, exiter);
    let code = match rd::receive(Some(b.exit_endpoint()), REPORT, 0) {
        Ok(Received::Exit(n)) if n.cause == Cause::Exited => n.code,
        other => panic!("expected the exiting child's notice, got {:?}", other.map(|_| ())),
    };
    (start_at, (b.now_us() + 1) * b.tpu, code)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("timeout-hint-exit");
    let room = rd::create(rd::USERS, &rd::spec(4096, 1, 1)).expect("the exiting child's budget");
    let (s0, e0, empty) = exit_once(&b, room);
    // What a filler costs its budget, measured on a probe.
    let probe = rd::create(rd::USERS, &rd::spec(4096, 1, 1)).expect("the probe's budget");
    start(&b, probe, filler);
    let _ = rd::receive(None, 100_000, 0);
    let each = rd::usage(probe).expect("the probe's usage").pages_usage;
    rd::destroy(probe).expect("destroy the probe");
    match rd::receive(Some(b.exit_endpoint()), REPORT, 0) {
        Ok(Received::Exit(n)) if n.cause == Cause::Killed => {}
        other => panic!("expected the probe's notice, got {:?}", other.map(|_| ())),
    }
    let mut fillers = 0;
    for (parent, n) in [(rd::SYSTEM, SYSTEM), (rd::USERS, USERS - 1)] {
        for _ in 0..n {
            let budget = rd::create(parent, &rd::spec(each + SPARE, 1, 1)).expect("a filler's budget");
            start(&b, budget, filler);
            fillers += 1;
        }
    }
    b.note(format_args!("{fillers} fillers of {each} pages"));

    let (s1, e1, filled) = exit_once(&b, room);
    b.check(
        empty == 0 && filled == 0,
        format_args!(
            "{} threads waited, each deadline still to come at both exits ({empty}, {filled})",
            MAX_THREADS - 1
        ),
    );
    // Two slices, the kernel's `SLICE_US`, in ticks: twice the most measured, 0.68 of a slice on
    // rv32, rounded up to whole slices.
    let _ = writeln!(Console, "EXIT-SECTIONS {s0} {e0} {s1} {e1} {}", 2 * SLICE_US * b.tpu);
    b.finish("TIMEOUT-HINT-EXIT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("timeout-hint-exit", info) }
