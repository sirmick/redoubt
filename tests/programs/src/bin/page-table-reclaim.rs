//! A page table that maps nothing is freed, and uncharged, by the call that emptied it (R6;
//! kernel/memory.md, "Page tables").
//!
//! The one program the tester starts, in a budget of its own (slot 3): every verdict below is
//! that budget's page usage as the kernel reports it (`rd::usage`, R6), never this program's own
//! claim (docs/testbench.md, "Rule F"); nothing else runs in that budget. It runs behind the
//! tester, not in `init`'s place, because a round on Sv32 is more pages than `root` keeps. It
//! checks four ways a table empties:
//! - `map_anon` then `unmap`, 50 times: each round charges the pages and a table, and `unmap` gives all of
//!   them back.
//! - A `map_anon` of every free page, refused at the budget's edge once its tables no longer fit: `map_run`'s
//!   rollback gives back the tables it made along with the pages.
//! - A transfer out of a page in a span of its own: the sender's table goes with the page, and the receiver's
//!   goes with its `unmap`.
//! - A lend and its reply: the table the lend was mapped with in the server goes at the reply.
//! - A lend its caller abandons (the call times out): the caller's table goes when the lend leaves it, and
//!   the server's when it replies and the lend is freed.
//! - A `process_map` into a child of another budget: the source's table goes with its page.
//!
//! Sender, receiver and server are threads of this one process, so every table is charged to
//! its own budget, and the budget's usage sees all of them.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use test_programs::rd::{self, Error, FOREVER, MessageKind, PAGE_SIZE, Received};
use test_programs::{Logger, checker, log};

static ENDPOINT: AtomicU32 = AtomicU32::new(0);

/// Pages per `map_anon` round: a leaf table's whole span (2 MiB on Sv39, 4 MiB on Sv32), so a
/// round always reaches a span no other mapping holds a table in (the logger's page is the
/// area's first).
const PAGES: usize = if cfg!(target_pointer_width = "64") { 512 } else { 1024 };
const ROUNDS: usize = 50;
/// A page alone in its leaf table's span on both widths (2 MiB on Sv39, 4 MiB on Sv32): below
/// the `map_anon` area, above the message area, and nothing else of this process is there.
const LONE: usize = 0x5000_0000;

fn check(out: &mut Logger, ok: bool, label: &str) {
    log!(out, "[pt-reclaim] {}: {}", if ok { "ok" } else { "FAIL" }, label);
    assert!(ok, "{}", label);
}

fn usage_pages() -> u64 { rd::usage(rd::OWN).unwrap().pages_usage }

/// Call word: hold the call past its caller's timeout before replying.
const LATE: usize = 1;
/// The caller's timeout for a `LATE` call, in microseconds, and how long the server holds it.
const TIMEOUT_US: u64 = 10_000;
const HOLD_MS: u64 = 50;

/// The receiving thread: it unmaps a transfer as soon as it has it, and replies to a call, which
/// tells the main thread that everything sent before it has been dealt with.
fn serve(_: usize) {
    let endpoint = ENDPOINT.load(Ordering::Acquire);
    loop {
        let Ok(Received::Message(m)) = rd::receive(Some(endpoint), FOREVER, 1) else { continue };
        match m.kind {
            MessageKind::Send { transfer: Some(pages) } => {
                rd::unmap(pages.addr, pages.npages.get() * PAGE_SIZE).expect("unmap the transfer");
            }
            MessageKind::Call { .. } => {
                if m.body.words[0] == LATE {
                    test_programs::wait_ms(HOLD_MS);
                }
                rd::reply(m.msg_id.get(), &rd::body([0; 4])).expect("reply");
            }
            MessageKind::Send { transfer: None } => {}
        }
    }
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let mut out = Logger::connect();
    let out = &mut out;

    // Before any thread, so nothing else in the `map_anon` area shares a round's tables.
    let mut rounds = true;
    for _ in 0..ROUNDS {
        let before = usage_pages();
        let at = rd::map_anon(PAGES * PAGE_SIZE, rd::rw()).expect("map_anon");
        let charged = usage_pages() - before;
        rd::unmap(at, PAGES * PAGE_SIZE).expect("unmap");
        rounds &= charged > PAGES as u64 && usage_pages() == before;
    }
    check(out, rounds, "each map_anon charged a page table, and its unmap gave it back");

    // All of its own budget's free pages fit in the 256 MiB area, so the search finds room and
    // the run fails only when a page or a table can no longer be paid for.
    let before = usage_pages();
    let free = rd::free(rd::OWN) as usize;
    assert!(free * PAGE_SIZE < 0x1000_0000, "its free pages must fit the map_anon area");
    let r = rd::map_anon(free * PAGE_SIZE, rd::rw());
    check(
        out,
        r == Err(Error::OutOfMemory) && usage_pages() == before,
        "a map_anon refused at the budget's edge leaves usage where it was",
    );

    let endpoint = rd::endpoint_create().expect("endpoint");
    ENDPOINT.store(endpoint, Ordering::Release);
    rd::thread(serve, 0).expect("the receiving thread");
    let sync = || {
        rd::call(endpoint, &rd::body([0; 4]), None, FOREVER).expect("sync");
    };
    sync();

    let before = usage_pages();
    rd::map_fixed(LONE, PAGE_SIZE, rd::rw()).expect("map_fixed the page to transfer");
    let charged = usage_pages() - before;
    rd::poke(LONE, 7);
    rd::send(endpoint, &rd::body([0; 4]), rd::pages(LONE, 1), FOREVER).expect("send");
    sync();
    check(out, charged > 1 && usage_pages() == before, "a transfer and its unmap leave no page table behind");

    rd::map_fixed(LONE, PAGE_SIZE, rd::rw()).expect("map_fixed the page to lend");
    rd::poke(LONE, 9);
    let before = usage_pages();
    rd::call(endpoint, &rd::body([0; 4]), rd::pages(LONE, 1), FOREVER).expect("the lend");
    check(
        out,
        rd::peek(LONE) == 9 && usage_pages() == before,
        "a lend and its reply leave the server's usage where it was",
    );
    rd::unmap(LONE, PAGE_SIZE).expect("unmap the lent page");

    let before = usage_pages();
    rd::map_fixed(LONE, PAGE_SIZE, rd::rw()).expect("map_fixed the page to lend");
    let late = rd::call(endpoint, &rd::body([LATE, 0, 0, 0]), rd::pages(LONE, 1), TIMEOUT_US);
    sync();
    check(
        out,
        late.err() == Some(Error::Timeout) && usage_pages() == before,
        "an abandoned lend, once replied to, leaves no page table behind",
    );

    let budget = rd::create(rd::OWN, &rd::spec(16, 1, 10)).expect("a budget");
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let child = rd::process_create(budget, exit).expect("a process");
    let before = usage_pages();
    rd::map_fixed(LONE, PAGE_SIZE, rd::rw()).expect("map_fixed the page to move");
    rd::process_map(child, LONE, 0x2000_0000, PAGE_SIZE, rd::rw()).expect("process_map");
    check(
        out,
        usage_pages() == before,
        "a process_map into another budget leaves no page table behind in the source",
    );

    log!(out, "[pt-reclaim] PAGE TABLE RECLAIM PASSED");
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[pt-reclaim] FAIL: {}", info);
    test_programs::park()
}
