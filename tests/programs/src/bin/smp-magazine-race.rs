//! A frame in a hart's magazine is no one's (kernel/memory.md, R84; `bench:smp-magazine-race`), at
//! 2 harts. Each hart draws its common allocations from a magazine of zero frames and refills it at a
//! system call's return, zeroing outside the lock the frames its own freeing left pending; meanwhile
//! the other hart races to take, map and free frames:
//!
//! - **The fillers.** As in `smp-inflight-race`, RAM is held but for a few pages, so the allocations below
//!   empty the bitmap and take frames from another hart's magazine, and back from flight.
//! - **The attacker.** Each round, P maps `RUN` pages, its writer W stores a pattern into them without end on
//!   one hart, and its unmapper U, on the other, unmaps them under it: the frames go pending holding the
//!   pattern, and come back through a magazine.
//! - **The witness.** Once the fillers hold RAM, V, in a budget of its own, runs two threads, one a hart:
//!   each maps `CHECKED` pages one call at a time, so each call draws from its hart's magazine and refills it
//!   at its return, finds every word zero, writes a marker into every word, finds the markers unchanged a
//!   while later, and unmaps the pages one call at a time, so they go pending on its hart, and its next
//!   refills take them.
//!
//! The verdict is the system's (rule F): V, the victim, inspecting its own pages; the checked kernel's
//! own checks (a frame drawn from a magazine not zero, a frame entering one not zero, I1's count of
//! the frames in magazines and in flight); and the trace (`smp_magazine`): every frame into a
//! magazine from its own hart's pending list or the bitmap, drawn only by its hart, taken by another
//! only for an empty bitmap, and never taken from the bitmap, retired or given back while held.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, ExitNotice, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// The attacker's run: more than a hart stages before it shoots a process down (64).
const RUN: usize = 160;
/// The pages each witness thread maps each turn, one call each.
const CHECKED: usize = 24;
/// How long the witness watches its markers each turn, in µs.
const WATCH_US: u64 = 1_000;
/// The attacker's rounds.
const ROUNDS: usize = 12;
/// A store fault (RISC-V `scause` 15): the writer's next store, through no entry.
const STORE_FAULT: u32 = 15;
/// What the writer stores: a pattern the witness's marker never makes.
const PATTERN: u64 = 0xa77a_c000_0000_0000;
/// The witness's marker, with each word's address in it.
const MARK: u64 = 0x5afe_0000_0000_0000;
/// The free pages each filler leaves its boot budget, besides what the rounds need.
const FILL_SLACK: u64 = 64;
/// Message tags.
const READY: usize = 1;
const STOP: usize = 2;
const DONE: usize = 3;
const START: usize = 4;

/// Block for good: the writer's fault ends the process.
fn wait_forever() -> ! {
    loop {
        let _ = rd::receive(None, WAIT, 0);
    }
}

// --- The attacker: P ------------------------------------------------------------------------------

/// W: store into every page of the run, round after round, without end.
fn writer(run: usize) {
    let mut n: u64 = 0;
    loop {
        n = n.wrapping_add(1);
        for page in 0..RUN {
            rd::poke(run + page * rd::PAGE_SIZE, PATTERN | n);
        }
    }
}

extern "C" fn attacker(_: usize) -> ! {
    let run = rd::map_anon(RUN * rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
    let last = run + (RUN - 1) * rd::PAGE_SIZE;
    if rd::thread(writer, run).is_err() {
        rd::process_exit(2);
    }
    // U: once W has written the whole run, take it away under W.
    let start = rd::time_now().unwrap_or(0);
    while rd::peek(last) == 0 {
        if rd::time_now().unwrap_or(u64::MAX) > start + WAIT {
            rd::process_exit(3);
        }
    }
    if rd::unmap(run, RUN * rd::PAGE_SIZE).is_err() {
        rd::process_exit(4);
    }
    wait_forever()
}

// --- The fillers (the judge's inbox in slot 1, the filler's own budget in slot 2) --------------------

/// Map all but `keep` of `budget`'s free pages, the budget this process runs in, in big runs each
/// leaving room for its page tables. Whether it got that far.
fn hold(budget: u32, keep: u64) -> bool {
    loop {
        let free = rd::free(budget);
        if free <= keep + 16 {
            return true;
        }
        let n = (free - keep - free / 256 - 8) as usize;
        if rd::map_anon(n * rd::PAGE_SIZE, rd::rw()).is_err() {
            return false;
        }
    }
}

/// Hold all of the budget it runs in, then say so.
extern "C" fn filler(_: usize) -> ! {
    if hold(2, 0) {
        let _ = rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT);
    }
    test_programs::park()
}

// --- The witness: V (the judge's inbox in slot 1, its stop endpoint in slot 2) ---------------------

/// What both witness threads found: words not zero when mapped, markers changed, turns, and whether
/// to stop.
static DIRTY: AtomicUsize = AtomicUsize::new(0);
static CHANGED: AtomicUsize = AtomicUsize::new(0);
static TURNS: AtomicUsize = AtomicUsize::new(0);
static STOPPING: AtomicUsize = AtomicUsize::new(0);
/// A witness thread that could not map or unmap: its step, 0 for none.
static BROKE: AtomicUsize = AtomicUsize::new(0);

/// Each word of the page at `page` that is not `want(address)`.
fn wrong(page: usize, want: impl Fn(usize) -> u64) -> usize {
    (0..rd::PAGE_SIZE / 8).map(|i| page + i * 8).filter(|at| rd::peek(*at) != want(*at)).count()
}

/// One turn of `CHECKED` pages, each mapped, checked, marked, watched and unmapped in a call of its
/// own; the step that failed, if one did (1 a map, 2 an unmap).
fn turn() -> Result<(), usize> {
    let mark = |at: usize| MARK | at as u64;
    let mut pages = [0usize; CHECKED];
    for page in pages.iter_mut() {
        let at = rd::map_anon(rd::PAGE_SIZE, rd::rw()).map_err(|_| 1usize)?;
        *page = at;
        DIRTY.fetch_add(wrong(at, |_| 0), Ordering::Relaxed);
        for i in 0..rd::PAGE_SIZE / 8 {
            rd::poke(at + i * 8, mark(at + i * 8));
        }
    }
    let start = rd::time_now().unwrap_or(0);
    while rd::time_now().unwrap_or(u64::MAX) < start + WATCH_US {}
    for page in pages {
        CHANGED.fetch_add(wrong(page, mark), Ordering::Relaxed);
        rd::unmap(page, rd::PAGE_SIZE).map_err(|_| 2usize)?;
    }
    TURNS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The second witness thread: turns until told to stop, or one fails.
fn witness_turns(_: usize) {
    while STOPPING.load(Ordering::Acquire) == 0 {
        if let Err(step) = turn() {
            BROKE.store(step, Ordering::Release);
            return;
        }
    }
}

extern "C" fn witness(_: usize) -> ! {
    if rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    // Idle while the fillers take RAM, which they could not do in time beside two threads mapping.
    while !matches!(rd::receive(Some(2), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == START) {}
    if rd::thread(witness_turns, 0).is_err() {
        rd::process_exit(2);
    }
    // The first thread is a witness too: a turn between looks at the stop endpoint.
    while !matches!(rd::receive(Some(2), 1, 0), Ok(Received::Message(m)) if m.body.words[0] == STOP) {
        if let Err(step) = turn() {
            BROKE.store(step, Ordering::Release);
            break;
        }
    }
    STOPPING.store(1, Ordering::Release);
    let (dirty, changed) = (DIRTY.load(Ordering::Relaxed), CHANGED.load(Ordering::Relaxed));
    let (turns, broke) = (TURNS.load(Ordering::Relaxed), BROKE.load(Ordering::Acquire));
    let _ = rd::send_waiting(1, &rd::body([DONE, dirty | broke << 24, changed, turns]), None, WAIT);
    test_programs::park()
}

// --- The judge ------------------------------------------------------------------------------------

fn notice(exit: u32) -> Option<ExitNotice> {
    match rd::receive(Some(exit), WAIT, 0) {
        Ok(Received::Exit(n)) => Some(n),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("smp-magazine-race");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");
    let stop = rd::endpoint_create().expect("the witness's stop endpoint");

    let v = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).expect("V's budget");
    spawn::spawn(&image, v, exit, witness as *const () as usize, &[], &[inbox, stop]).expect("V");
    let ready =
        matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY);
    b.check(ready, format_args!("the witness is ready"));

    // RAM nearly full, as in `smp-inflight-race`.
    let held = hold(rd::ROOT, FILL_SLACK);
    b.check(held, format_args!("the judge holds root's free pages but {FILL_SLACK}"));
    for (parent, keep) in [(rd::USERS, FILL_SLACK), (rd::SYSTEM, RUN as u64 + 80 + FILL_SLACK)] {
        let pages = rd::free(parent).saturating_sub(keep);
        let filled = rd::create(parent, &rd::spec(pages, 1, 1)).is_ok_and(|f| {
            spawn::spawn(&image, f, exit, filler as *const () as usize, &[], &[inbox, f]).is_ok()
                && matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY)
        });
        b.check(filled, format_args!("a filler holds budget {parent}'s free pages but {keep}"));
    }
    let started = rd::send_waiting(stop, &rd::body([START, 0, 0, 0]), None, WAIT).is_ok();
    b.check(started, format_args!("the witness starts"));

    let mut faulted = 0;
    for _ in 0..ROUNDS {
        let p = rd::create(rd::SYSTEM, &rd::spec(RUN as u64 + 80, 1, 10)).expect("P's budget");
        spawn::spawn(&image, p, exit, attacker as *const () as usize, &[], &[]).expect("P");
        let ended = notice(exit);
        faulted += usize::from(ended.is_some_and(|n| n.cause == Cause::Faulted && n.code == STORE_FAULT));
        let _ = rd::destroy(p);
    }
    b.check(
        faulted == ROUNDS,
        format_args!(
            "every round, P's writer faulted at its first store after the unmap ({faulted} of {ROUNDS})"
        ),
    );

    let _ = rd::send_waiting(stop, &rd::body([STOP, 0, 0, 0]), None, WAIT);
    let report = match rd::receive(Some(inbox), WAIT, 0) {
        Ok(Received::Message(m)) if m.body.words[0] == DONE => Some(m.body.words),
        _ => None,
    };
    let [_, dirty, changed, turns] = report.unwrap_or([0, usize::MAX, usize::MAX, 0]);
    b.check(
        report.is_some() && dirty == 0 && changed == 0 && turns > 1,
        format_args!(
            "the witness's pages, {turns} turns of {CHECKED} on two threads: {} words not zero when mapped, {changed} markers changed, broken at step {}",
            dirty & 0xff_ffff,
            dirty >> 24
        ),
    );
    b.finish("SMP-MAGAZINE-RACE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("smp-magazine-race", info) }
