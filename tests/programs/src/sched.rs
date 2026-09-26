//! The scheduling cases (WP-K5; KERNEL-SPEC.md R7, R12; RESOURCES.md, Attack tests).
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
//! window begins at the same instant.

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use uart_16550::MmioSerialPort;

use crate::rd::{self, Error, Received};
use crate::spawn::{self, Image};

/// Iterations between clock checks in the counting loop.
const CHUNK: u64 = 256;
/// The slice (KERNEL-SPEC.md, Constants), in microseconds.
pub const SLICE_US: u64 = 10_000;

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
    /// with p3 = 1, also create budgets with staggered deadlines under slot 3. Report 0.
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
    /// 3, interrupt in slot 4), each waited for in `receive`; report [`Stats::DRIVER_WAKE`].
    Driver = 14,
    /// The latency case's steward stand-in, with leases carved from slot 3: p0 timeout wakes,
    /// p1 leases destroyed by hand (each after a timeout: its decision) and p1 destroyed by their
    /// deadlines; report [`Stats::TIMER_WAKE`], [`Stats::DESTROY`], [`Stats::DECISION_WAKE`] and
    /// [`Stats::DEADLINE`].
    Steward = 15,
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
        ]
        .into_iter()
        .find(|r| *r as u8 == x)
    }
}

/// `rdtime`.
pub fn ticks() -> u64 { crate::read_time() }

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

/// Wait on slot 2 for the window: (start, end) in `rdtime` ticks.
fn window() -> (u64, u64) {
    let Ok(Received::Message(m)) = rd::receive(Some(2), rd::FOREVER, 0) else { rd::process_exit(90) };
    let w = m.body.words;
    (join(w[0], w[1]), join(w[2], w[3]))
}

/// The launcher's `rdtime` ticks per µs, the last parameter of every child.
fn tpu() -> u64 { param(7).max(1) }

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

/// Wait (sleeping) until `n` threads are done.
fn await_done(n: usize) {
    while DONE.load(SeqCst) < n {
        let _ = rd::receive(None, 1_000, 0);
    }
}

/// A child's entry: its role from the startup block.
pub extern "C" fn child(arg: usize) -> ! {
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
    let (start, end) = window();
    let tpu = tpu();
    set_end(end);
    sleep_until(start, tpu);
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
            driver(param(0) as usize);
            rd::process_exit(0)
        }
        Some(Role::Steward) => {
            steward(param(0) as usize, param(1) as usize);
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
        Some(Role::BudgetChurn) => budget_churn(end, tpu),
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
        // in between. Creating and destroying moves nothing (OWNER DECISION 6): the budget keeps
        // its share. (A lift that counted the entry wait, from the floor, would grow the lead by
        // half again at each one.)
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

/// The driver stand-in: `k` alarms, each a little over 2 ms ahead (phases spread over a
/// millisecond); how late, on the RTC's own clock, it runs again after each.
fn driver(k: usize) {
    let Ok((base, _)) = rd::map_device(3) else { return };
    let mut late = [0u64; MAX_SAMPLES];
    let (k, mut n, mut lost) = (k.min(MAX_SAMPLES), 0, 0usize);
    for i in 0..2 * k {
        if n == k {
            break;
        }
        let at = rtc::now_ns(base) + 2_000_000 + (i as u64 * 397_000) % 1_000_000;
        rtc::alarm(base, at);
        // An alarm that fires while the source is masked (the driver's slice ended before it
        // got back into `receive`) may never be delivered (K5-code-review-5, R5: K3's
        // follow-up); give up on it after 100 ms, count it, and arm again.
        if matches!(rd::receive(Some(4), 100_000, 0), Ok(Received::Interrupt)) {
            late[n] = rtc::now_ns(base).saturating_sub(at) / 1000;
            n += 1;
        } else {
            lost += 1;
        }
        rtc::clear(base);
    }
    Stats::report(&mut late[..n], Stats::DRIVER_WAKE);
    let _ = rd::send(1, &rd::body([lost, 0, 0, Stats::DRIVER_LOST | 1 << 8]), None, rd::FOREVER);
}

/// The steward stand-in: `k` timeout wakes (how late it runs again after each deadline); `leases`
/// one-process leases carved from slot 3, each destroyed by hand after a timeout (how long
/// `budget_destroy` takes); and `leases` more destroyed by their deadlines (how late the killed
/// notice arrives).
fn steward(k: usize, leases: usize) {
    let now = || rd::time_now().unwrap_or(0);
    let (k, leases) = (k.min(MAX_SAMPLES), leases.min(MAX_SAMPLES));
    let mut wake = [0u64; MAX_SAMPLES];
    for (i, sample) in wake[..k].iter_mut().enumerate() {
        let timeout = 3_000 + (i as u64 * 397) % 1_000;
        let before = now();
        let _ = rd::receive(None, timeout, 0);
        *sample = now().saturating_sub(before + timeout);
    }
    Stats::report(&mut wake[..k], Stats::TIMER_WAKE);
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit");
    let pages = image.pages() as u64 + 96;
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::Spin as u8;
    // A lease's spinner counts until a window that never ends (its lease ends first).
    let (mut destroy, mut nd) = ([0u64; MAX_SAMPLES], 0);
    let (mut decision, mut ns) = ([0u64; MAX_SAMPLES], 0);
    let (mut notice, mut nn) = ([0u64; MAX_SAMPLES], 0);
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
        decision[ns] = woke.saturating_sub(before + 5_000);
        ns += 1;
        if rd::destroy(lease).is_ok() {
            destroy[nd] = now() - woke;
            nd += 1;
        }
        let _ = rd::receive(Some(exit), 1_000_000, 0);
    }
    // By deadline: the deadline is set before the spawn, so it leaves room for the spawn under
    // load; a lease whose deadline came first is retried.
    for _ in 0..leases * 2 {
        if nn == leases {
            break;
        }
        let deadline = now() + 150_000;
        let Ok(lease) = rd::create(3, &rd::BudgetSpec { deadline, ..rd::spec(pages, 1, 10) }) else {
            continue;
        };
        if spawn::spawn(&image, lease, exit, lease_spinner as *const () as usize, &startup, &[]).is_err() {
            let _ = rd::destroy(lease);
            continue;
        }
        if let Ok(Received::Exit(n)) = rd::receive(Some(exit), 1_000_000, 0) {
            if n.cause == rd::Cause::Killed {
                notice[nn] = now().saturating_sub(deadline);
                nn += 1;
            }
        }
    }
    Stats::report(&mut destroy[..nd], Stats::DESTROY);
    Stats::report(&mut decision[..ns], Stats::DECISION_WAKE);
    Stats::report(&mut notice[..nn], Stats::DEADLINE);
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

    /// Among `devices` (read with [`rd::log_rx`] before any handle is made): the RTC's MMIO
    /// handle and where it is mapped here, and its interrupt handle. The MMIO is the one-page
    /// device that is not virtio and whose first word (its time's low half, in ns) moves by half
    /// a million to a hundred million over a millisecond's sleep: nothing else of the others is
    /// read, since some fault on reads they do not expect. The interrupt is the one a `receive`
    /// on which returns when the alarm fires.
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
        });
        Some((mmio, base, irq?))
    }
}

/// The launcher's UART, once mapped, for [`panicked`].
static UART: AtomicUsize = AtomicUsize::new(0);

/// A case's panic: say so on the UART (the launcher's; a child has none), then park.
pub fn panicked(name: &str, info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(SeqCst);
    if uart != 0 {
        // SAFETY: the launcher mapped this UART and is the bundle's only program.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        let _ = writeln!(out, "[{}] FAIL: panic: {}", name, info);
    }
    crate::park()
}

/// The launcher.
pub struct Bench {
    pub out: MmioSerialPort,
    image: Image,
    exit: u32,
    rep: u32,
    go: [u32; 64],
    started: usize,
    /// `rdtime` ticks per microsecond.
    pub tpu: u64,
    /// Counting-loop iterations per millisecond of CPU, alone.
    pub rate: u64,
    name: &'static str,
    failed: bool,
}

impl Bench {
    pub fn new(name: &'static str) -> Bench {
        let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
        UART.store(uart, SeqCst);
        // SAFETY: this program is the bundle's only one, and owns the UART it just mapped.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        out.init();
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
        let mut b = Bench { out, image, exit, rep, go: [0; 64], started: 0, tpu, rate, name, failed: false };
        let _ = writeln!(b.out, "[{}] calibrated: {} ticks/us, {} iterations/ms", name, tpu, rate);
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
        let mut startup = [0u8; 1 + 8 * 8];
        startup[0] = role as u8;
        for (k, p) in params.iter().enumerate().take(7) {
            startup[1 + k * 8..9 + k * 8].copy_from_slice(&p.to_le_bytes());
        }
        startup[1 + 7 * 8..].copy_from_slice(&self.tpu.to_le_bytes());
        let mut handles = [rep_client, go, 0, 0, 0, 0];
        handles[2..2 + extra.len()].copy_from_slice(extra);
        spawn::spawn(
            &self.image,
            budget,
            self.exit,
            child as *const () as usize,
            &startup,
            &handles[..2 + extra.len()],
        )
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

    /// A count as a share of the window, in thousandths.
    pub fn share(&self, count: u64, window_us: u64) -> u64 {
        count * 1000 / (self.rate * window_us / 1000).max(1)
    }

    pub fn check(&mut self, ok: bool, what: core::fmt::Arguments) {
        self.failed |= !ok;
        let _ = writeln!(self.out, "[{}] {}: {}", self.name, if ok { "ok" } else { "FAIL" }, what);
    }

    pub fn note(&mut self, what: core::fmt::Arguments) {
        let _ = writeln!(self.out, "[{}] {}", self.name, what);
    }

    /// Report, power off.
    pub fn finish(mut self, upper: &str) -> ! {
        if !self.failed {
            let _ = writeln!(self.out, "{} TEST PASSED", upper);
        }
        let _ = rd::system_reset(rd::RESET, rd::ResetKind::PowerOff);
        crate::park()
    }

    pub fn exit_endpoint(&self) -> u32 { self.exit }

    pub fn image(&self) -> &Image { &self.image }
}

/// `Error` re-exported for the cases.
pub type E = Error;
