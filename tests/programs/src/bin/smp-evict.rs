//! Several harts add no reach (kernel/memory.md, "Residual risks"; `bench:smp-evict`), at 2 harts.
//!
//! **The shootdown at a destruction.** A writer in budget B stores into its own pages without
//! end, on one hart. The judge, on the other, destroys B; a checker already waiting in budget C
//! then maps pages, which take B's freed frames (frames are given out lowest first), and reads
//! them for a while: every word must stay zero. A kernel that frees B's frames while the writer's
//! hart still holds B's translations lets its stores land in C's pages, which the checker sees,
//! or the checked build's free-list audit trips.
//!
//! **The stale mask.** A process P maps a page, touches it, waits, unmaps it and waits again, over
//! and over, so that its budget runs now on one hart and now on the other. Last, it unmaps its
//! page and waits while the judge, Q, maps a page of its own (the frame P let go is the lowest
//! free) and writes it; then P, running again on whichever hart, reads its old address. The read
//! must fault, which the kernel reports in P's exit notice, and Q's page keeps what Q wrote. A
//! hart that installs P after another unmapped one of its pages owes its ASID a flush; the checked
//! build stops if one installs it without (on QEMU, which empties a hart's TLB at every `satp`
//! write, the audit is what sees it).
//!
//! On QEMU these checks hold even without the shootdown or the mask (see `tests/smp-evict.toml`): the
//! verdict of the recorded negatives is the checked kernel's audit.
#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, ExitNotice, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// The writer's pages.
const WRITTEN: usize = 16;
/// The checker's pages: every frame B held, and many more.
const CHECKED: usize = 512;
/// How long the checker watches its pages, in ms.
const WATCH_MS: u64 = 50;
/// The stale-mask process's rounds.
const ROUNDS: usize = 200;
/// Message tags.
const READY: usize = 1;
const GO: usize = 2;

// --- The writer: B, with the judge's inbox in slot 1 -------------------------------------------

extern "C" fn writer(_: usize) -> ! {
    let pages = rd::map_anon(WRITTEN * rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
    let words = WRITTEN * rd::PAGE_SIZE / 8;
    rd::poke(pages, 1);
    if rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    let mut n: u64 = 0;
    loop {
        n = n.wrapping_add(1);
        for i in 0..words {
            rd::poke(pages + i * 8, 0x5a5a_0000_0000_0000 | n);
        }
    }
}

// --- The checker: C, with the judge's inbox in slot 1 and its own go endpoint in slot 2 --------

extern "C" fn checker(_: usize) -> ! {
    if rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    if !matches!(rd::receive(Some(2), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == GO) {
        rd::process_exit(1);
    }
    let pages = rd::map_anon(CHECKED * rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(2));
    let words = CHECKED * rd::PAGE_SIZE / 8;
    let start = rd::time_now().unwrap_or(0);
    let mut dirty = 0usize;
    while rd::time_now().unwrap_or(u64::MAX) < start + WATCH_MS * 1000 {
        // A read of a fresh page maps a zeroed frame.
        dirty += (0..words).filter(|i| rd::peek(pages + i * 8) != 0).count();
    }
    let _ = rd::send_waiting(1, &rd::body([GO, dirty, 0, 0]), None, WAIT);
    test_programs::park()
}

// --- P: its own budget, the judge's inbox in slot 1 and its go endpoint in slot 2 --------------

extern "C" fn mover(_: usize) -> ! {
    let mut addr = 0;
    for _ in 0..ROUNDS {
        addr = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
        rd::poke(addr, 0x5a5a);
        test_programs::wait_ms(1);
        if rd::unmap(addr, rd::PAGE_SIZE).is_err() {
            rd::process_exit(1);
        }
        test_programs::wait_ms(1);
    }
    // Q takes the frame meanwhile.
    if rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    if !matches!(rd::receive(Some(2), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == GO) {
        rd::process_exit(1);
    }
    // The last page's address, unmapped: this read faults, and the notice says so.
    let _ = rd::peek(addr);
    rd::process_exit(0)
}

// --- The judge ------------------------------------------------------------------------------

fn ready(inbox: u32) -> bool {
    matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY)
}

fn notice(exit: u32) -> Option<ExitNotice> {
    match rd::receive(Some(exit), WAIT, 0) {
        Ok(Received::Exit(n)) => Some(n),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("smp-evict");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");

    // The checker waits in C first, so nothing is spawned between B's end and its maps.
    let go = rd::endpoint_create().expect("the checker's go endpoint");
    let c = rd::create(rd::SYSTEM, &rd::spec(1600, 1, 10)).expect("C");
    spawn::spawn(&image, c, exit, checker as *const () as usize, &[], &[inbox, go]).expect("the checker");
    let checker_ready = ready(inbox);
    let budget = rd::create(rd::SYSTEM, &rd::spec(800, 1, 10)).expect("B");
    spawn::spawn(&image, budget, exit, writer as *const () as usize, &[], &[inbox]).expect("the writer");
    let writer_ready = ready(inbox);
    b.check(checker_ready && writer_ready, format_args!("the writer is writing and the checker waiting"));
    // The writer runs on the other hart meanwhile.
    test_programs::wait_ms(5);
    rd::destroy(budget).expect("destroying B");
    let _ = rd::send(go, &rd::body([GO, 0, 0, 0]), None, WAIT);
    let dirty = match rd::receive(Some(inbox), WAIT, 0) {
        Ok(Received::Message(m)) if m.body.words[0] == GO => Some(m.body.words[1]),
        _ => None,
    };
    b.check(
        dirty == Some(0),
        format_args!(
            "the checker's {} pages, B's freed frames among them, stayed zero ({:?} words written)",
            CHECKED, dirty
        ),
    );
    let killed = notice(exit);
    b.check(
        killed.as_ref().is_some_and(|n| n.cause == Cause::Killed),
        format_args!("the writer was killed with B"),
    );

    let m = rd::create(rd::SYSTEM, &rd::spec(800, 1, 10)).expect("P's budget");
    let p_go = rd::endpoint_create().expect("P's go endpoint");
    spawn::spawn(&image, m, exit, mover as *const () as usize, &[], &[inbox, p_go]).expect("P");
    let unmapped = ready(inbox);
    let mine = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("Q's page");
    rd::poke(mine, 0x0051_0051);
    let _ = rd::send(p_go, &rd::body([GO, 0, 0, 0]), None, WAIT);
    let moved = notice(exit);
    b.check(
        unmapped && moved.as_ref().is_some_and(|n| n.cause == Cause::Faulted),
        format_args!("P's read of its page unmapped on another hart faulted"),
    );
    b.check(rd::peek(mine) == 0x0051_0051, format_args!("Q's page, P's old frame or not, kept what Q wrote"));
    b.finish("SMP-EVICT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("smp-evict", info) }
