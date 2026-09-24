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
    /// Churn threads: a thread counts for p0 ticks then exits, over and over; report the total.
    ThreadChurn = 5,
    /// Churn processes in the budget in slot 3: a child counts for p0 ticks then exits (p1 = 0)
    /// or faults (p1 = 1); report the total.
    ProcessChurn = 6,
    /// A process-churn child: count p0 ticks, report to slot 1, exit or fault.
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

fn join(lo: usize, hi: usize) -> u64 { (lo as u32 as u64) | ((hi as u32 as u64) << 32) }

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

extern "C" fn churn_worker(done: usize) -> ! {
    let n = spin_until(ticks() + param(0));
    TOTAL.fetch_add(n as usize, SeqCst);
    let _ = rd::send(done as u32, &rd::body([0; 4]), None, rd::FOREVER);
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
        let _ = rd::send(1, &rd::body([n as usize, 0, 0, 0]), None, rd::FOREVER);
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
            let done = rd::endpoint_create().expect("done endpoint");
            let done_client = rd::mint_from_handle(done, 1, None).expect("mint");
            let mut round = 0;
            while ticks() < end {
                // One worker at a time, on two stacks in turn: the one before last has certainly
                // exited (the last may still be between its report and its exit).
                thread_on(30 + round % 2, churn_worker, done_client as usize);
                round += 1;
                let _ = rd::receive(Some(done), rd::FOREVER, 0);
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
/// then exiting or faulting.
fn process_churn(end: u64) -> u64 {
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit");
    let rep = rd::endpoint_create().expect("rep");
    let rep_client = rd::mint_from_handle(rep, 1, None).expect("mint");
    let mut total = 0u64;
    let mut startup = [0u8; 1 + 8 * 8];
    startup[0] = Role::ChurnChild as u8;
    startup[1..9].copy_from_slice(&param(0).to_le_bytes());
    startup[9..17].copy_from_slice(&param(1).to_le_bytes());
    while ticks() < end {
        if spawn::spawn(&image, 3, exit, child as *const () as usize, &startup, &[rep_client]).is_err() {
            break;
        }
        if let Ok(Received::Message(m)) = rd::receive(Some(rep), rd::FOREVER, 0) {
            total += m.body.words[0] as u64;
        }
        let _ = rd::receive(Some(exit), rd::FOREVER, 0);
    }
    total
}

/// Budget churn under this process's own budget (slot 3); see [`Role::BudgetChurn`].
fn budget_churn(end: u64, tpu: u64) -> u64 {
    let variant = param(0);
    let weight = param(1).max(1) as u32;
    if variant == 4 {
        // The shell pattern, as the kernel sees it: this budget keeps counting on a thread of its
        // own, and gives budget after budget most of its weight, then takes it back, with no run
        // in between. Creating and destroying moves nothing (OWNER DECISION 6): the thread keeps
        // this budget's share.
        thread(shell_spinner, 0);
        while ticks() < end {
            if let Ok(c) = rd::create(3, &rd::spec(1, 0, weight)) {
                let _ = rd::destroy(c);
            }
            let _ = rd::receive(None, 1_000, 0);
        }
        await_done(1);
        return TOTAL.load(SeqCst) as u64;
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
