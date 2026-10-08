//! One process on several harts loses no page to a stale translation (kernel/memory.md, "Residual
//! risks"; `bench:smp-shootdown`), at 2 harts. In each variant a writer thread stores into a page
//! without end on one hart while a sibling, on the other, makes the kernel take the page away:
//!
//! - **Unmap.** U, in process P, unmaps the page W writes. A checker in budget C, told to go just before,
//!   then maps pages (frames are given out lowest first, so the one P let go is among them) and watches them
//!   stay zero; W's next store faults, and P's notice says so (code 15).
//! - **A lend returned.** Server S's receiver R takes the judge's call, which lends a page holding a pattern,
//!   and its writer W stores into the borrowed page; once W is writing, R replies. The judge, back from its
//!   call, watches its page not change; W's next store faults S.
//! - **A lend within one process.** Q's thread A calls an endpoint Q itself receives on, lending a page; B
//!   takes the call and replies once Q's writer W, on the other hart, writes the borrowed page. A, back from
//!   its call, watches its page and tells the judge if it changes; W's next store faults Q.
//!
//! The kernel shoots the process down on the other hart before the call returns, so the writer's
//! hart has no entry left to store through. On QEMU a stale entry lives only until the writer's
//! hart next writes `satp`, at most a slice, so the program's own checks may hold without the
//! shootdown; the checked kernel's audit (a process that lost an entry while another hart ran it,
//! not shot down there) is what fails the recorded negative (`smp-no-shootdown`).
#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, ExitNotice, MessageKind, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// The checker's pages: every frame P let go, and many more.
const CHECKED: usize = 64;
/// How long a page is watched after it comes back, in µs.
const WATCH_US: u64 = 10_000;
/// A store fault (RISC-V `scause` 15): the writer's next store, through no entry.
const STORE_FAULT: u32 = 15;
/// What the judge lends: a pattern the writer's counter never makes.
const PATTERN: u64 = 0x0123_4567_89ab_cdef;
/// Message tags.
const READY: usize = 1;
const GO: usize = 2;
const CHANGED: usize = 3;

/// Store a counter into `at` without end.
fn write_forever(at: usize) -> ! {
    let mut n: u64 = 0;
    loop {
        n = n.wrapping_add(1);
        rd::poke(at, 0x5a5a_0000_0000_0000 | n);
    }
}

/// Spin until `at` holds something other than `was`, for at most `WAIT`: a thread on the other
/// hart wrote it. `false` if it never did.
fn changes(at: usize, was: u64) -> bool {
    let start = rd::time_now().unwrap_or(0);
    while rd::peek(at) == was {
        if rd::time_now().unwrap_or(u64::MAX) > start + WAIT {
            return false;
        }
    }
    true
}

/// Watch `at` for `WATCH_US`: whether it changed from what it held at the start.
fn watch(at: usize) -> bool {
    let (was, start) = (rd::peek(at), rd::time_now().unwrap_or(0));
    while rd::time_now().unwrap_or(u64::MAX) < start + WATCH_US {
        if rd::peek(at) != was {
            return true;
        }
    }
    false
}

/// Block for good: the writer's fault ends the process.
fn wait_forever() -> ! {
    loop {
        let _ = rd::receive(None, WAIT, 0);
    }
}

// --- Unmap: P (the judge's inbox in slot 1, the checker's go endpoint in slot 2), and C ----------

fn unmap_writer(page: usize) { write_forever(page) }

extern "C" fn unmapper(_: usize) -> ! {
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
    rd::poke(page, 0);
    if rd::thread(unmap_writer, page).is_err() || !changes(page, 0) {
        rd::process_exit(2);
    }
    // The checker runs once a hart is free: after the unmap.
    let _ = rd::send(2, &rd::body([GO, 0, 0, 0]), None, WAIT);
    if rd::unmap(page, rd::PAGE_SIZE).is_err() {
        rd::process_exit(3);
    }
    wait_forever()
}

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
    while rd::time_now().unwrap_or(u64::MAX) < start + WATCH_US {
        dirty += (0..words).filter(|i| rd::peek(pages + i * 8) != 0).count();
    }
    let _ = rd::send_waiting(1, &rd::body([GO, dirty, 0, 0]), None, WAIT);
    test_programs::park()
}

// --- A lend returned: S (its endpoint in slot 1) --------------------------------------------------

/// The writer: waits for the borrowed page's address in `shared`, then writes the page.
fn lend_writer(shared: usize) {
    while rd::peek(shared) == 0 {}
    write_forever(rd::peek(shared) as usize)
}

/// Receive one call on `endpoint` lending a page, have the writer on `shared` write it, and
/// reply once it is writing. `false` if any of it went wrong.
fn serve_one(endpoint: u32, shared: usize) -> bool {
    let Ok(Received::Message(m)) = rd::receive(Some(endpoint), WAIT, 0) else { return false };
    let MessageKind::Call { lend: Some(lend) } = m.kind else { return false };
    let borrowed = lend.addr;
    let was = rd::peek(borrowed);
    rd::poke(shared, borrowed as u64);
    changes(borrowed, was) && rd::reply(m.msg_id.get(), &rd::body([0; 4])).is_ok()
}

extern "C" fn server(_: usize) -> ! {
    let shared = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
    rd::poke(shared, 0);
    if rd::thread(lend_writer, shared).is_err() || !serve_one(1, shared) {
        rd::process_exit(2);
    }
    wait_forever()
}

// --- A lend within one process: Q (the judge's inbox in slot 1) ------------------------------------

/// B: takes A's call on Q's own endpoint and replies once W writes the page.
fn self_receiver(arg: usize) {
    let (endpoint, shared) = (arg as u32 & 0xffff, arg >> 16);
    if !serve_one(endpoint, shared) {
        rd::process_exit(2);
    }
    wait_forever()
}

extern "C" fn self_lender(_: usize) -> ! {
    let endpoint = rd::endpoint_create().unwrap_or_else(|_| rd::process_exit(1));
    let mine = rd::mint_from_handle(endpoint, 1, None).unwrap_or_else(|_| rd::process_exit(1));
    let shared = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
    rd::poke(shared, 0);
    let page = rd::page();
    rd::poke(page, PATTERN);
    // `shared` is page-aligned, so its address has room below it for the endpoint's index.
    if rd::thread(lend_writer, shared).is_err()
        || rd::thread(self_receiver, shared << 16 | endpoint as usize).is_err()
    {
        rd::process_exit(1);
    }
    // A: the call returns once B replied, the page given back.
    if rd::call(mine, &rd::body([0; 4]), rd::pages(page, 1), WAIT).is_err() {
        rd::process_exit(3);
    }
    if watch(page) {
        let _ = rd::send(1, &rd::body([CHANGED, 0, 0, 0]), None, WAIT);
    }
    wait_forever()
}

// --- The judge --------------------------------------------------------------------------------

fn notice(exit: u32) -> Option<ExitNotice> {
    match rd::receive(Some(exit), WAIT, 0) {
        Ok(Received::Exit(n)) => Some(n),
        _ => None,
    }
}

/// Faulted by the writer's store, code 15.
fn store_fault(n: &Option<ExitNotice>) -> bool {
    n.as_ref().is_some_and(|n| n.cause == Cause::Faulted && n.code == STORE_FAULT)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("smp-shootdown");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");

    // Unmap. The checker waits in C first, so nothing is spawned between P's unmap and its maps.
    let go = rd::endpoint_create().expect("the checker's go endpoint");
    let c = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("C");
    spawn::spawn(&image, c, exit, checker as *const () as usize, &[], &[inbox, go]).expect("the checker");
    let ready =
        matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY);
    let p = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("P's budget");
    spawn::spawn(&image, p, exit, unmapper as *const () as usize, &[], &[inbox, go]).expect("P");
    let ended = notice(exit);
    let dirty = match rd::receive(Some(inbox), WAIT, 0) {
        Ok(Received::Message(m)) if m.body.words[0] == GO => Some(m.body.words[1]),
        _ => None,
    };
    b.check(
        ready && dirty == Some(0),
        format_args!(
            "unmap: the checker's {} pages, P's freed frame among them, stayed zero ({:?})",
            CHECKED, dirty
        ),
    );
    b.check(store_fault(&ended), format_args!("unmap: P's writer faulted at its next store ({:?})", ended));

    // A lend returned: the judge is the client.
    let endpoint = rd::endpoint_create().expect("S's endpoint");
    let s = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("S's budget");
    spawn::spawn(&image, s, exit, server as *const () as usize, &[], &[endpoint]).expect("S");
    let page = rd::page();
    rd::poke(page, PATTERN);
    let called = rd::call_waiting(endpoint, &rd::body([0; 4]), rd::pages(page, 1), WAIT).is_ok();
    let changed = watch(page);
    let ended = notice(exit);
    b.check(called && !changed, format_args!("a lend returned: the page did not change after the reply"));
    b.check(
        store_fault(&ended),
        format_args!("a lend returned: S's writer faulted at its next store ({:?})", ended),
    );

    // A lend within one process: Q's notice and A's word both come to the inbox, in order.
    let q = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("Q's budget");
    spawn::spawn(&image, q, inbox, self_lender as *const () as usize, &[], &[inbox]).expect("Q");
    let (mut changed, mut ended) = (false, None);
    while ended.is_none() {
        match rd::receive(Some(inbox), WAIT, 0) {
            Ok(Received::Message(m)) if m.body.words[0] == CHANGED => changed = true,
            Ok(Received::Exit(n)) => ended = Some(n),
            _ => break,
        }
    }
    b.check(!changed, format_args!("a lend within one process: A saw no change after the reply"));
    b.check(
        store_fault(&ended),
        format_args!("a lend within one process: Q's writer faulted at its next store ({:?})", ended),
    );
    b.finish("SMP-SHOOTDOWN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("smp-shootdown", info) }
