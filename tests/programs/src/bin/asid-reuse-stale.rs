//! No cached translation outlives its mapping (`tests/asid-reuse-stale.toml`; kernel/memory-layout.md,
//! "`satp`"): a process's PID is its ASID, so a PID given out again must find its ASID flushed,
//! and an unmap must flush its page. This program runs first and alone, and checks itself: each
//! verdict is a value read through the MMU, where a stale translation would show another frame.
//!
//! One process: map V and the page after it, write a marker at V, unmap V alone, map a page
//! elsewhere and write a second marker there (the freed frame is the next one handed out), map V
//! again and read it. A fresh page reads 0; a stale translation would read the old frame, which
//! holds one marker or the other. The page after V keeps V's leaf table, so the unmap frees no
//! table and only its own flush covers its write.
//!
//! Across processes: child A maps V, writes a marker, reads it back and exits. This program then
//! spawns children until one draws A's PID; each maps V and reports what it reads there. Every one
//! must read 0, B (A's PID) included: a stale translation under A's ASID would read A's frame.
//!
//! A spawned child is a copy of this image, statics and all, so no child uses `Logger`: in the
//! copy, `logsrv` reads as started and would print to a console the child cannot reach.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, ExitNotice, Message, Received};
use test_programs::{log, logsrv, spawn};

const WAIT: u64 = 2_000_000;
/// The page every process here maps: between the message area and the `map_anon` area, clear of
/// the image, the stack, the startup page and the bundle the loader maps into this first program
/// (memory-layout.md, "Regions").
const V: usize = 0x5800_0000;
/// Where the one-process check maps its second page.
const ELSEWHERE: usize = 0x5900_0000;
/// The markers fit a message word on both widths, so a child reports one whole.
const MARKER: u64 = 0x5a5a_1234;
const SECOND: u64 = 0x0bad_cafe;
/// The exit code of a panicking process (the handler below).
const PANICKED: u32 = 101;
/// PIDs are drawn at random from at most 510 (`MAX_PROCESS_COUNT` is 511, kernel/processes.md),
/// so this many tries miss A's only with odds of about e^-16.
const TRIES: usize = 8192;

/// A: map V, write the marker, and report what it reads back.
extern "C" fn writer(_: usize) -> ! {
    rd::map_fixed(V, rd::PAGE_SIZE, rd::rw()).expect("A maps V");
    rd::poke(V, MARKER);
    rd::send(1, &rd::body([rd::peek(V) as usize, 0, 0, 0]), None, WAIT).ok();
    rd::process_exit(0)
}

/// Every child after A, B among them: map V and report what it reads there.
extern "C" fn reader(_: usize) -> ! {
    rd::map_fixed(V, rd::PAGE_SIZE, rd::rw()).expect("a child maps V");
    rd::send(1, &rd::body([rd::peek(V) as usize, 0, 0, 0]), None, WAIT).ok();
    rd::process_exit(0)
}

fn message(endpoint: u32) -> Message {
    match rd::receive(Some(endpoint), WAIT, 0).expect("a message") {
        Received::Message(m) => m,
        _ => panic!("expected a message"),
    }
}

fn notice(exit: u32) -> ExitNotice {
    match rd::receive(Some(exit), WAIT, 0).expect("an exit notice") {
        Received::Exit(n) => n,
        _ => panic!("expected an exit notice"),
    }
}

/// What a child read at V, and its notice, which must be a clean exit.
fn report(f: u32, exit: u32) -> (u64, ExitNotice) {
    let read = message(f).body.words[0] as u64;
    let n = notice(exit);
    assert_eq!((n.cause, n.code), (Cause::Exited, 0), "a child did not exit cleanly");
    (read, n)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();

    // --- One process ------------------------------------------------------------------------
    rd::map_fixed(V, 2 * rd::PAGE_SIZE, rd::rw()).expect("map V and the page after it");
    rd::poke(V, MARKER);
    assert_eq!(rd::peek(V), MARKER);
    rd::unmap(V, rd::PAGE_SIZE).expect("unmap V");
    rd::map_fixed(ELSEWHERE, rd::PAGE_SIZE, rd::rw()).expect("map elsewhere");
    rd::poke(ELSEWHERE, SECOND);
    rd::map_fixed(V, rd::PAGE_SIZE, rd::rw()).expect("map V again");
    let again = rd::peek(V);
    log!(
        logger,
        "[asid-reuse] one process: V reads {:#x} after an unmap and a new map, neither marker: {}",
        again,
        again == 0
    );

    // --- A ----------------------------------------------------------------------------------
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");
    let kids = rd::create(rd::USERS, &rd::spec(600, 2, 100)).expect("the children's budget");
    let f = rd::endpoint_create().expect("F");
    let f_send = rd::mint_from_handle(f, 1, None).expect("a send on F");
    spawn::spawn(&image, kids, exit, writer as *const () as usize, &[], &[f_send]).expect("A");
    let (a_read, a) = report(f, exit);
    log!(
        logger,
        "[asid-reuse] A (pid {}) wrote its marker at V and read it back: {}",
        a.pid,
        a_read == MARKER
    );

    // --- B: spawn until a child draws A's PID -----------------------------------------------
    let mut all_zero = true;
    let mut b = None;
    for _ in 0..TRIES {
        spawn::spawn(&image, kids, exit, reader as *const () as usize, &[], &[f_send]).expect("a child");
        let (read, n) = report(f, exit);
        all_zero &= read == 0;
        if n.pid == a.pid {
            b = Some(read);
            break;
        }
    }
    let b_read = b.expect("no child drew A's PID");
    log!(logger, "[asid-reuse] B drew pid {}", a.pid);
    log!(logger, "[asid-reuse] B read {:#x} at V, not A's marker: {}", b_read, b_read == 0);
    log!(logger, "[asid-reuse] every child read 0 at V: {}", all_zero);
    if again == 0 && a_read == MARKER && b_read == 0 && all_zero {
        log!(logger, "ASID REUSE STALE TEST PASSED");
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { rd::process_exit(PANICKED) }
