//! The scheduling cases (kernel/budgets.md R7; kernel/scheduling.md R12).
//!
//! Each case is one program, the bundle's only one, and a launcher: it holds `root`, `system` and
//! `users`, makes budgets, starts children in them (copies of itself, [`crate::spawn`]), and
//! judges each share from what they report. It prints on the UART itself.
//!
//! **Work, not time.** Every child that competes for the CPU runs the same counting loop
//! ([`spin_until`]), which checks the clock through `rdtime` (no system call) every
//! `CHUNK` iterations. The launcher first runs that loop alone, to learn how many iterations a
//! millisecond of CPU is ([`Bench::rate`]); a child's share of a window is then its count over what
//! the whole window would have given one loop alone. The cases run in virtual time (`icount`), so
//! the numbers do not depend on the host.
//!
//! **Starting together.** Children are started one by one (each start copies this image), then
//! each is sent a window `[start, end]` in `rdtime` ticks; they sleep until `start`, so every
//! window begins at the same instant. The cluster case alone sends its window in `time_now`
//! microseconds, from one kernel reading ([`Bench::go_cluster`]), and its children check that
//! clock between chunks (`cluster_spin_until`).

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use crate::console::{self, Console};
use crate::rd::{self, Error, Received};
use crate::spawn::{self, Image};

/// Iterations between clock checks in the counting loop.
const CHUNK: u64 = 256;
/// The slice (kernel/scheduling.md, "Preemption points"), in microseconds.
pub const SLICE_US: u64 = 1_000;

// The latency workload (kernel/scheduling.md, "Responsiveness"), one definition used by both
// `sched-latency` and `kernel-containment`. The targets are the case files' post-check bounds.

/// Share tolerance, in thousandths.
pub const TOL: u64 = 30;
/// Driver alarms and steward timeouts per latency run (p99 is then the second largest).
pub const K: u64 = 200;
/// Leases per latency run the steward destroys by hand, and as many by deadline.
pub const LEASES: u64 = 50;
/// The latency case's window, µs.
pub const WINDOW_US: u64 = 16_000_000;
/// R10's kernel time target, µs, for `budget_destroy`'s recorded bound (the post-check judges R10
/// and every latency target, net of the checked build's audits).
pub const R10_P99: usize = 30_000;

/// What a child does. Its startup block is the role, then up to eight `u64` parameters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Role {
    /// Count until the window ends; report the count.
    Spin = 1,
    /// `threads` threads, each: count for `burst` ticks, sleep `nap` µs, over the window; report
    /// the total. p0 = burst ticks, p1 = nap µs, p2 = threads.
    Gamer = 2,
    /// Serve calls on slot 3, counting for p0 µs per call, until the window ends; report the
    /// work done.
    Server = 3,
    /// p2 threads calling slot 3 for the whole window; report the calls made.
    Flood = 4,
    /// Churn threads: a thread counts for p0 ticks then exits straight from the count, over and
    /// over; report the total.
    ThreadChurn = 5,
    /// Churn processes in the budget in slot 3: a child counts for p0 ticks then exits (p1 = 0)
    /// or faults (p1 = 1) straight from the count; report how many ran.
    ProcessChurn = 6,
    /// A churn child: count p0 ticks, report to slot 1 unless p2 = 1, exit or fault.
    ChurnChild = 7,
    /// Budget churn under the budget in slot 3 (the child's own): p0 = variant (0 blocking,
    /// 1 spinning parent destroying at its slice's end, 2 destroyed by a deadline just after the
    /// parent's slice, 3 fresh intermediates); p1 = child weight. Report the total.
    BudgetChurn = 8,
    /// p2 threads each sleeping p0 µs (staggered by p1 µs per thread), re-arming, for the window;
    /// with p3 = 1, also create budgets with staggered deadlines under slot 3. With p3 = 2, the
    /// waits end early instead: p2 - 1 threads each wait on an endpoint of their own with a
    /// timeout of p0 µs (staggered by p1 µs per thread), and a sibling sends to each at once;
    /// report the waits answered. Report 0 otherwise.
    TimerFlood = 9,
    /// Report the time (µs) it first runs after the window's start, then exit.
    Probe = 10,
    /// Sleep until p0 ticks, then count until the window ends; report the count.
    SpinFrom = 11,
    /// Count until p0 ticks (not reported), sleep until p1 ticks, then count until the window
    /// ends; report that last count.
    SpinGap = 12,
    /// `send` through slot 3 until it fails; report the `rdtime` it first runs again (0 unless
    /// the send failed `Dead`), then exit.
    TieSender = 13,
    /// `receive` on slot 3; report the `rdtime` it first runs again, then exit.
    TieReceiver = 16,
    /// p1 times: sleep p0 µs, and measure how long after its deadline it runs again; report the
    /// shortest such delay, in µs.
    WakeDelay = 17,
    /// Destroy the budget in slot 3, then count until the window ends; report, first, how long the
    /// destruction took and the longest this thread then went without the CPU (µs), then the
    /// count.
    DestroyThenCount = 18,
    /// The latency case's driver stand-in: p0 samples of the goldfish RTC's alarm (MMIO in slot
    /// 3, interrupt in slot 4), each waited for in `receive`; report [`Stats::DRIVER_WAKE`]. With
    /// p1 = 1, hold each sample's window until the launcher asks ([`Bench::samples`]).
    Driver = 14,
    /// The latency case's steward stand-in, with leases carved from slot 3: p0 timeout wakes,
    /// p1 leases destroyed by hand (each after a timeout: its decision) and p1 destroyed by their
    /// deadlines; report [`Stats::TIMER_WAKE`], [`Stats::DESTROY`], [`Stats::DECISION_WAKE`] and
    /// [`Stats::DEADLINE`]. With p2 = 1, hold each wake's and notice's window until the launcher
    /// asks ([`Bench::samples`]).
    Steward = 15,
    /// Rounds of p0 empty weight-0 budgets under slot 3, their deadlines 1 µs apart from 200 µs
    /// ahead, each round then counting for 1 ms; report the count.
    DeadlineFlood = 19,
    /// Sleep, so that what follows begins a slice; carve p0 of this budget's weight (slot 3) to an
    /// empty child, count until nine tenths of a slice after the wake, then destroy it so the
    /// weight comes back. Count until the window ends; report create and return offsets (µs),
    /// `time_now` of the return, then that last count.
    CarveSpin = 20,
    /// The containment gate's hostile agent, run by the steward in a lease (kernel/README.md,
    /// "Containment").
    Agent = 21,
    /// The agent's sub-agent: bounded thread and process churn in the sub-budget.
    SubAgent = 22,
    /// The containment gate's victim server.
    Victim = 23,
    /// The containment gate's bystander: its share, and the queued stamped handle.
    Bystander = 24,
    /// The containment gate's steward stand-in.
    Containment = 25,
    /// The endpoint maker, run once in `users`.
    EndpointMaker = 26,
    /// Cluster fixture: one absolute release timeout, then ordinary spinning.
    ClusterSpinner = 27,
    /// Cluster fixture: spin across the spinners' common release.
    ClusterServer = 28,
    /// Cluster fixture: 200 goldfish RTC interrupt waits with fixed programmed offsets.
    ClusterDriver = 29,
    /// Cluster fixture: 200 timeout waits with the same programmed offsets.
    ClusterTimer = 30,
}

impl Role {
    fn from_u8(x: u8) -> Option<Role> {
        use Role::*;
        [
            Spin,
            Gamer,
            Server,
            Flood,
            ThreadChurn,
            ProcessChurn,
            ChurnChild,
            BudgetChurn,
            TimerFlood,
            Probe,
            SpinFrom,
            SpinGap,
            TieSender,
            Driver,
            Steward,
            TieReceiver,
            WakeDelay,
            DestroyThenCount,
            DeadlineFlood,
            CarveSpin,
            Agent,
            SubAgent,
            Victim,
            Bystander,
            Containment,
            EndpointMaker,
            ClusterSpinner,
            ClusterServer,
            ClusterDriver,
            ClusterTimer,
        ]
        .into_iter()
        .find(|r| *r as u8 == x)
    }
}

/// `rdtime`.
pub fn ticks() -> u64 { crate::read_time() }

/// Mark `budget` in the kernel's trace: carve an empty child of weight `weight` and destroy it, so
/// the trace's lift names `budget` as its parent.
pub fn mark(budget: u32, weight: u32) {
    let child = rd::create(budget, &rd::spec(0, 0, weight)).expect("a mark");
    rd::destroy(child).expect("a mark's destruction");
}

/// Count until `rdtime` reaches `end`; the count.
#[inline(never)]
pub fn spin_until(end: u64) -> u64 {
    let mut n: u64 = 0;
    loop {
        for _ in 0..CHUNK {
            n = core::hint::black_box(n + 1);
        }
        if ticks() >= end {
            return n;
        }
    }
}

/// Cluster workload and positive preparation use the same billed kernel clock as its window.
#[inline(never)]
fn cluster_spin_until(end: u64) -> Result<u64, Error> {
    let mut n = 0u64;
    loop {
        for _ in 0..CHUNK {
            n = core::hint::black_box(n + 1);
        }
        if rd::time_now()? >= end {
            return Ok(n);
        }
    }
}

/// Sleep until `rdtime` reaches `at` (a `receive` with a timeout and nothing to receive).
pub fn sleep_until(at: u64, ticks_per_us: u64) {
    let now = ticks();
    if at > now {
        let _ = rd::receive(None, (at - now).div_ceil(ticks_per_us.max(1)), 0);
    }
}

fn word(arg: usize, i: usize) -> u64 {
    let mut v = 0u64;
    for b in 0..8 {
        v |= u64::from(spawn::startup_byte(arg, 1 + i * 8 + b)) << (8 * b);
    }
    v
}

/// Two message words holding a `u64` as 32-bit halves (the same on both widths).
fn halves(v: u64) -> [usize; 2] { [v as u32 as usize, (v >> 32) as u32 as usize] }

pub fn join(lo: usize, hi: usize) -> u64 { (lo as u32 as u64) | ((hi as u32 as u64) << 32) }

/// Report `value` on slot 1.
fn report(value: u64) {
    let [lo, hi] = halves(value);
    let _ = rd::send(1, &rd::body([lo, hi, 0, 0]), None, rd::FOREVER);
}

/// Wait on slot 2 for the window: (start, end) in `rdtime` ticks (the cluster's in `time_now` µs).
fn window() -> (u64, u64) {
    let Ok(Received::Message(m)) = rd::receive(Some(2), rd::FOREVER, 0) else { rd::process_exit(90) };
    let w = m.body.words;
    (join(w[0], w[1]), join(w[2], w[3]))
}

/// The launcher's `rdtime` ticks per µs, the last parameter of every child.
fn tpu() -> u64 { param(7).max(1) }

/// Whether this process is a child (set at [`child`]'s entry): its panics cannot print.
static IS_CHILD: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static PARAMS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
static END: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];

fn end_ticks() -> u64 { END[0].load(SeqCst) as u64 | ((END[1].load(SeqCst) as u64) << 32) }

fn set_end(end: u64) {
    END[0].store(end as u32 as usize, SeqCst);
    END[1].store((end >> 32) as usize, SeqCst);
}

fn param(i: usize) -> u64 { PARAMS[i].load(SeqCst) as u64 }

/// Stacks for this process's extra threads, one per thread slot, mapped once and reused: a thread
/// that exits leaves its slot's stack for the next.
static STACKS: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
const STACK_PAGES: usize = 2;

/// A thread of this process running `f(arg)` on the stack of slot `slot`.
fn thread_on(slot: usize, f: extern "C" fn(usize) -> !, arg: usize) {
    let mut stack = STACKS[slot].load(SeqCst);
    if stack == 0 {
        stack = rd::map_anon(STACK_PAGES * rd::PAGE_SIZE, rd::rw()).expect("stack");
        STACKS[slot].store(stack, SeqCst);
    }
    rd::thread_create(f as *const () as usize, stack + STACK_PAGES * rd::PAGE_SIZE - 16, arg)
        .expect("thread");
}

/// A thread of this process running `f(arg)`, on a stack of its own (slots from 1 up).
fn thread(f: extern "C" fn(usize) -> !, arg: usize) {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    thread_on(NEXT.fetch_add(1, SeqCst), f, arg);
}

extern "C" fn gamer_thread(_: usize) -> ! {
    let (burst, nap) = (param(0), param(1));
    let end = end_ticks();
    let mut n = 0;
    while ticks() < end {
        n += spin_until(ticks() + burst);
        let _ = rd::receive(None, nap, 0);
    }
    TOTAL.fetch_add(n as usize, SeqCst);
    DONE.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

extern "C" fn flood_thread(_: usize) -> ! {
    let end = end_ticks();
    let mut calls = 0;
    while ticks() < end {
        if rd::call(3, &rd::body([1, 0, 0, 0]), None, 1_000_000).is_ok() {
            calls += 1;
        }
    }
    TOTAL.fetch_add(calls, SeqCst);
    DONE.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

/// Workers of [`Role::ThreadChurn`] that have finished counting.
static CHURNED: AtomicUsize = AtomicUsize::new(0);

/// Count, then end in `thread_exit` itself: no system call between the count and the exit, so the
/// run is charged at the exit or not at all (the count goes through memory, not a message).
extern "C" fn churn_worker(_: usize) -> ! {
    let n = spin_until(ticks() + param(0));
    TOTAL.fetch_add(n as usize, SeqCst);
    CHURNED.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

extern "C" fn flood_sleeper(i: usize) -> ! {
    let (nap, stagger) = (param(0), param(1));
    let end = end_ticks();
    while ticks() < end {
        let _ = rd::receive(None, nap + stagger * i as u64, 0);
    }
    DONE.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

/// Each [`flood_waiter`]'s endpoint, and the handle [`flood_answerer`] sends to it through.
static FLOOD_ENDPOINTS: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
static FLOOD_HANDLES: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];

/// Wait on its own endpoint for the window, each wait's timeout well ahead: the answerer sends at
/// once, so the wait ends before its timeout, and the timer armed for it comes early. Counts the
/// waits answered.
extern "C" fn flood_waiter(i: usize) -> ! {
    let (ahead, stagger) = (param(0), param(1));
    let end = end_ticks();
    let ep = FLOOD_ENDPOINTS[i].load(SeqCst) as u32;
    let mut answered = 0;
    while ticks() < end {
        if let Ok(Received::Message(_)) = rd::receive(Some(ep), ahead + stagger * i as u64, 0) {
            answered += 1;
        }
    }
    TOTAL.fetch_add(answered, SeqCst);
    DONE.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

/// Send to every waiter, over and over for the window, never blocking (a timeout of 0): each send
/// a waiter is waiting for ends its wait at once.
extern "C" fn flood_answerer(waiters: usize) -> ! {
    let end = end_ticks();
    while ticks() < end {
        for h in FLOOD_HANDLES.iter().take(waiters) {
            let _ = rd::send(h.load(SeqCst) as u32, &rd::body([1, 0, 0, 0]), None, 0);
        }
    }
    DONE.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

/// Wait (sleeping) until `n` threads are done.
fn await_done(n: usize) {
    while DONE.load(SeqCst) < n {
        let _ = rd::receive(None, 1_000, 0);
    }
}

/// A child's entry: its role from the startup block.
pub extern "C" fn child(arg: usize) -> ! { run_child(arg, None) }

/// The cluster fixture's one-thread-per-budget children. Each reports readiness before the next
/// child is created, making its first W an isolated setup wake in the fixed start order.
pub extern "C" fn cluster_child(arg: usize) -> ! {
    IS_CHILD.store(true, SeqCst);
    let role = Role::from_u8(spawn::startup_byte(arg, 0));
    for (i, p) in PARAMS.iter().enumerate() {
        p.store(word(arg, i) as usize, SeqCst);
    }
    report(0); // setup acknowledgement; consumed before the parent starts the next child
    let (start, end) = window();
    let Some(release) = start.checked_add(50_000) else { rd::process_exit(91) };
    match role {
        Some(Role::ClusterServer) => report(cluster_spin_until(end).unwrap_or(u64::MAX)),
        Some(Role::ClusterSpinner) => {
            let before = rd::time_now().unwrap_or(u64::MAX);
            let Some(delay) = release.checked_sub(before).filter(|d| *d > 0) else {
                report(u64::MAX);
                rd::process_exit(92)
            };
            report(if rd::receive(None, delay, 0) == Err(Error::Timeout) {
                cluster_spin_until(end).unwrap_or(u64::MAX)
            } else {
                u64::MAX
            });
        }
        Some(Role::ClusterDriver | Role::ClusterTimer) => cluster_wakes(role.unwrap(), start, release, end),
        _ => report(u64::MAX),
    }
    rd::process_exit(0)
}

/// One blocking wait per reported wake. Zero intent waits toward its nominal slot and phase;
/// positive intent spins to the nominal slot, then arms a fresh relative wait for the phase.
/// Neither path makes an extra preparatory wait, so W/sample order remains joinable.
fn cluster_wakes(role: Role, start: u64, release: u64, end: u64) {
    const N: usize = 200;
    const PHASES: [u64; 4] = [100, 300, 600, 850];
    let base = if role == Role::ClusterDriver {
        match rd::map_device(3) {
            Ok((base, _)) => base,
            Err(e) => {
                cluster_failure(role, 0, 1, e as usize, release, release, release, &[], &[]);
                return;
            }
        }
    } else {
        0
    };
    let mut windows = [Window::default(); N];
    let mut metadata = [ClusterMeta::default(); N];
    let mut n = 0;
    for i in 0..N {
        let Some(slot) = release.checked_add(i as u64 * 80_000) else { break };
        let Some(target) = slot.checked_add(PHASES[i % 4]) else { break };
        // Each phase has 25 zero and 25 positive intent rounds. The trace, not this intent,
        // decides which samples qualify in either category.
        let positive = i / 4 % 2 == 1;
        if positive {
            if cluster_spin_until(slot).is_err() {
                cluster_failure(role, i, 6, 0, target, slot, release, &windows[..n], &metadata[..n]);
                return;
            }
        }
        let before = match rd::time_now() {
            Ok(v) => v,
            Err(e) => {
                cluster_failure(role, i, 6, e as usize, target, slot, release, &windows[..n], &metadata[..n]);
                return;
            }
        };
        let delay = if positive { Some(PHASES[i % 4]) } else { target.checked_sub(before) };
        let Some(delay) = delay.filter(|d| *d > 0) else {
            cluster_failure(role, i, 2, 0, target, before, release, &windows[..n], &metadata[..n]);
            return;
        };
        if positive && before < slot {
            cluster_failure(role, i, 2, 0, target, before, release, &windows[..n], &metadata[..n]);
            return;
        }
        let Some(lower) = before.checked_add(delay) else {
            cluster_failure(role, i, 2, 0, target, before, release, &windows[..n], &metadata[..n]);
            return;
        };
        let prep_us = if before >= slot { (before - slot) as i32 } else { -((slot - before) as i32) };
        let driver = role == Role::ClusterDriver;
        // The timer has no E and no RTC: those fields stay 0, so L is never shown as a deadline
        // the kernel observed.
        let (early, observed, arm, deadline, service) = if driver {
            let arm = rtc::now_ns(base);
            let Some(deadline) = delay.checked_mul(1_000).and_then(|d| arm.checked_add(d)) else {
                cluster_failure(role, i, 2, 0, target, before, release, &windows[..n], &metadata[..n]);
                return;
            };
            rtc::alarm(base, deadline);
            let received = rd::receive(Some(4), delay + 100_000, 0);
            // E, the RTC service S, then P, in that order; both kernel reads must succeed.
            let early = rd::time_now();
            let service = rtc::now_ns(base);
            let observed = rd::time_now();
            rtc::clear(base);
            let (early, observed) = match (early, observed) {
                (Ok(early), Ok(observed)) => (early, observed),
                (Err(e), _) | (_, Err(e)) => {
                    cluster_failure(
                        role,
                        i,
                        6,
                        e as usize,
                        target,
                        before,
                        release,
                        &windows[..n],
                        &metadata[..n],
                    );
                    return;
                }
            };
            if !matches!(received, Ok(Received::Interrupt)) {
                cluster_failure(
                    role,
                    i,
                    3,
                    cluster_result(received),
                    target,
                    before,
                    release,
                    &windows[..n],
                    &metadata[..n],
                );
                return;
            }
            (early, observed, arm, deadline, service)
        } else {
            let received = rd::receive(None, delay, 0);
            let observed = match rd::time_now() {
                Ok(now) => now,
                Err(e) => {
                    cluster_failure(
                        role,
                        i,
                        6,
                        e as usize,
                        target,
                        before,
                        release,
                        &windows[..n],
                        &metadata[..n],
                    );
                    return;
                }
            };
            if received != Err(Error::Timeout) {
                cluster_failure(
                    role,
                    i,
                    4,
                    cluster_result(received),
                    target,
                    before,
                    release,
                    &windows[..n],
                    &metadata[..n],
                );
                return;
            }
            (0, observed, 0, 0, 0)
        };
        // The envelope [L, U): U = P + 1 holds all of P's floored microsecond.
        let Some(upper) = observed.checked_add(1) else {
            cluster_failure(role, i, 7, 0, target, observed, release, &windows[..n], &metadata[..n]);
            return;
        };
        if lower < release
            || lower > observed
            || upper > end
            || (driver && (early < before || early > observed || service < deadline || arm > deadline))
        {
            cluster_failure(role, i, 7, 0, target, observed, release, &windows[..n], &metadata[..n]);
            return;
        }
        windows[n] = Window { end: upper, gross: upper - lower };
        metadata[n] = ClusterMeta {
            prep_us,
            delay_us: delay as u32,
            before,
            lower,
            early,
            observed,
            arm,
            deadline,
            service,
        };
        n += 1;
    }
    let tag = if role == Role::ClusterDriver { Stats::DRIVER_WAKE } else { Stats::TIMER_WAKE };
    // Only these two empty zero-weight scopes are destroyed by the stand-ins. Their existing
    // R10 X/Y records fence all 200 waits before any reporting send can block and wake again.
    let marker = if role == Role::ClusterDriver { 5 } else { 3 };
    if let Err(e) = rd::destroy(marker) {
        cluster_failure(
            role,
            N,
            5,
            e as usize,
            release + (199 * 80_000 + 850),
            release,
            release,
            &windows[..n],
            &metadata[..n],
        );
        return;
    }
    report_windows(&windows[..n], tag);
    hand_over_cluster(tag, start, end, &windows[..n], &metadata[..n]);
}

/// A failure report occupies the usual one-report slot. Its times are signed microseconds from
/// the common release; the high bit in word 2 marks it as diagnostic, not a latency statistic.
/// Branches are map error (1), zero or late delay, positive B before its slot or overflow (2),
/// driver receive (3), timer receive (4), marker destruction (5), a failed kernel clock read (6),
/// an envelope outside `[R, F]` or out of order (7).
/// Results are map errors' ABI codes, zero for delay, or receive variants 1..4 and errors
/// 0x80 | ABI code. This path runs only after a failed attempt, outside valid sample windows.
fn cluster_failure(
    role: Role,
    index: usize,
    branch: usize,
    result: usize,
    target: u64,
    now: u64,
    release: u64,
    windows: &[Window],
    metadata: &[ClusterMeta],
) {
    let relative = |at: u64| -> usize {
        let us = if at >= release { (at - release) as i32 } else { -((release - at) as i32) };
        us as u32 as usize
    };
    let tag = if role == Role::ClusterDriver { Stats::DRIVER_WAKE } else { Stats::TIMER_WAKE };
    let code = 0x8000_0000usize | (index << 16) | (branch << 8) | result;
    let _ =
        rd::send(1, &rd::body([relative(target), relative(now), code, tag | index << 8]), None, rd::FOREVER);
    hand_over_cluster(tag, release - 50_000, release + WINDOW_US, windows, metadata);
}

fn cluster_result(received: Result<Received, Error>) -> usize {
    match received {
        Ok(Received::Message(_)) => 1,
        Ok(Received::Interrupt) => 2,
        Ok(Received::Exit(_)) => 3,
        Ok(Received::Abandoned(_)) => 4,
        Err(e) => 0x80 | e as usize,
    }
}

/// The containment gate's children's entry: [`child`], with the gate's roles. Only the gate's
/// program names it, so no other case's image carries the gate's code (an image's size is in
/// every spawn a case measures).
pub extern "C" fn containment_child(arg: usize) -> ! { run_child(arg, Some(containment_role)) }

/// The gate's roles; each one ends its process. `windowed` is whether the child has waited for its
/// go window: the endpoint maker and the agents are started directly, without one.
fn containment_role(role: Option<Role>, windowed: bool) {
    match (role, windowed) {
        (Some(Role::EndpointMaker), false) => endpoint_maker(),
        (Some(Role::Agent), false) => agent_main(),
        (Some(Role::SubAgent), false) => sub_agent_main(),
        (Some(Role::Victim), true) => victim(),
        (Some(Role::Bystander), true) => bystander(),
        (Some(Role::Containment), true) => containment(),
        _ => {}
    }
}

/// A child's body: its role from the startup block; `more` runs a case's own roles.
fn run_child(arg: usize, more: Option<fn(Option<Role>, bool)>) -> ! {
    IS_CHILD.store(true, SeqCst);
    let role = Role::from_u8(spawn::startup_byte(arg, 0));
    for (i, p) in PARAMS.iter().enumerate() {
        p.store(word(arg, i) as usize, SeqCst);
    }
    if role == Some(Role::ChurnChild) {
        let n = spin_until(ticks() + param(0));
        // Process churn (p2 = 1) reports nothing: the count ends in the exit or the fault itself,
        // so the run is charged there or not at all.
        if param(2) == 0 {
            let _ = rd::send(1, &rd::body([n as usize, 0, 0, 0]), None, rd::FOREVER);
        }
        if param(1) == 1 {
            // A fault: a store to page 0, which nothing maps.
            // SAFETY: none; this faults on purpose, and the kernel ends the process.
            unsafe { (0x8 as *mut u64).write_volatile(1) };
        }
        rd::process_exit(0)
    }
    // A case's roles started directly (not through `Bench::start`) have no go window and no
    // report handle, so they are dispatched before the window wait.
    if let Some(more) = more {
        more(role, false);
    }
    let (start, end) = window();
    let tpu = tpu();
    set_end(end);
    sleep_until(start, tpu);
    if let Some(more) = more {
        more(role, true);
    }
    let total = match role {
        Some(Role::Spin) => spin_until(end),
        Some(Role::SpinFrom) => {
            sleep_until(param(0), tpu);
            spin_until(end)
        }
        Some(Role::SpinGap) => {
            spin_until(param(0));
            sleep_until(param(1), tpu);
            spin_until(end)
        }
        Some(Role::TieSender) => {
            let r = rd::send(3, &rd::body([0; 4]), None, rd::FOREVER);
            let t = ticks();
            report(if r == Err(Error::Dead) { t } else { 0 });
            rd::process_exit(0)
        }
        Some(Role::WakeDelay) => {
            let (nap, mut least) = (param(0), u64::MAX);
            for _ in 0..param(1) {
                let before = ticks();
                let _ = rd::receive(None, nap, 0);
                least = least.min(ticks().saturating_sub(before + nap * tpu) / tpu);
            }
            least
        }
        Some(Role::DestroyThenCount) => {
            // Timed with rdtime: no call of its own before the destruction.
            let before = ticks();
            let _ = rd::destroy(3);
            let took = (ticks() - before) / tpu;
            // From here on, the longest time this thread went without the CPU.
            let (mut n, mut last, mut gap) = (0u64, ticks(), 0u64);
            while last < end {
                for _ in 0..CHUNK {
                    n = core::hint::black_box(n + 1);
                }
                let now = ticks();
                gap = gap.max(now - last);
                last = now;
            }
            let _ = rd::send(1, &rd::body([took as usize, (gap / tpu) as usize, 0, 0]), None, rd::FOREVER);
            n
        }
        Some(Role::TieReceiver) => {
            let r = rd::receive(Some(3), rd::FOREVER, 0);
            let t = ticks();
            report(if matches!(r, Ok(Received::Message(_))) { t } else { 0 });
            rd::process_exit(0)
        }
        Some(Role::Driver) => {
            driver(param(0) as usize, param(1) == 1);
            rd::process_exit(0)
        }
        Some(Role::Steward) => {
            steward(param(0) as usize, param(1) as usize, param(2) == 1);
            rd::process_exit(0)
        }
        Some(Role::Probe) => {
            let _ =
                rd::send(1, &rd::body([rd::time_now().unwrap_or(0) as usize, 0, 0, 0]), None, rd::FOREVER);
            rd::process_exit(0)
        }
        Some(Role::Gamer) => {
            let threads = param(2).max(1) as usize;
            for _ in 1..threads {
                thread(gamer_thread, 0);
            }
            let mut n = 0;
            while ticks() < end {
                n += spin_until(ticks() + param(0));
                let _ = rd::receive(None, param(1), 0);
            }
            await_done(threads - 1);
            n + TOTAL.load(SeqCst) as u64
        }
        Some(Role::Server) => {
            let mut work = 0;
            while ticks() < end {
                let wait = (end.saturating_sub(ticks()) / tpu).max(1);
                if let Ok(Received::Message(m)) = rd::receive(Some(3), wait, 0) {
                    work += spin_until(ticks() + param(0) * tpu);
                    let _ = rd::reply(m.msg_id.get(), &rd::body([0; 4]));
                }
            }
            work
        }
        Some(Role::Flood) => {
            let threads = param(2).max(1) as usize;
            for _ in 0..threads {
                thread(flood_thread, 0);
            }
            await_done(threads);
            TOTAL.load(SeqCst) as u64
        }
        Some(Role::ThreadChurn) => {
            let mut round = 0;
            while ticks() < end {
                // One worker at a time, on two stacks in turn: the one before last has certainly
                // exited (the last may still be between its count and its exit). This thread
                // sleeps while the worker counts, and looks every millisecond.
                thread_on(30 + round % 2, churn_worker, 0);
                round += 1;
                while CHURNED.load(SeqCst) < round {
                    let _ = rd::receive(None, 1_000, 0);
                }
            }
            TOTAL.load(SeqCst) as u64
        }
        Some(Role::ProcessChurn) => process_churn(end),
        Some(Role::DeadlineFlood) => {
            let mut n = 0;
            while ticks() < end {
                let now = rd::time_now().unwrap_or(0);
                for i in 0..param(0) {
                    let spec = rd::BudgetSpec { deadline: now + 200 + i, ..rd::spec(1, 0, 0) };
                    let _ = rd::create(3, &spec);
                }
                n += spin_until((ticks() + 1_000 * tpu).min(end));
            }
            n
        }
        Some(Role::BudgetChurn) => budget_churn(end, tpu),
        Some(Role::CarveSpin) => {
            // The carve must begin a slice, and the count end nine tenths into it, the carve's own
            // time (about a millisecond) included, so that the destruction falls inside it: requeued at
            // weight 1, this budget's pass would defer the destruction by seconds. A wake is picked
            // with a fresh slice; the one-millisecond timeout lets it block before that wake.
            let _ = rd::receive(None, 1_000, 0);
            let woke = ticks();
            let child = rd::create(3, &rd::spec(0, 0, param(0) as u32)).expect("the carve");
            let created = ticks();
            spin_until(woke + (9 * SLICE_US / 10) * tpu);
            rd::destroy(child).expect("the carve's return");
            let returned_ticks = ticks();
            let returned = rd::time_now().unwrap_or(0);
            let n = spin_until(end);
            report(created.saturating_sub(woke) / tpu);
            report(returned_ticks.saturating_sub(woke) / tpu);
            report(returned);
            n
        }
        Some(Role::TimerFlood) if param(3) == 2 => {
            let threads = param(2).max(2) as usize;
            for i in 0..threads - 1 {
                let ep = rd::endpoint_create().expect("endpoint");
                FLOOD_ENDPOINTS[i].store(ep as usize, SeqCst);
                FLOOD_HANDLES[i].store(rd::mint_from_handle(ep, 1, None).expect("handle") as usize, SeqCst);
            }
            for i in 0..threads - 1 {
                thread(flood_waiter, i);
            }
            thread(flood_answerer, threads - 1);
            // One timed wait for the window, not a nap each millisecond: a nap that times out is
            // a wait due at every expiry walk of this process, and each such walk recomputes its
            // timer hint from the waits still blocked, so a hint a cancelled wait left behind,
            // 15 ms ahead, would never be reached.
            sleep_until(end, tpu);
            await_done(threads);
            TOTAL.load(SeqCst) as u64
        }
        Some(Role::TimerFlood) => {
            let threads = param(2).max(1) as usize;
            for i in 0..threads {
                thread(flood_sleeper, i);
            }
            if param(3) == 1 {
                // Budgets with deadlines a microsecond apart, all under this one's own budget.
                let now = rd::time_now().unwrap_or(0);
                for i in 0..64 {
                    let spec = rd::BudgetSpec { deadline: now + 1_000 + i, ..rd::spec(1, 0, 0) };
                    let _ = rd::create(3, &spec);
                }
            }
            await_done(threads);
            0
        }
        _ => 0,
    };
    report(total);
    rd::process_exit(0)
}

/// Process churn: children in this process's own budget (slot 3), each counting for a while and
/// then exiting or faulting straight from the count (no report: the victim's share is what the
/// case judges). Reports the children that ran.
fn process_churn(end: u64) -> u64 {
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit");
    let mut children = 0u64;
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::ChurnChild as u8;
    startup[1..9].copy_from_slice(&param(0).to_le_bytes());
    startup[9..17].copy_from_slice(&param(1).to_le_bytes());
    startup[17..25].copy_from_slice(&1u64.to_le_bytes());
    while ticks() < end {
        if spawn::spawn(&image, 3, exit, child as *const () as usize, &startup, &[]).is_err() {
            break;
        }
        let _ = rd::receive(Some(exit), rd::FOREVER, 0);
        children += 1;
    }
    children
}

/// Budget churn under this process's own budget (slot 3); see [`Role::BudgetChurn`].
fn budget_churn(end: u64, tpu: u64) -> u64 {
    let variant = param(0);
    let weight = param(1).max(1) as u32;
    if variant == 4 {
        // The shell pattern, as the kernel sees it: this budget keeps counting on a thread of its
        // own and on this one, which runs most of a slice and then, holding the lead that gave
        // it, gives five budgets back to back most of its weight and takes it back, with no run
        // in between. Creating and destroying moves nothing (kernel/scheduling.md, "Inheritance"):
        // the budget keeps its share. (A lift that counted the entry wait, from the floor, would
        // grow the lead by half again at each one.)
        thread(shell_spinner, 0);
        let mut total = 0;
        while ticks() < end {
            total += spin_until((ticks() + (SLICE_US - SLICE_US / 5) * tpu).min(end));
            for _ in 0..5 {
                if let Ok(c) = rd::create(3, &rd::spec(1, 0, weight)) {
                    let _ = rd::destroy(c);
                }
            }
            let _ = rd::receive(None, 1_000, 0);
        }
        await_done(1);
        return total + TOTAL.load(SeqCst) as u64;
    }
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit");
    let rep = rd::endpoint_create().expect("rep");
    let rep_client = rd::mint_from_handle(rep, 1, None).expect("mint");
    let pages = image.pages() as u64 + 48;
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::ChurnChild as u8;
    // A shell's command runs about a millisecond; the other variants' children a slice.
    let run = if variant == 4 { 1_000 } else { SLICE_US };
    startup[1..9].copy_from_slice(&(run * tpu).to_le_bytes());
    let mut total = 0u64;
    while ticks() < end {
        // Variant 3: a fresh intermediate for each child, kept (its debt parked on it) until the
        // weight runs out.
        let parent =
            if variant == 3 { rd::create(3, &rd::spec(pages + 1, 1, weight)).unwrap_or(3) } else { 3 };
        let deadline =
            if variant == 2 { rd::time_now().unwrap_or(0) + SLICE_US + SLICE_US / 2 } else { rd::FOREVER };
        let spec = rd::BudgetSpec { deadline, ..rd::spec(pages, 1, weight) };
        let Ok(c) = rd::create(parent, &spec) else { break };
        if spawn::spawn(&image, c, exit, child as *const () as usize, &startup, &[rep_client]).is_err() {
            let _ = rd::destroy(c);
            break;
        }
        match variant {
            // Spinning parent: count through most of our own slice, then destroy the child,
            // which has had its slice by now or is about to lose it.
            1 => {
                total += spin_until(ticks() + (SLICE_US - SLICE_US / 10) * tpu);
                if let Ok(Received::Message(m)) = rd::receive(Some(rep), 0, 0) {
                    total += m.body.words[0] as u64;
                }
                let _ = rd::destroy(c);
            }
            // The deadline destroys it; meanwhile count.
            2 => {
                total += spin_until(ticks() + 2 * SLICE_US * tpu);
            }
            _ => {
                if let Ok(Received::Message(m)) = rd::receive(Some(rep), rd::FOREVER, 0) {
                    total += m.body.words[0] as u64;
                }
                let _ = rd::destroy(c);
            }
        }
        // Drain the reports and notices this cycle left.
        while let Ok(Received::Message(m)) = rd::receive(Some(rep), 0, 0) {
            total += m.body.words[0] as u64;
        }
        while rd::receive(Some(exit), 0, 0).is_ok() {}
    }
    if variant == 4 {
        await_done(1);
        total += TOTAL.load(SeqCst) as u64;
    }
    total
}

extern "C" fn shell_spinner(_: usize) -> ! {
    let n = spin_until(end_ticks());
    TOTAL.fetch_add(n as usize, SeqCst);
    DONE.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

/// Latency statistics, in µs, reported as one message each: `[p50, p99, max, tag | count << 8]`.
pub struct Stats;

impl Stats {
    pub const DEADLINE: usize = 4;
    /// The steward's wake from the timeout after which it destroys a lease: its decision.
    pub const DECISION_WAKE: usize = 5;
    pub const DESTROY: usize = 3;
    /// The driver's alarms that never arrived (first word: how many).
    pub const DRIVER_LOST: usize = 6;
    pub const DRIVER_WAKE: usize = 1;
    pub const TIMER_WAKE: usize = 2;

    /// The name the bench's post-check knows a measure's samples by.
    fn measure(tag: usize) -> Option<&'static str> {
        match tag {
            Stats::DRIVER_WAKE => Some("driver_wake"),
            Stats::TIMER_WAKE => Some("timer_wake"),
            Stats::DECISION_WAKE => Some("decision_wake"),
            Stats::DEADLINE => Some("deadline_notice"),
            _ => None,
        }
    }

    /// Sort `samples` and send `[p50, p99, max, tag | count << 8]` on slot 1.
    fn report(samples: &mut [u64], tag: usize) {
        samples.sort_unstable();
        let n = samples.len();
        let at =
            |q: usize| samples.get((n * q).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0) as usize;
        let max = samples.last().copied().unwrap_or(0) as usize;
        let _ = rd::send(1, &rd::body([at(50), at(99), max, tag | n << 8]), None, rd::FOREVER);
    }
}

/// The most samples a stand-in takes of one thing.
const MAX_SAMPLES: usize = 256;

/// One latency sample's window: its end on `time_now` and its length, µs. The bench's post-check
/// subtracts the checked build's audit time inside it (kernel/scheduling.md, "Responsiveness").
#[derive(Clone, Copy, Default)]
struct Window {
    end: u64,
    gross: u64,
}

/// Ordered cluster metadata, held until both stand-ins finish their measurements. `before` (B),
/// `lower` (L), `early` (E) and `observed` (P) are `time_now` microseconds; `arm`, `deadline` and
/// `service` are goldfish RTC nanoseconds. The timer reads neither E nor the RTC: those are 0.
#[derive(Clone, Copy, Default)]
struct ClusterMeta {
    /// Arming preparation relative to the nominal slot, in signed microseconds.
    prep_us: i32,
    delay_us: u32,
    before: u64,
    lower: u64,
    early: u64,
    observed: u64,
    arm: u64,
    deadline: u64,
    service: u64,
}

// 200 * (64-byte metadata + 16-byte window) = 16,000 bytes on the child's 32-KiB stack
// (`spawn::STACK_PAGES`), with 2 KiB more in `report_windows`.
const _: () = assert!(core::mem::size_of::<ClusterMeta>() == 64);

/// After a stand-in's reports, once the launcher asks on slot 2 (when every measurement is over,
/// so the windows' messages perturb none): send each window, `[end, end, gross, tag]` (the end in
/// halves), then `[0; 4]`.
fn hand_over(sets: &[(usize, &[Window])]) {
    let _ = rd::receive(Some(2), rd::FOREVER, 0);
    for (tag, windows) in sets {
        for w in windows.iter() {
            let [lo, hi] = halves(w.end);
            let _ = rd::send(1, &rd::body([lo, hi, w.gross as usize, *tag]), None, rd::FOREVER);
        }
    }
    let _ = rd::send(1, &rd::body([0; 4]), None, rd::FOREVER);
}

/// Cluster-only metadata and windows, after measurement and after the parent's explicit request:
/// a header echoing the H and F this child received, then per attempt, in original order, five
/// metadata messages and its envelope `[U, U, U - L, tag]` (U in halves), then `[0; 4]`.
fn hand_over_cluster(tag: usize, start: u64, end: u64, windows: &[Window], metadata: &[ClusterMeta]) {
    let _ = rd::receive(Some(2), rd::FOREVER, 0);
    let [h0, h1] = halves(start);
    let [f0, f1] = halves(end);
    let _ = rd::send(1, &rd::body([h0, h1, f0, f1]), None, rd::FOREVER);
    for (i, (w, meta)) in windows.iter().zip(metadata).enumerate() {
        let [b0, b1] = halves(meta.before);
        let [l0, l1] = halves(meta.lower);
        let [e0, e1] = halves(meta.early);
        let [p0, p1] = halves(meta.observed);
        let [arm_lo, arm_hi] = halves(meta.arm);
        let [deadline_lo, deadline_hi] = halves(meta.deadline);
        let [service_lo, service_hi] = halves(meta.service);
        let [end_lo, end_hi] = halves(w.end);
        for words in [
            [i, usize::from(i / 4 % 2 == 1), meta.prep_us as u32 as usize, meta.delay_us as usize],
            [b0, b1, l0, l1],
            [e0, e1, p0, p1],
            [arm_lo, arm_hi, deadline_lo, deadline_hi],
            [service_lo, service_hi, i, 0xc4],
            [end_lo, end_hi, w.gross as usize, tag],
        ] {
            let _ = rd::send(1, &rd::body(words), None, rd::FOREVER);
        }
    }
    let _ = rd::send(1, &rd::body([0; 4]), None, rd::FOREVER);
}

/// Report the windows' lengths as the stats `tag`.
fn report_windows(windows: &[Window], tag: usize) {
    let mut gross = [0u64; MAX_SAMPLES];
    let n = windows.len().min(MAX_SAMPLES);
    gross.iter_mut().zip(windows).for_each(|(g, w)| *g = w.gross);
    Stats::report(&mut gross[..n], tag);
}

/// The steward's timeout wakes, one per window (how late it runs again after each deadline),
/// reported as [`Stats::TIMER_WAKE`].
fn timer_wakes(windows: &mut [Window]) {
    for (i, w) in windows.iter_mut().enumerate() {
        let timeout = 3_000 + (i as u64 * 397) % 1_000;
        let before = rd::time_now().unwrap_or(0);
        let _ = rd::receive(None, timeout, 0);
        let end = rd::time_now().unwrap_or(0);
        *w = Window { end, gross: end.saturating_sub(before + timeout) };
    }
    report_windows(windows, Stats::TIMER_WAKE);
}

/// The driver stand-in: `k` alarms, each a little over 2 ms ahead (phases spread over a
/// millisecond); how late, on the RTC's own clock, it runs again after each.
fn driver(k: usize, hold: bool) {
    let Ok((base, _)) = rd::map_device(3) else { return };
    let mut late = [0u64; MAX_SAMPLES];
    let mut windows = [Window::default(); MAX_SAMPLES];
    let (k, mut n, mut lost) = (k.min(MAX_SAMPLES), 0, 0usize);
    for i in 0..2 * k {
        if n == k {
            break;
        }
        let at = rtc::now_ns(base) + 2_000_000 + (i as u64 * 397_000) % 1_000_000;
        rtc::alarm(base, at);
        // An alarm that fires while the source is masked (the driver's slice ended before it
        // got back into `receive`) may never be delivered (R5; docs/todo/irq-level-latch.md);
        // give up on it after 100 ms, count it, and arm again.
        if matches!(rd::receive(Some(4), 100_000, 0), Ok(Received::Interrupt)) {
            // The same window on `time_now` (the RTC keeps virtual time too), its end read first
            // so that it holds all the lateness measured.
            let end = rd::time_now().unwrap_or(0);
            late[n] = rtc::now_ns(base).saturating_sub(at) / 1000;
            windows[n] = Window { end, gross: late[n] };
            n += 1;
        } else {
            lost += 1;
        }
        rtc::clear(base);
    }
    Stats::report(&mut late[..n], Stats::DRIVER_WAKE);
    let _ = rd::send(1, &rd::body([lost, 0, 0, Stats::DRIVER_LOST | 1 << 8]), None, rd::FOREVER);
    if hold {
        hand_over(&[(Stats::DRIVER_WAKE, &windows[..n])]);
    }
}

/// The steward stand-in: `k` timeout wakes (how late it runs again after each deadline); `leases`
/// one-process leases carved from slot 3, each destroyed by hand after a timeout (how long
/// `budget_destroy` takes); and `leases` more destroyed by their deadlines (how late the killed
/// notice arrives).
fn steward(k: usize, leases: usize, hold: bool) {
    let now = || rd::time_now().unwrap_or(0);
    let (k, leases) = (k.min(MAX_SAMPLES), leases.min(MAX_SAMPLES));
    let mut windows = [[Window::default(); MAX_SAMPLES]; 3];
    timer_wakes(&mut windows[0][..k]);
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit");
    let pages = image.pages() as u64 + 96;
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::Spin as u8;
    // A lease's spinner counts until a window that never ends (its lease ends first).
    let (mut destroy, mut nd) = ([0u64; MAX_SAMPLES], 0);
    let (mut ns, mut nn) = (0, 0);
    // By hand: spawn a lease's process, sleep (the timeout the steward decides on), destroy.
    for _ in 0..leases * 2 {
        if nd == leases {
            break;
        }
        let Ok(lease) = rd::create(3, &rd::spec(pages, 1, 10)) else { continue };
        if spawn::spawn(&image, lease, exit, lease_spinner as *const () as usize, &startup, &[]).is_err() {
            let _ = rd::destroy(lease);
            continue;
        }
        let before = now();
        let _ = rd::receive(None, 5_000, 0);
        let woke = now();
        windows[1][ns] = Window { end: woke, gross: woke.saturating_sub(before + 5_000) };
        ns += 1;
        if rd::destroy(lease).is_ok() {
            destroy[nd] = now() - woke;
            nd += 1;
        }
        let _ = rd::receive(Some(exit), 1_000_000, 0);
    }
    // By deadline: a budget's deadline is fixed when it is created, before the spawn, so it
    // leaves room for the spawn under load; a lease whose deadline came first is retried. The
    // steward's spawn at N = 16 takes about 150 ms on one hart: at a lead of 150 ms every retry on
    // one seed lost the race and took no sample. On two, the spawn waits for the lock behind the
    // sessions' kernel entries too, and at 300 ms rv64 took 19 of 50, so the lead is 600 ms.
    for _ in 0..leases * 2 {
        if nn == leases {
            break;
        }
        let deadline = now() + 600_000;
        let Ok(lease) = rd::create(3, &rd::BudgetSpec { deadline, ..rd::spec(pages, 1, 10) }) else {
            continue;
        };
        if spawn::spawn(&image, lease, exit, lease_spinner as *const () as usize, &startup, &[]).is_err() {
            let _ = rd::destroy(lease);
            continue;
        }
        if let Ok(Received::Exit(n)) = rd::receive(Some(exit), 1_000_000, 0) {
            if n.cause == rd::Cause::Killed {
                let end = now();
                windows[2][nn] = Window { end, gross: end.saturating_sub(deadline) };
                nn += 1;
            }
        }
    }
    Stats::report(&mut destroy[..nd], Stats::DESTROY);
    report_windows(&windows[1][..ns], Stats::DECISION_WAKE);
    report_windows(&windows[2][..nn], Stats::DEADLINE);
    if hold {
        hand_over(&[
            (Stats::TIMER_WAKE, &windows[0][..k]),
            (Stats::DECISION_WAKE, &windows[1][..ns]),
            (Stats::DEADLINE, &windows[2][..nn]),
        ]);
    }
}

/// A lease's process: spin until killed.
extern "C" fn lease_spinner(_: usize) -> ! {
    loop {
        spin_until(u64::MAX);
    }
}

/// The goldfish RTC (QEMU `virt`), for the latency case's driver stand-in: a nanosecond clock
/// with one alarm and an interrupt. Found among the first program's device handles by what it
/// does, since a handle says nothing of what device it is.
pub mod rtc {
    use crate::rd::{self, Received};

    const TIME_LOW: usize = 0x00;
    const TIME_HIGH: usize = 0x04;
    const ALARM_LOW: usize = 0x08;
    const ALARM_HIGH: usize = 0x0c;
    const IRQ_ENABLED: usize = 0x10;
    const CLEAR_INTERRUPT: usize = 0x1c;
    /// What a virtio-mmio device holds at offset 0 ("virt"): not the RTC.
    const VIRTIO_MAGIC: u32 = 0x7472_6976;

    fn read(base: usize, reg: usize) -> u32 {
        // SAFETY: `base` is a device page `map_device` mapped here; the registers are 32-bit.
        unsafe { ((base + reg) as *const u32).read_volatile() }
    }

    fn write(base: usize, reg: usize, v: u32) {
        // SAFETY: as `read`.
        unsafe { ((base + reg) as *mut u32).write_volatile(v) }
    }

    /// The RTC's time, in ns (reading the low word latches the high one).
    pub fn now_ns(base: usize) -> u64 {
        let lo = read(base, TIME_LOW);
        u64::from(lo) | u64::from(read(base, TIME_HIGH)) << 32
    }

    /// Raise the interrupt when the RTC reaches `at` ns.
    pub fn alarm(base: usize, at: u64) {
        write(base, IRQ_ENABLED, 1);
        write(base, ALARM_HIGH, (at >> 32) as u32);
        write(base, ALARM_LOW, at as u32);
    }

    pub fn clear(base: usize) { write(base, CLEAR_INTERRUPT, 1); }

    /// Among `devices` (read with [`rd::first_free`] before any handle is made): the RTC's MMIO
    /// handle and where it is mapped here, and its interrupt handle. The MMIO is the one-page
    /// device that is not virtio and whose first word (its time's low half, in ns) moves by half
    /// a million to a hundred million over a millisecond's sleep: nothing else of the others is
    /// read, since some fault on reads they do not expect. The interrupt is the one a `receive`
    /// on which returns when the alarm fires. The probing's alarms are all taken before this
    /// returns, so the caller's first `receive` on it meets only an alarm the caller set.
    pub fn find(devices: core::ops::Range<u32>) -> Option<(u32, usize, u32)> {
        let (mmio, base) = devices.clone().find_map(|h| {
            let (addr, len) = rd::map_device(h).ok()?;
            if len == rd::PAGE_SIZE && read(addr, 0) != VIRTIO_MAGIC {
                let a = read(addr, TIME_LOW);
                let _ = rd::receive(None, 1_000, 0);
                let moved = read(addr, TIME_LOW).wrapping_sub(a);
                if (500_000..100_000_000).contains(&moved) {
                    return Some((h, addr));
                }
            }
            let _ = rd::unmap(addr, len);
            None
        })?;
        // Each candidate waits in `receive` while an alarm fires: an alarm that fired while its
        // source was masked would not be seen by a later `receive` (observed on QEMU virt).
        let irq = devices.filter(|h| *h != mmio).find(|h| {
            alarm(base, now_ns(base) + 1_000_000);
            let fired = matches!(rd::receive(Some(*h), 3_000, 0), Ok(Received::Interrupt));
            clear(base);
            fired
        })?;
        // A probe's alarm can still be raised on the found source (the kernel reports it at the
        // next unmask): take them all, until 20 ms pass quietly.
        loop {
            clear(base);
            if rd::receive(Some(irq), 20_000, 0).is_err() {
                break;
            }
        }
        Some((mmio, base, irq))
    }
}

/// A case's panic: say so on the UART, then park. A child has no UART (it is a copy of the
/// launcher, console state and all, without the mapping), so it faults instead, on purpose, at
/// `0x7000_0000 | file << 16 | line` (file 1 `sched.rs`, 2 `spawn.rs`, 3 `rd.rs`, 0 another): the
/// kernel's `PROGRAM HALT` line names where it panicked, and no text of the child's reaches the
/// console (rule F).
pub fn panicked(name: &str, info: &core::panic::PanicInfo) -> ! {
    if IS_CHILD.load(SeqCst) {
        let (file, line) = info.location().map_or(("", 0), |l| (l.file(), l.line() as usize));
        let tag =
            ["sched.rs", "spawn.rs", "rd.rs"].iter().position(|f| file.ends_with(f)).map_or(0, |i| i + 1);
        // Nothing is mapped there: the load faults, and the kernel ends the process.
        rd::peek(0x7000_0000 | tag << 16 | line.min(0xffff));
    }
    let _ = writeln!(Console, "[{}] FAIL: panic: {}", name, info);
    crate::park()
}

// --- The containment gate (kernel/README.md, "Containment") -------------------------------

/// The gate's lease geometry: the same page counts on both widths. The sub-budget is the
/// sub-agent's image and churn. The agent fills its handle table to `MAX_HANDLES` with endpoints,
/// and the lease is sized ([`ct_lease_pages`]) so that the kernel's refusal of a handle past a
/// full table ends the fill, never the lease's pages: the full fill, which is what makes R10 walk
/// as far as a lease can make it walk.
pub const CT_SUB_PAGES: u64 = 1500;
/// What objects cost (kernel/objects.md, the pages each object takes): an endpoint is one page,
/// and a handle-table page holds 128 handles.
const ENDPOINT_PAGES: u64 = 1;
const HANDLES_PER_PAGE: u64 = 128;
/// The agent's handles that are not its fill: its call, send and progress handles, its lease and
/// the sub-budget.
const CT_AGENT_HANDLES: u64 = 5;
/// The endpoints of a full fill: the rest of the handle table.
const CT_FILL_ENDPOINTS: u64 = redoubt_sys::MAX_HANDLES as u64 - CT_AGENT_HANDLES;
/// The agent's thread pages: an IPC page per thread, a stack per thread it starts, and its lends.
const CT_THREAD_PAGES: u64 = ((1 + CT_SPINS + CT_CALLERS + CT_SENDERS)
    + (CT_SPINS + CT_CALLERS + CT_SENDERS) * STACK_PAGES
    + CT_CALLERS * CT_LEND_PAGES) as u64;
/// Room for what the agent holds besides: its header page, page tables and startup page.
const CT_AGENT_SPARE: u64 = 64;

/// The pages a full fill holds at its least: the image, the sub-budget's carve and its own page,
/// the agent's thread pages, the fill's endpoints and a full handle table. A lease whose usage
/// reaches this, with pages to spare, filled its table.
fn ct_fill_pages(image: &Image) -> u64 {
    image.pages() as u64
        + CT_SUB_PAGES
        + 1
        + CT_THREAD_PAGES
        + CT_FILL_ENDPOINTS * ENDPOINT_PAGES
        + redoubt_sys::MAX_HANDLES as u64 / HANDLES_PER_PAGE
}

/// A lease: a full fill and room to spare, so the table fills before the pages run out.
fn ct_lease_pages(image: &Image) -> u64 { ct_fill_pages(image) + CT_AGENT_SPARE }

/// Whether the armed lease `lease` filled its handle table: its usage (the kernel's count) holds a
/// full fill's pages and is still under its limit, so the fill ended on the table, not the pages.
fn filled(image: &Image, lease: u32) -> bool {
    rd::usage(lease).is_ok_and(|u| u.pages_usage >= ct_fill_pages(image) && u.pages_usage < u.pages_limit)
}
const CT_LEASE_WEIGHT: u32 = 10;
const CT_LEND_PAGES: usize = 2;
/// The sub-agent's later deadline, from the agent's own start. It is far past any lease deadline,
/// so the lease dies first and this never fires: a child's later deadline never outlives it.
const CT_SUBLEASE_US: u64 = 600_000_000;
/// Slot H's decision: this long after the agent armed, the steward destroys it by hand.
const CT_DECISION_US: u64 = 5_000;
/// Leases per slot (kernel/README.md, "Containment"), not counting a slot D lease made again.
pub const CT_LEASES: usize = 9;
/// The most leases the steward makes, the ones made again included: its tables' size.
const CT_MAX_LEASES: usize = 64;
/// The slot D deadline is calibrated from the time the slot H agent took to arm: it is set to
/// this many times that, plus a margin, so the fill finishes first and the lease's end is the
/// measured destruction. [`CT_ARM_FALLBACK_US`] is the first round's guess.
const CT_ARM_FACTOR: u64 = 4;
const CT_D_MARGIN_US: u64 = 200_000;
const CT_ARM_FALLBACK_US: u64 = 2_000_000;
const CT_SPINS: usize = 3;
/// The agent's threads that call the victim with a lend, each one call held open.
pub const CT_CALLERS: usize = 4;
const CT_SENDERS: usize = 4;
const CT_SUB_ROUNDS: usize = 32;
const CT_SUB_CHILDREN: usize = 4;

// Badges: one distinct per report path (rule F). A lease's call and progress handles carry
// its sequence number, which the steward chose when it minted them, so the victim and the
// steward tell leases apart by what the kernel delivers, never by what an agent writes.
const CT_CALL_BADGE: u64 = 0x1000;
const CT_PROGRESS_BADGE: u64 = 0x2000;
const CT_SEND_BADGE: u64 = 0x11;
const CT_CARRIED: u64 = 0x21;
const CT_STEWARD: u64 = 0xC0;
const CT_GIFT_SEND: u64 = 0x30;
const CT_QUEUED_SEND: u64 = 0x20;

// Progress words.
const CT_SUB: usize = 1;
const CT_ARMED: usize = 2;
// Control words.
const CT_END: usize = 1;
const CT_CHECK: usize = 2;
const CT_DONE: usize = 3;

/// The tag of the steward's verdict report on its slot 1 (the only line a hostile agent's lease
/// could otherwise write to is progress, never a report).
pub const CT_VERDICT: usize = 7;
/// The tag of the steward's report of the bystander's window: `[start, start, length, tag]` (µs,
/// the start in halves).
pub const CT_STEADY: usize = 8;
/// The bystander's window, µs (kernel/README.md, "Containment"): opened once both slots' leases
/// and their sub-agents run, cut short so that it closes before slot D's deadline.
const CT_STEADY_US: u64 = 10_000_000;
/// The shortest window the steward accepts.
const CT_STEADY_MIN_US: u64 = 1_000_000;

// Handle slots each role receives (Bench::start: 1 report, 2 go, extras from 3).
const CT_V_CALL: u32 = 3;
const CT_V_SEND_D: u32 = 4;
const CT_V_SEND_H: u32 = 5;
const CT_V_QUEUED_D: u32 = 6;
const CT_V_QUEUED_H: u32 = 7;
const CT_V_BUDGET: u32 = 8;
const CT_S_SESSIONS: u32 = 3;
const CT_S_CALL: u32 = 4;
const CT_S_SEND_D: u32 = 5;
const CT_S_SEND_H: u32 = 6;
const CT_S_PROGRESS: u32 = 7;
const CT_S_GIFT: u32 = 8;
const CT_S_COUNT: u32 = 9;
const CT_B_GIFT: u32 = 3;
const CT_B_QUEUED_D: u32 = 4;
const CT_B_QUEUED_H: u32 = 5;
const CT_B_COUNT: u32 = 6;
const CT_M_HANDOFF: u32 = 1;

/// What a lend of the agent's carries: a function of the lease, the caller and the page.
fn lend_pattern(seq: u64, caller: u64, page: u64) -> u64 {
    0x9e37_79b9_0000_0000 | (seq << 32) | (caller << 24) | page
}

fn usage_pages(budget: u32) -> u64 { rd::usage(budget).map(|u| u.pages_usage).unwrap_or(u64::MAX) }

/// Whether a handle the steward held is gone after the budget that stamped or named it died.
fn gone(handle: u32) -> bool { matches!(rd::usage(handle), Err(rd::Error::BadHandle)) }

/// A reply the caller no longer waits for: `discarded`, mask 0.
fn discarded(msg_id: u64) -> bool {
    let rec = rd::body([0; 4]).encode();
    let Some(id) = core::num::NonZeroU64::new(msg_id) else { return false };
    matches!(
        redoubt_sys::syscall(&rd::Call::Reply { msg_id: id, body_rec: rec.as_ptr() as usize }),
        Ok(rd::Return::Reply(o)) if !o.delivered && o.installed == 0
    )
}

// --- The hostile agent's threads ---

/// Spin on `rdtime` only: never enters the kernel.
extern "C" fn agent_spinner(_: usize) -> ! {
    loop {
        spin_until(u64::MAX);
    }
}

/// Enter the kernel in a tight loop (a call that returns at once).
extern "C" fn agent_tight(_: usize) -> ! {
    loop {
        let _ = rd::time_now();
    }
}

/// Call the victim with a two-page lend the victim holds open; the caller is killed at the
/// lease's end, which abandons the call (R3).
extern "C" fn agent_caller(arg: usize) -> ! {
    let seq = param(0);
    let caller = arg as u64;
    let pages = rd::many_pages(CT_LEND_PAGES);
    for p in 0..CT_LEND_PAGES as u64 {
        rd::poke(pages + p as usize * rd::PAGE_SIZE, lend_pattern(seq, caller, p));
    }
    let _ = rd::call_outcome(
        1,
        &rd::body([caller as usize, 0, 0, 0]),
        rd::pages(pages, CT_LEND_PAGES),
        rd::FOREVER,
    );
    crate::park()
}

/// Block in `send` on the lease's send endpoint: nothing receives there, so it stays blocked
/// until the lease dies and fails it with `Dead`.
extern "C" fn agent_sender(_: usize) -> ! {
    let _ = rd::send(2, &rd::body([7, 0, 0, 0]), None, rd::FOREVER);
    crate::park()
}

/// The agent's process: start its threads, carve the sub-budget, fill the lease and its handle
/// table, then arm and spin. Slots: 1 call, 2 send, 3 progress, 4 the lease budget. Parameter
/// 0 is the lease's sequence number, which its lends carry.
fn agent_main() -> ! {
    for _ in 0..CT_SPINS {
        thread(agent_spinner, 0);
    }
    thread(agent_tight, 0);
    for c in 0..CT_CALLERS {
        thread(agent_caller, c);
    }
    for _ in 0..CT_SENDERS {
        thread(agent_sender, 0);
    }
    let sub = rd::create(
        4,
        &rd::BudgetSpec {
            deadline: rd::time_now().unwrap_or(0) + CT_SUBLEASE_US,
            ..rd::spec(CT_SUB_PAGES, 2, 1)
        },
    )
    .expect("the sub-budget");
    let _ = rd::send(3, &rd::body_with([CT_SUB, 0, 0, 0], &[sub]), None, rd::FOREVER);
    // The full fill (kernel/README.md, "Containment"): endpoints until the kernel refuses one,
    // which, in a lease of `ct_lease_pages`, is the full handle table; then mints from the first
    // endpoint, which the full table refuses too. The cap is only a safety net; the steward judges
    // the fill from the lease's usage, never from this agent.
    let first = rd::endpoint_create().expect("the first endpoint");
    let mut eps = 1;
    while eps < redoubt_sys::MAX_HANDLES && rd::endpoint_create().is_ok() {
        eps += 1;
    }
    let mut mints = 0;
    while mints < redoubt_sys::MAX_HANDLES && rd::mint_from_handle(first, 0x500 + mints as u64, None).is_ok()
    {
        mints += 1;
    }
    let _ = rd::send(3, &rd::body([CT_ARMED, 0, 0, eps | mints << 20]), None, rd::FOREVER);
    loop {
        spin_until(u64::MAX);
    }
}

// --- The sub-agent: bounded thread and process churn ---

static SUB_CHURNED: AtomicUsize = AtomicUsize::new(0);

extern "C" fn sub_worker(_: usize) -> ! {
    let _ = spin_until(ticks() + 2_000);
    SUB_CHURNED.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    crate::park()
}

/// The agent's sub-agent, in the sub-budget (slot 1 is that budget's handle): a bounded number
/// of thread create/exit rounds and child processes, then spin. The bound is what keeps the
/// kernel trace ring from dropping records.
fn sub_agent_main() -> ! {
    for round in 0..CT_SUB_ROUNDS {
        thread_on(30 + round % 2, sub_worker, 0);
        while SUB_CHURNED.load(SeqCst) < round + 1 {
            let _ = rd::receive(None, 1_000, 0);
        }
    }
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("the sub-agent's exit endpoint");
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::ChurnChild as u8;
    startup[1..9].copy_from_slice(&2_000u64.to_le_bytes());
    startup[9..17].copy_from_slice(&0u64.to_le_bytes());
    startup[17..25].copy_from_slice(&1u64.to_le_bytes());
    for _ in 0..CT_SUB_CHILDREN {
        if spawn::spawn(&image, 1, exit, child as *const () as usize, &startup, &[]).is_err() {
            break;
        }
        let _ = rd::receive(Some(exit), rd::FOREVER, 0);
    }
    loop {
        spin_until(u64::MAX);
    }
}

// --- The endpoint maker: run once in `users`, then exit ---

/// Make the victim's endpoints and the steward's progress endpoint (all owned and stamped by
/// `users`, which is where this runs), hand out copies, and exit: an endpoint outlives its maker.
fn endpoint_maker() -> ! {
    let handoff = CT_M_HANDOFF;
    let call = rd::endpoint_create().expect("call");
    let send_d = rd::endpoint_create().expect("send_d");
    let send_h = rd::endpoint_create().expect("send_h");
    let queued_d = rd::endpoint_create().expect("queued_d");
    let queued_h = rd::endpoint_create().expect("queued_h");
    let progress = rd::endpoint_create().expect("progress");
    let gift = rd::endpoint_create().expect("gift");
    let gift_send = rd::mint_from_handle(gift, CT_GIFT_SEND, None).expect("gift_send");
    let queue_d_send = rd::mint_from_handle(queued_d, CT_QUEUED_SEND, None).expect("queue_d");
    let queue_h_send = rd::mint_from_handle(queued_h, CT_QUEUED_SEND, None).expect("queue_h");
    // Victim (tags 1 and 5), steward (2 and 3), bystander (4).
    let msg = |tag: usize, hs: &[u32]| {
        let _ = rd::send(handoff, &rd::body_with([tag, 0, 0, 0], hs), None, rd::FOREVER);
    };
    msg(1, &[call, send_d, send_h, queued_d]);
    msg(5, &[queued_h]);
    msg(2, &[call, send_d, send_h, progress]);
    msg(3, &[gift_send]);
    msg(4, &[gift, queue_d_send, queue_h_send]);
    rd::process_exit(0)
}

// --- The victim server ---

#[derive(Clone, Copy)]
struct Hold {
    msg_id: u64,
    seq: u64,
    caller: u64,
    lend: usize,
    notices: u32,
}

/// A control call from the steward on the call endpoint; a distinct badge names it (rule F).
fn victim_control(ctl: u32, cmd: usize, a: usize, b: usize) {
    let _ = rd::call_waiting(ctl, &rd::body([cmd, a, b, 0]), None, rd::FOREVER);
}

/// The victim server: take and hold the agents' lend calls, each under the badge of the lease
/// that made it, and log one abandoned notice per id (a second is a failure). It answers the
/// steward's controls: at a slot's lease end ([`CT_END`]: slot, round), drain the slot's send
/// endpoint and take the bystander's queued message (its carried handle must be 0); at a check
/// ([`CT_CHECK`]: below), check every held call of a lease numbered below it (one notice, bytes
/// intact, reply discarded mask 0); at [`CT_DONE`], report its failures, its controls, the calls
/// it checked and the ones it still holds.
fn victim() -> ! {
    let call_rx = CT_V_CALL;
    let send_rx = [CT_V_SEND_D, CT_V_SEND_H];
    let q_rx = [CT_V_QUEUED_D, CT_V_QUEUED_H];
    let budget = CT_V_BUDGET;
    let start_usage = usage_pages(budget);
    // Room for the held calls of two rounds, slot D leases made again included.
    let mut holds = [Hold { msg_id: 0, seq: 0, caller: 0, lend: 0, notices: 0 }; 64];
    let mut n = 0usize;
    let mut fails = 0u64;
    let mut controls = 0u64;
    let mut checked = 0u64;
    // Each lease's checked calls, and the leases below `done` already judged: every lease made
    // exactly `CT_CALLERS` calls, so one lease's extra call cannot hide another's missing one.
    let mut per_lease = [0usize; CT_MAX_LEASES];
    let mut done = 0usize;
    let leases = CT_CALL_BADGE..CT_CALL_BADGE + CT_MAX_LEASES as u64;
    loop {
        match rd::receive(Some(call_rx), rd::FOREVER, 0) {
            Ok(Received::Message(m)) => match m.kind {
                rd::MessageKind::Call { lend: Some(pages) } if leases.contains(&m.badge) => {
                    if n < holds.len() {
                        holds[n] = Hold {
                            msg_id: m.msg_id.get(),
                            seq: m.badge - CT_CALL_BADGE,
                            caller: m.body.words[0] as u64,
                            lend: pages.addr,
                            notices: 0,
                        };
                        n += 1;
                    } else {
                        fails |= 1;
                    }
                }
                rd::MessageKind::Call { .. } if m.badge == CT_STEWARD => {
                    let w = m.body.words;
                    if w[0] == CT_DONE {
                        // Reply to the control first: its open-call page is the receiver's, and
                        // the usage check must see it freed (I10).
                        let _ = rd::reply(m.msg_id.get(), &rd::body([0; 4]));
                        if usage_pages(budget) != start_usage {
                            fails |= 128;
                        }
                        let _ = rd::send(
                            1,
                            &rd::body([fails as usize, controls as usize, checked as usize, n]),
                            None,
                            rd::FOREVER,
                        );
                        rd::process_exit(0)
                    }
                    if w[0] == CT_END && w[1] > 1 {
                        fails |= 2;
                    } else if w[0] == CT_END {
                        let slot = w[1];
                        // Nothing a blocked send carried is here after the lease's end.
                        if !matches!(rd::receive(Some(send_rx[slot]), 0, 0), Err(rd::Error::Timeout)) {
                            fails |= 4;
                        }
                        // The bystander's queued message arrived with its stamped handle as 0.
                        let mut qok = false;
                        while let Ok(Received::Message(qm)) = rd::receive(Some(q_rx[slot]), 0, 0) {
                            if qm.body.words[0] == w[2]
                                && qm.body.handles.as_slice().first().copied().flatten().is_none()
                            {
                                qok = true;
                            }
                        }
                        if !qok {
                            fails |= 8;
                        }
                    } else if w[0] == CT_CHECK {
                        let below = w[1] as u64;
                        let mut i = 0;
                        while i < n {
                            if holds[i].seq < below {
                                let (at, seq, caller) = (holds[i].lend, holds[i].seq, holds[i].caller);
                                if holds[i].notices != 1 {
                                    fails |= 16;
                                }
                                if rd::peek(at) != lend_pattern(seq, caller, 0)
                                    || rd::peek(at + rd::PAGE_SIZE) != lend_pattern(seq, caller, 1)
                                {
                                    fails |= 32;
                                }
                                if !discarded(holds[i].msg_id) {
                                    fails |= 64;
                                }
                                checked += 1;
                                per_lease[seq as usize] += 1;
                                holds[i] = holds[n - 1];
                                n -= 1;
                            } else {
                                i += 1;
                            }
                        }
                        let below = (below as usize).min(CT_MAX_LEASES);
                        if per_lease[done.min(below)..below].iter().any(|&c| c != CT_CALLERS) {
                            fails |= 256;
                        }
                        done = done.max(below);
                    } else {
                        fails |= 2;
                    }
                    controls += 1;
                    let _ = rd::reply(
                        m.msg_id.get(),
                        &rd::body([w[0], fails as usize, controls as usize, checked as usize]),
                    );
                }
                _ => {}
            },
            Ok(Received::Abandoned(id)) => {
                for h in holds[..n].iter_mut() {
                    if h.msg_id == id.get() {
                        h.notices += 1;
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
}

// --- The bystander ---

/// The bystander's counting thread: wait for its window from the steward (`[start, end]` in µs,
/// in halves), count to its end, report, then end this thread. It receives only on its own
/// endpoint, so it never takes a gift meant for the relay.
extern "C" fn bystander_count(_: usize) -> ! {
    let end = match rd::receive(Some(CT_B_COUNT), rd::FOREVER, 0) {
        Ok(Received::Message(m)) => join(m.body.words[2], m.body.words[3]),
        _ => 0,
    };
    let left = end.saturating_sub(rd::time_now().unwrap_or(end));
    let count = spin_until(ticks() + left * tpu());
    let _ = rd::send(1, &rd::body([count as usize, 0, 0, 0]), None, rd::FOREVER);
    rd::thread_exit().ok();
    crate::park()
}

/// The bystander: two threads relay the leases' carried handles for the whole run, one more counts
/// its share over the window the steward opens. A relay takes a lease-stamped handle from the
/// steward and queues it to the victim through an unstamped handle, so the lease's death revokes it
/// in flight and the victim receives it as 0 (R10, R9). A relay's send waits until the victim takes
/// the message, at that lease's end; two leases are live at once, so two relays keep the steward
/// from waiting on one lease's end to give the other its handle.
fn bystander() -> ! {
    thread(bystander_count, 0);
    thread(bystander_relay, 0);
    bystander_relay(0)
}

extern "C" fn bystander_relay(_: usize) -> ! {
    loop {
        if let Ok(Received::Message(m)) = rd::receive(Some(CT_B_GIFT), rd::FOREVER, 0) {
            let w = m.body.words;
            if let Some(h) = m.body.handles.as_slice().first().copied().flatten() {
                let q = if w[1] == 0 { CT_B_QUEUED_D } else { CT_B_QUEUED_H };
                let _ =
                    rd::send(q, &rd::body_with([w[0], w[1], w[2], w[3]], &[h.index()]), None, rd::FOREVER);
            }
        }
    }
}

// --- The steward stand-in ---

/// The steward's leases, by sequence number: each lease's slot, its sub-agent's budget (0 until
/// one starts) and whether it armed. A lease's number is in the badges of its handles, so its
/// agent can speak only for itself.
struct Leases {
    made: usize,
    slot: [usize; CT_MAX_LEASES],
    sub: [u32; CT_MAX_LEASES],
    armed: [bool; CT_MAX_LEASES],
}

/// One lease in the steward's hands: its budget, its sequence number and the handles minted into
/// it (call, send, progress).
struct Lease {
    budget: u32,
    seq: usize,
    handles: [u32; 3],
}

impl Lease {
    /// Whether the lease and every handle the steward minted into it are gone.
    fn gone(&self) -> bool { gone(self.budget) && self.handles.iter().all(|&h| gone(h)) }
}

/// Make a lease in `sessions` for `slot`, with `deadline` (`rd::FOREVER` for none), and start its agent: the
/// lease's call, send and progress handles are minted from the `users`-stamped receive rights the
/// maker handed over, stamped with the lease so they die with it, and badged with its number.
fn make_lease(
    ls: &mut Leases,
    image: &Image,
    sessions: u32,
    rx: (u32, u32, u32),
    exit: u32,
    slot: usize,
    deadline: u64,
) -> Lease {
    let seq = ls.made;
    assert!(seq < CT_MAX_LEASES, "the steward's lease table is full");
    ls.made += 1;
    (ls.slot[seq], ls.sub[seq], ls.armed[seq]) = (slot, 0, false);
    let spec = rd::BudgetSpec { deadline, ..rd::spec(ct_lease_pages(image), 4, CT_LEASE_WEIGHT) };
    let budget = rd::create(sessions, &spec).expect("a lease");
    let badge = seq as u64;
    let handles = [
        rd::mint_from_handle(rx.0, CT_CALL_BADGE + badge, Some(budget)).expect("mint call"),
        rd::mint_from_handle(rx.1, CT_SEND_BADGE, Some(budget)).expect("mint send"),
        rd::mint_from_handle(rx.2, CT_PROGRESS_BADGE + badge, Some(budget)).expect("mint progress"),
    ];
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::Agent as u8;
    startup[1..9].copy_from_slice(&badge.to_le_bytes());
    let all = [handles[0], handles[1], handles[2], budget];
    spawn::spawn(image, budget, exit, containment_child as *const () as usize, &startup, &all)
        .expect("the agent");
    Lease { budget, seq, handles }
}

/// Give the bystander this lease's carried handle (minted into the lease, so it dies with it);
/// whether the lease was still there to mint into.
fn give_carried(gift: u32, call_rx: u32, lease: &Lease, slot: usize, r: usize) -> bool {
    let Ok(carried) = rd::mint_from_handle(call_rx, CT_CARRIED, Some(lease.budget)) else { return false };
    let _ = rd::send(gift, &rd::body_with([r, slot, 0, 0], &[carried]), None, rd::FOREVER);
    true
}

/// Pump one progress message: start the sub-agent in the sub-budget the agent hands over (once
/// per lease), or note that its lease armed. The lease is the one the badge names; anything else
/// is dropped. Progress is at most progress: no verdict is ever read from the agent.
fn pump_progress(progress_rx: u32, image: &Image, exit: &[u32; 2], ls: &mut Leases) {
    let Ok(Received::Message(m)) = rd::receive(Some(progress_rx), 50_000, 0) else { return };
    let Some(seq) = m.badge.checked_sub(CT_PROGRESS_BADGE).map(|s| s as usize) else { return };
    if seq >= ls.made {
        return;
    }
    match m.body.words[0] {
        CT_SUB if ls.sub[seq] == 0 => {
            if let Some(h) = m.body.handles.as_slice().first().copied().flatten() {
                let mut st = [0u8; 1 + 8 * 8];
                st[0] = Role::SubAgent as u8;
                // Its handle is kept only if it started: a started sub-agent owes a notice.
                let sub = h.index();
                let to = exit[ls.slot[seq]];
                if spawn::spawn(image, sub, to, containment_child as *const () as usize, &st, &[sub]).is_ok()
                {
                    ls.sub[seq] = sub;
                }
            }
        }
        CT_ARMED => ls.armed[seq] = true,
        _ => {}
    }
}

/// Wait until `lease` has armed, pumping progress for every lease: how long from `since` it
/// took, µs. `None` if the lease is gone first (its deadline came before it armed). There is no
/// other way out: a full fill takes as long as it takes.
fn wait_arm(
    progress_rx: u32,
    image: &Image,
    exit: &[u32; 2],
    ls: &mut Leases,
    lease: &Lease,
    since: u64,
) -> Option<u64> {
    while !ls.armed[lease.seq] {
        if gone(lease.budget) {
            return None;
        }
        pump_progress(progress_rx, image, exit, ls);
    }
    Some(rd::time_now().unwrap_or(since).saturating_sub(since))
}

/// The notices a lease owes: its agent's, and its sub-agent's if one started.
fn owed(ls: &Leases, lease: &Lease) -> u64 { 1 + u64::from(ls.sub[lease.seq] != 0) }

/// Take a lease's `want` killed notices, blaming nobody. With `sample`, record each one's window
/// from the lease's `deadline`. Returns how many notices were taken.
fn take_notices(
    exit: u32,
    deadline: Option<u64>,
    want: u64,
    sample: Option<(&mut [Window], &mut usize)>,
    fails: &mut u64,
) -> u64 {
    // A deadline notice can only arrive at or after the deadline; wait for it plus a margin, in
    // one receive, so that the stand-in is blocked on the exit endpoint when the notice comes
    // (a shorter poll that ran out near the deadline would leave it runnable behind others).
    let until = deadline.unwrap_or_else(|| rd::time_now().unwrap_or(0)) + 5_000_000;
    let mut sample = sample;
    let mut got = 0;
    while got < want {
        let left = until.saturating_sub(rd::time_now().unwrap_or(0));
        if left == 0 {
            *fails |= 8;
            break;
        }
        match rd::receive(Some(exit), left, 0) {
            Ok(Received::Exit(n)) => {
                if n.cause != rd::Cause::Killed
                    || n.blamed_account != 0
                    || !n.blamed_labels.as_slice().is_empty()
                {
                    *fails |= 8;
                }
                if let (Some(d), Some((notice, nn))) = (deadline, sample.as_mut()) {
                    if **nn < notice.len() {
                        let end = rd::time_now().unwrap_or(d);
                        notice[**nn] = Window { end, gross: end.saturating_sub(d) };
                        **nn += 1;
                    }
                }
                got += 1;
            }
            _ => {
                *fails |= 8;
                break;
            }
        }
    }
    got
}

/// The steward stand-in: two slots, each with its own exit endpoint. Slot D is ended by its
/// deadline while slot H still lives; slot H is then ended by hand (the decision wake). It reuses
/// the latency stand-in's measuring code rather than copying it.
fn containment() -> ! {
    let sessions = CT_S_SESSIONS;
    let call_rx = CT_S_CALL;
    let send_rx = [CT_S_SEND_D, CT_S_SEND_H];
    let progress_rx = CT_S_PROGRESS;
    let gift = CT_S_GIFT;
    // Only a receive right mints; the control handle is this steward's own, badged apart.
    let ctl = rd::mint_from_handle(call_rx, CT_STEWARD, None).expect("the control handle");
    let now = || rd::time_now().unwrap_or(0);
    // Each sample's window is held until the launcher asks ([`Bench::samples`]).
    let mut wake = [Window::default(); 64];
    timer_wakes(&mut wake);

    let image = spawn::image();
    let exit = [rd::endpoint_create().expect("exit D"), rd::endpoint_create().expect("exit H")];
    let (mut destroy, mut nd) = ([0u64; CT_LEASES * 2], 0usize);
    let (mut decision, mut nw) = ([Window::default(); CT_LEASES * 2], 0usize);
    let (mut notice, mut nn) = ([Window::default(); MAX_SAMPLES], 0usize);
    let mut ls =
        Leases { made: 0, slot: [0; CT_MAX_LEASES], sub: [0; CT_MAX_LEASES], armed: [false; CT_MAX_LEASES] };
    let mut retried = 0usize;
    let mut fails = 0u64;

    let mut arm_hint = CT_ARM_FALLBACK_US;
    for r in 0..CT_LEASES {
        let base = usage_pages(sessions);
        let first = ls.made;
        // Slot H (1): no deadline; ended by the steward's decision. It is created and armed first,
        // so the time its agent took to arm calibrates slot D's deadline: the fill takes time, so
        // the deadline must land after the agent arms for the lease's end to be the destruction.
        let h_created = now();
        let h = make_lease(
            &mut ls,
            &image,
            sessions,
            (call_rx, send_rx[1], progress_rx),
            exit[1],
            1,
            rd::FOREVER,
        );
        let h_arm = wait_arm(progress_rx, &image, &exit, &mut ls, &h, h_created);
        // Its carried handle queued while it lives, so its end revokes it in flight.
        if h_arm.is_none() || !give_carried(gift, call_rx, &h, 1, r) {
            fails |= 16;
        }
        if h_arm.is_some() && !filled(&image, h.budget) {
            fails |= 32;
        }
        let h_arm = h_arm.unwrap_or(arm_hint);
        // Slot D (0): its deadline is calibrated from the measured arming time, so the fill
        // finishes first and its deadline notice is the measured destruction. A deadline is set
        // at creation, so a lease whose deadline still comes before it arms, or before its
        // carried handle is given, is taken down unsampled and made again with twice the lead.
        let mut lead = h_arm.saturating_mul(CT_ARM_FACTOR).max(CT_D_MARGIN_US);
        let (d, d_deadline, d_arm) = loop {
            let d_created = now();
            let d_deadline = d_created + lead + CT_D_MARGIN_US;
            let d = make_lease(
                &mut ls,
                &image,
                sessions,
                (call_rx, send_rx[0], progress_rx),
                exit[0],
                0,
                d_deadline,
            );
            if let Some(t) = wait_arm(progress_rx, &image, &exit, &mut ls, &d, d_created) {
                if !filled(&image, d.budget) {
                    fails |= 32;
                }
                if give_carried(gift, call_rx, &d, 0, r) {
                    break (d, d_deadline, t);
                }
            }
            take_notices(exit[0], Some(d_deadline), owed(&ls, &d), None, &mut fails);
            retried += 1;
            lead = lead.saturating_mul(2);
        };
        arm_hint = arm_hint.max(h_arm).max(d_arm);
        // The bystander's window: both slots' leases and sub-agents now run, and until slot D's
        // deadline no budget under `users` is made, carved or ended. It closes a margin before
        // that deadline.
        if r == 0 {
            let start = now();
            let end = (start + CT_STEADY_US).min(d_deadline.saturating_sub(CT_D_MARGIN_US));
            if end < start + CT_STEADY_MIN_US {
                fails |= 64;
            }
            let end = end.max(start + CT_STEADY_MIN_US);
            let ([a, b], [c, d]) = (halves(start), halves(end));
            let _ = rd::send(CT_S_COUNT, &rd::body([a, b, c, d]), None, rd::FOREVER);
            let length = (end - start) as usize;
            let _ = rd::send(1, &rd::body([a, b, length, CT_STEADY | 1 << 8]), None, rd::FOREVER);
        }
        // D: its deadline ends it while H still lives, so two hostile leases are live at the end.
        let sample = Some((&mut notice[..], &mut nn));
        take_notices(exit[0], Some(d_deadline), owed(&ls, &d), sample, &mut fails);
        if !d.gone() {
            fails |= 2;
        }
        victim_control(ctl, CT_END, 0, r);
        // H: the timeout that is the decision, then destroy.
        let before = now();
        let _ = rd::receive(None, CT_DECISION_US, 0);
        let end = now();
        decision[nw] = Window { end, gross: end.saturating_sub(before + CT_DECISION_US) };
        nw += 1;
        let t0 = now();
        if rd::destroy(h.budget).is_ok() {
            destroy[nd] = now() - t0;
            nd += 1;
        } else {
            fails |= 1;
        }
        take_notices(exit[1], None, owed(&ls, &h), None, &mut fails);
        if !h.gone() {
            fails |= 2;
        }
        victim_control(ctl, CT_END, 1, r);
        // I10: every lease of the round is gone and its notices taken, so sessions' usage is
        // exactly what it was before the round.
        if usage_pages(sessions) != base {
            fails |= 4;
        }
        // The held calls of the rounds before, whose freed frames this round's leases reused.
        victim_control(ctl, CT_CHECK, first, 0);
    }
    // The last round's held calls have no later lease to reuse their frames; check them now.
    victim_control(ctl, CT_CHECK, ls.made, 0);
    victim_control(ctl, CT_DONE, 0, 0);

    report_windows(&decision[..nw], Stats::DECISION_WAKE);
    Stats::report(&mut destroy[..nd], Stats::DESTROY);
    report_windows(&notice[..nn], Stats::DEADLINE);
    let _ = rd::send(1, &rd::body([fails as usize, retried, nn, CT_VERDICT | 1 << 8]), None, rd::FOREVER);
    hand_over(&[
        (Stats::TIMER_WAKE, &wake),
        (Stats::DECISION_WAKE, &decision[..nw]),
        (Stats::DEADLINE, &notice[..nn]),
    ]);
    rd::process_exit(0)
}

/// The launcher.
pub struct Bench {
    image: Image,
    exit: u32,
    rep: u32,
    go: [u32; 64],
    /// Each child's go handle, kept to ask it for its samples ([`Bench::samples`]).
    ask: [u32; 64],
    started: usize,
    /// `rdtime` ticks per microsecond.
    pub tpu: u64,
    /// Counting-loop iterations per millisecond of CPU, alone.
    pub rate: u64,
    name: &'static str,
    failed: bool,
    /// The children's entry: [`child`], or a case's own ([`Bench::set_entry`]).
    entry: usize,
}

impl Bench {
    pub fn new(name: &'static str) -> Bench {
        let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
        console::init(uart);
        let image = spawn::image();
        let exit = rd::endpoint_create().unwrap();
        let rep = rd::endpoint_create().unwrap();
        // The clock: ticks per µs, from rdtime against time_now over 50 ms of sleep.
        let (t0, u0) = (ticks(), rd::time_now().unwrap());
        let _ = rd::receive(None, 50_000, 0);
        let (t1, u1) = (ticks(), rd::time_now().unwrap());
        let d = (u1 - u0).max(1);
        let tpu = ((t1 - t0 + d / 2) / d).max(1);
        // The loop's rate alone, over 100 ms.
        let n = spin_until(ticks() + 100_000 * tpu);
        let rate = n / 100;
        let b = Bench {
            image,
            exit,
            rep,
            go: [0; 64],
            ask: [0; 64],
            started: 0,
            tpu,
            rate,
            name,
            failed: false,
            entry: child as *const () as usize,
        };
        let _ = writeln!(Console, "[{}] calibrated: {} ticks/us, {} iterations/ms", name, tpu, rate);
        b
    }

    pub fn now_us(&self) -> u64 { rd::time_now().unwrap() }

    /// A budget under `parent` with room for `processes` children.
    pub fn budget(&self, parent: u32, weight: u32, processes: u32, deadline: u64) -> u32 {
        // Each process: its image, stack, startup page, tables and threads, with room to spare,
        // so a budget nested in another fits in what its parent holds after its own process.
        let per = self.image.pages() as u64 + 96;
        let pages = per * u64::from(processes.max(1)) + 16 * u64::from(processes.max(1)) + 16;
        rd::create(parent, &rd::BudgetSpec { deadline, ..rd::spec(pages, processes, weight) })
            .expect("budget")
    }

    /// Start a child with `role` and parameters in `budget`; `extra` handles go in slots 3..
    /// Its index (its report's badge).
    pub fn start(&mut self, budget: u32, role: Role, params: &[u64], extra: &[u32]) -> usize {
        let i = self.started;
        self.started += 1;
        let rep_client = rd::mint_from_handle(self.rep, i as u64 + 1, None).unwrap();
        let go = rd::endpoint_create().unwrap();
        self.go[i] = rd::mint_from_handle(go, 1, None).unwrap();
        self.ask[i] = self.go[i];
        let mut startup = [0u8; 1 + 8 * 8];
        startup[0] = role as u8;
        for (k, p) in params.iter().enumerate().take(7) {
            startup[1 + k * 8..9 + k * 8].copy_from_slice(&p.to_le_bytes());
        }
        startup[1 + 7 * 8..].copy_from_slice(&self.tpu.to_le_bytes());
        let mut handles = [0u32; 32];
        handles[0] = rep_client;
        handles[1] = go;
        handles[2..2 + extra.len()].copy_from_slice(extra);
        spawn::spawn(&self.image, budget, self.exit, self.entry, &startup, &handles[..2 + extra.len()])
            .expect("spawn");
        i
    }

    /// Open the window for every child started: `[now + lead, now + lead + length]` µs. Returns
    /// (start, end) in µs.
    pub fn go(&mut self, lead_us: u64, length_us: u64) -> (u64, u64) {
        let (now_t, now_u) = (ticks(), self.now_us());
        let (start, end) = (now_t + lead_us * self.tpu, now_t + (lead_us + length_us) * self.tpu);
        let ([a, b], [c, d]) = (halves(start), halves(end));
        let words = [a, b, c, d];
        for i in 0..self.started {
            if self.go[i] != 0 {
                let _ = rd::send(self.go[i], &rd::body(words), None, rd::FOREVER);
                self.go[i] = 0;
            }
        }
        (now_u + lead_us, now_u + lead_us + length_us)
    }

    /// The cluster alone uses one `time_now` reading for all go payloads and the printed plan.
    pub fn go_cluster(&mut self) -> (u64, u64, u64) {
        let h = self.now_us().checked_add(200_000).expect("cluster H");
        let r = h.checked_add(50_000).expect("cluster R");
        let f = r.checked_add(WINDOW_US).expect("cluster F");
        let [h0, h1] = halves(h);
        let [f0, f1] = halves(f);
        for i in 0..self.started {
            if self.go[i] != 0 {
                rd::send(self.go[i], &rd::body([h0, h1, f0, f1]), None, rd::FOREVER).expect("cluster go");
                self.go[i] = 0;
            }
        }
        (h, r, f)
    }

    /// Collect `n` reports, by child index.
    pub fn collect(&mut self, n: usize) -> [u64; 64] {
        let mut out = [0u64; 64];
        for _ in 0..n {
            if let Ok(Received::Message(m)) = rd::receive(Some(self.rep), 60_000_000, 0) {
                let i = (m.badge as usize).saturating_sub(1);
                let w = m.body.words;
                if i < 64 {
                    out[i] = join(w[0], w[1]);
                }
            }
        }
        // Their exit notices.
        while rd::receive(Some(self.exit), 0, 0).is_ok() {}
        out
    }

    /// Collect `n` messages of raw words: for each child index, up to four messages in the order
    /// they came.
    pub fn collect_words(&mut self, n: usize) -> [[[usize; 4]; 4]; 64] {
        let mut out = [[[0usize; 4]; 4]; 64];
        let mut seen = [0usize; 64];
        for _ in 0..n {
            if let Ok(Received::Message(m)) = rd::receive(Some(self.rep), 60_000_000, 0) {
                let i = (m.badge as usize).saturating_sub(1);
                if i < 64 && seen[i] < 4 {
                    out[i][seen[i]] = m.body.words;
                    seen[i] += 1;
                }
            }
        }
        while rd::receive(Some(self.exit), 0, 0).is_ok() {}
        out
    }

    /// Cluster children each send one report. Wait for all distinct children, preserving counts
    /// so a duplicate cannot hide a missing child's report or start diagnostics mid-window.
    pub fn collect_cluster_words(&mut self, n: usize) -> ([[[usize; 4]; 4]; 64], [usize; 64]) {
        let mut out = [[[0usize; 4]; 4]; 64];
        let mut seen = [0usize; 64];
        let mut distinct = 0;
        while distinct < n {
            let Ok(Received::Message(m)) = rd::receive(Some(self.rep), 60_000_000, 0) else { break };
            let i = (m.badge as usize).saturating_sub(1);
            if i < n {
                if seen[i] == 0 {
                    distinct += 1;
                }
                if seen[i] < 4 {
                    out[i][seen[i]] = m.body.words;
                }
                seen[i] += 1;
            }
        }
        // No sample handover has begun. Count any already queued extras as duplicate reports.
        while let Ok(Received::Message(m)) = rd::receive(Some(self.rep), 0, 0) {
            let i = (m.badge as usize).saturating_sub(1);
            if i < n {
                if seen[i] < 4 {
                    out[i][seen[i]] = m.body.words;
                }
                seen[i] += 1;
            }
        }
        while rd::receive(Some(self.exit), 0, 0).is_ok() {}
        (out, seen)
    }

    /// Ask child `i`, a stand-in holding its samples, for them, once every report is collected,
    /// and print each as `LATENCY-SAMPLE <group> <measure> <end> <gross>` (µs) for the bench's
    /// post-check (`sched_oracle`), which judges the targets net of the checked build's audits.
    /// `reports` are the child's reports: each measure's count among them is printed too
    /// (`LATENCY-COUNT <group> <measure> <n>`), so a window lost on the way fails the check.
    pub fn samples(&mut self, i: usize, reports: &[[usize; 4]], group: core::fmt::Arguments) {
        for w in reports {
            if let Some(measure) = Stats::measure(w[3] & 0xff).filter(|_| w[3] >> 8 > 0) {
                let _ = writeln!(Console, "LATENCY-COUNT {} {} {}", group, measure, w[3] >> 8);
            }
        }
        let _ = rd::send(self.ask[i], &rd::body([0; 4]), None, rd::FOREVER);
        loop {
            let Ok(Received::Message(m)) = rd::receive(Some(self.rep), 60_000_000, 0) else {
                self.check(false, format_args!("{}: a stand-in's samples stopped", group));
                return;
            };
            if m.badge as usize != i + 1 {
                self.check(
                    false,
                    format_args!(
                        "{}: a report from child {} among child {}'s samples",
                        group,
                        m.badge,
                        i + 1
                    ),
                );
                return;
            }
            let w = m.body.words;
            let Some(measure) = Stats::measure(w[3]) else { return };
            let _ = writeln!(Console, "LATENCY-SAMPLE {} {} {} {}", group, measure, join(w[0], w[1]), w[2]);
        }
    }

    /// Receive the cluster's ordered per-attempt metadata and trusted latency windows after all
    /// measurement reports. The explicit index and intent make a shifted handover fail closed.
    pub fn cluster_samples(&mut self, i: usize, count: usize, tag: usize) {
        let Some(measure) = Stats::measure(tag) else {
            self.check(false, format_args!("cluster child {i}: unknown sample tag {tag}"));
            return;
        };
        if count > 200 {
            self.check(false, format_args!("cluster child {i}: {count} samples exceed 200"));
            return;
        }
        let _ = writeln!(Console, "LATENCY-COUNT cluster {measure} {count}");
        let _ = rd::send(self.ask[i], &rd::body([0; 4]), None, rd::FOREVER);
        let Some(header) = self.cluster_word(i) else { return };
        let _ = writeln!(
            Console,
            "CLUSTER-HEADER {measure} {} {}",
            join(header[0], header[1]),
            join(header[2], header[3])
        );
        for index in 0..count {
            let Some(header) = self.cluster_word(i) else { return };
            let Some(bounds) = self.cluster_word(i) else { return };
            let Some(observations) = self.cluster_word(i) else { return };
            let Some(times) = self.cluster_word(i) else { return };
            let Some(last) = self.cluster_word(i) else { return };
            let Some(window) = self.cluster_word(i) else { return };
            let intent = usize::from(index / 4 % 2 == 1);
            let (arm, deadline, service) =
                (join(times[0], times[1]), join(times[2], times[3]), join(last[0], last[1]));
            let (before, lower) = (join(bounds[0], bounds[1]), join(bounds[2], bounds[3]));
            let (early, observed) =
                (join(observations[0], observations[1]), join(observations[2], observations[3]));
            let upper = join(window[0], window[1]);
            if header[0] != index
                || header[1] != intent
                || last[2..] != [index, 0xc4]
                || window[3] != tag
                || lower != before.checked_add(header[3] as u64).unwrap_or(0)
                || observed.checked_add(1) != Some(upper)
                || upper.checked_sub(lower) != Some(window[2] as u64)
            {
                self.check(false, format_args!("cluster {measure} sample {index}: malformed metadata"));
                return;
            }
            let _ = writeln!(
                Console,
                "CLUSTER-SAMPLE {measure} {index} {intent} {} {} {before} {} {lower} {early} {observed} {upper} {arm} {deadline} {service} {} {}",
                index * 80_000,
                header[2] as u32 as i32,
                header[3],
                if tag == Stats::DRIVER_WAKE { service.saturating_sub(deadline) / 1_000 } else { 0 },
                if tag == Stats::DRIVER_WAKE { "rtc_ns" } else { "no_rtc" },
            );
            let _ = writeln!(Console, "LATENCY-SAMPLE cluster {measure} {upper} {}", window[2]);
        }
        if self.cluster_word(i) != Some([0; 4]) {
            self.check(false, format_args!("cluster {measure}: missing handover terminator"));
        }
    }

    fn cluster_word(&mut self, i: usize) -> Option<[usize; 4]> {
        match rd::receive(Some(self.rep), 60_000_000, 0) {
            Ok(Received::Message(m)) if m.badge as usize == i + 1 => Some(m.body.words),
            _ => {
                self.check(false, format_args!("cluster child {i}: a metadata word stopped or moved"));
                None
            }
        }
    }

    /// Start children at `entry` rather than [`child`]: a case with roles of its own.
    pub fn set_entry(&mut self, entry: extern "C" fn(usize) -> !) {
        self.entry = entry as *const () as usize;
    }

    /// One report message from a child: (its index, its four words). Blocks for ever; a case
    /// that must wait for the reports it names, not a count, uses this.
    pub fn receive_report(&mut self) -> Option<(usize, [usize; 4])> {
        match rd::receive(Some(self.rep), rd::FOREVER, 0) {
            Ok(Received::Message(m)) => Some(((m.badge as usize).saturating_sub(1), m.body.words)),
            _ => None,
        }
    }

    /// A count as a share of the window, in thousandths.
    pub fn share(&self, count: u64, window_us: u64) -> u64 {
        count * 1000 / (self.rate * window_us / 1000).max(1)
    }

    /// A share of the CPU the kernel charged, which the bench's post-check (`sched_oracle`) reads
    /// from the trace alone: printed as `CHARGED-SHARE <name> <start> <end> <tolerance>
    /// <mark>[:<threads>]...`, the window in µs, how far in thousandths the share may lie from what
    /// it is owed among the budgets charged beside it, and the weights of the empty budgets the
    /// program carved and destroyed to mark the budget judged (the first) and the budgets it is
    /// judged among (each mark's, and those under it), each with its runnable threads if it has
    /// fewer than the harts (its water-filling share is capped at them).
    pub fn charged_share(
        &self,
        name: &str,
        (start, end): (u64, u64),
        tolerance: u64,
        marks: &[(u32, Option<u32>)],
    ) {
        let _ = write!(Console, "CHARGED-SHARE {} {} {} {}", name, start, end, tolerance);
        for (m, k) in marks {
            let _ = match k {
                Some(k) => write!(Console, " {}:{}", m, k),
                None => write!(Console, " {}", m),
            };
        }
        let _ = writeln!(Console);
    }

    /// A share across harts of the CPU the kernel charged, which the bench's post-check
    /// (`sched_oracle`) reads from the trace alone: printed as `HART-SHARE <name> <start> <end>
    /// <tolerance>[+|-] <mark>:<threads> <weight>:<threads>...`, the window in µs, how far in
    /// thousandths the share may lie from what it is owed (`+` only below it, `-` only above), the
    /// weight of the empty budget the program carved and destroyed to mark the budget judged
    /// ([`mark`]) with its runnable threads, and the weight and runnable threads of each budget it
    /// runs against it.
    pub fn hart_share(
        &self,
        name: &str,
        (start, end): (u64, u64),
        (tolerance, side): (u64, &str),
        (mark, threads): (u32, u32),
        others: &[(u32, u32)],
    ) {
        let _ = write!(
            Console,
            "HART-SHARE {} {} {} {}{} {}:{}",
            name, start, end, tolerance, side, mark, threads
        );
        for (w, k) in others {
            let _ = write!(Console, " {}:{}", w, k);
        }
        let _ = writeln!(Console);
    }

    pub fn check(&mut self, ok: bool, what: core::fmt::Arguments) {
        self.failed |= !ok;
        let _ = writeln!(Console, "[{}] {}: {}", self.name, if ok { "ok" } else { "FAIL" }, what);
    }

    pub fn note(&mut self, what: core::fmt::Arguments) {
        let _ = writeln!(Console, "[{}] {}", self.name, what);
    }

    /// Report, power off.
    pub fn finish(self, upper: &str) -> ! {
        if !self.failed {
            let _ = writeln!(Console, "{} TEST PASSED", upper);
        }
        let _ = rd::system_reset(rd::RESET, rd::ResetKind::PowerOff);
        crate::park()
    }

    pub fn exit_endpoint(&self) -> u32 { self.exit }

    pub fn image(&self) -> &Image { &self.image }
}

/// Budget churn against an equal-weight victim (`sched-budget-churn` and its shell's case): each
/// phase is `(variant, the child's weight, name, what, the victim's mark)`, the attacker running
/// [`Role::BudgetChurn`] and the victim spinning, and the victim's share printed for the
/// post-check (`HART-SHARE`) with the counts noted beside.
pub fn churn_against_victim(b: &mut Bench, phases: &[(u64, u64, &str, &str, u32)]) {
    // The window, and how far below its share the victim may get, thousandths (R12's 50).
    const WINDOW: u64 = 2_000_000;
    const TOL: u64 = 50;
    for &(variant, weight, name, what, m) in phases {
        // Room for the attacker, its children and (variant 3) intermediates.
        let attacker = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        mark(victim, m);
        let a = b.start(attacker, Role::BudgetChurn, &[variant, weight], &[attacker]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let window = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        // Every variant is judged by the victim, who keeps at least half; the shell's own share
        // pays for its calls at its halved weight, so the victim may get more (no ceiling). The
        // post-check judges the victim's share of the kernel's charges, which bill the checked
        // build's audits to no one, and recomputes every lift. The attacker's subtree runs at
        // most two threads, its own and a child's.
        b.hart_share(name, window, (TOL, "+"), (m, 1), &[(100, 2)]);
        b.note(format_args!(
            "{}: the victim counted {} of 1000 of the window, the attacker's subtree {}",
            what,
            b.share(counts[v], window.1 - window.0),
            b.share(counts[a], window.1 - window.0)
        ));
        rd::destroy(attacker).unwrap();
        rd::destroy(victim).unwrap();
    }
}

/// `Error` re-exported for the cases.
pub type E = Error;
