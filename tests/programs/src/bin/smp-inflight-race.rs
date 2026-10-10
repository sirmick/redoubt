//! A frame in flight is no one's (kernel/memory.md, R81; `bench:smp-inflight-race`), at 2 harts.
//! A freed frame is zeroed, by the hart that freed it when idle or by an allocation that finds the
//! bitmap empty, and only then given back to the bitmap; meanwhile a second hart races to take, map
//! and free frames:
//!
//! - **The fillers.** First a filler in a child of each boot budget holds all but a few of its free pages, so
//!   RAM is nearly full: the allocations below empty the bitmap, and take the frames in flight back on
//!   demand, any hart's.
//! - **The attacker.** Each round, P maps `RUN` pages, and its writer W stores a pattern into all of them
//!   without end on one hart while its unmapper U, on the other, unmaps them in one call: more than a hart
//!   stages before a shootdown, so the call shoots P down early as well as at its end. W's next store faults,
//!   which ends P, and P's other frames are retired at its end.
//! - **The witness.** All the while V, in a budget of its own, maps `CHECKED` pages, finds every word of them
//!   zero, writes a marker into every word, and finds the markers unchanged a while later, then unmaps them.
//!   With RAM nearly full, it gets the frames P let go, and the ones it let go itself, as soon as each is
//!   zeroed.
//!
//! The verdict is the system's (rule F): V, the victim, inspecting its own pages; the checked
//! kernel's own checks (a frame taken from the bitmap not zero, a frame given back not zero,
//! I1's count of the frames in flight); and the trace (`smp_inflight`): every frame pending only
//! after the shootdown of the process its entry was cleared in, given back only once pending (its
//! hart zeroed it), and never taken while in flight.
#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, ExitNotice, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// The attacker's run: more than a hart stages before it shoots a process down (64).
const RUN: usize = 160;
/// The witness's pages each turn.
const CHECKED: usize = 32;
/// How long the witness watches its markers each turn, in µs.
const WATCH_US: u64 = 2_000;
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

/// Each word of `pages` that is not `want(address)`.
fn wrong(pages: usize, want: impl Fn(usize) -> u64) -> usize {
    (0..CHECKED * rd::PAGE_SIZE / 8).map(|i| pages + i * 8).filter(|at| rd::peek(*at) != want(*at)).count()
}

extern "C" fn witness(_: usize) -> ! {
    if rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    let (mut dirty, mut changed, mut turns) = (0usize, 0usize, 0usize);
    loop {
        if matches!(rd::receive(Some(2), 1, 0), Ok(Received::Message(m)) if m.body.words[0] == STOP) {
            break;
        }
        let pages = rd::map_anon(CHECKED * rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(2));
        dirty += wrong(pages, |_| 0);
        let mark = |at: usize| MARK | at as u64;
        for i in 0..CHECKED * rd::PAGE_SIZE / 8 {
            rd::poke(pages + i * 8, mark(pages + i * 8));
        }
        let start = rd::time_now().unwrap_or(0);
        while rd::time_now().unwrap_or(u64::MAX) < start + WATCH_US {}
        changed += wrong(pages, mark);
        if rd::unmap(pages, CHECKED * rd::PAGE_SIZE).is_err() {
            rd::process_exit(3);
        }
        turns += 1;
    }
    let _ = rd::send_waiting(1, &rd::body([DONE, dirty, changed, turns]), None, WAIT);
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
    let mut b = Bench::new("smp-inflight-race");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");
    let stop = rd::endpoint_create().expect("the witness's stop endpoint");

    let v = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).expect("V's budget");
    spawn::spawn(&image, v, exit, witness as *const () as usize, &[], &[inbox, stop]).expect("V");
    let ready =
        matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY);
    b.check(ready, format_args!("the witness is ready"));

    // RAM nearly full. The judge runs in `root`, whose processes are all carved out, so it holds
    // `root`'s free pages itself; a filler holds each child's, `system` keeping room for each round's
    // P besides.
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
        report.is_some() && dirty == 0 && changed == 0 && turns > 0,
        format_args!(
            "the witness's pages, {turns} turns of {CHECKED}: {dirty} words not zero when mapped, {changed} markers changed"
        ),
    );
    b.finish("SMP-INFLIGHT-RACE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("smp-inflight-race", info) }
