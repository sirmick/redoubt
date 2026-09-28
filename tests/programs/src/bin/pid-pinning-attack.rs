//! Untaken exit notices cannot pin the machine's PIDs (`tests/pid-pinning-attack.toml`; R6, R7,
//! R10). A process's PID counts against the process limit of the budget it runs in until its
//! object is freed, and moves to that budget's parent when the budget is destroyed first, so a
//! creator that never takes its children's notices exhausts only its own limit.
//!
//! This program runs first and alone and judges from what the kernel reports: exit notices,
//! `budget_usage` and `process_create`'s results.
//!
//! Pinning: Alice runs in a budget with a process limit of `LIMIT`. She starts children in it, each
//! exiting at once, and never receives their notices, until `process_create` refuses her. The
//! kernel then counts `LIMIT - 1` PIDs in her budget, held by her children's untaken notices, and
//! a process of Bob's, in another budget, still starts and ends. Counted nowhere, Alice's pending
//! notices would hold every free PID, and Bob's `process_create` would fail.
//!
//! The move: a child budget of `P` runs two processes that this program created, and is destroyed
//! with their notices untaken. Their PIDs now count in `P`, so `P` (limit 3) has room for one more
//! process and no second. With the count dropped at the destruction or at the processes' end, `P`
//! would count nothing and have room for three.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, Error, ExitNotice, Received};
use test_programs::{log, logsrv, spawn};

const WAIT: u64 = 5_000_000;
/// Alice's process limit: Alice herself and three children.
const LIMIT: u32 = 4;
/// More children than there are PIDs, so a creator counted nowhere exhausts them.
const TRIES: u32 = 100;
/// The code Bob's process exits with.
const CHILD_CODE: u32 = 7;
/// The exit code of a panicking process (the handler below).
const PANICKED: u32 = 101;

/// A child: exit at once.
extern "C" fn exiter(_: usize) -> ! { rd::process_exit(CHILD_CODE) }

/// A child that waits to be killed.
extern "C" fn parked(_: usize) -> ! { test_programs::park() }

/// Alice: start children in her own budget (slot 1), never taking a notice, until refused. Exit
/// with (children started) * 256 + the error, for the log; the verdict is the kernel's usage.
extern "C" fn alice(_: usize) -> ! {
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("Alice's exit endpoint");
    let mut started = 0;
    let error = loop {
        if started == TRIES {
            break 0;
        }
        match spawn::spawn(&image, 1, exit, exiter as *const () as usize, &[], &[]) {
            Ok(_) => started += 1,
            Err(e) => break e as u32,
        }
        // Let the child end; its notice stays untaken.
        test_programs::wait_ms(2);
    };
    rd::process_exit(started * 256 + error)
}

fn notice(exit: u32) -> ExitNotice {
    match rd::receive(Some(exit), WAIT, 0).expect("an exit notice") {
        Received::Exit(n) => n,
        _ => panic!("expected an exit notice"),
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");

    // Pinning: Alice never takes her children's notices.
    let alice_budget = rd::create(rd::USERS, &rd::spec(2_000, LIMIT, 100)).expect("Alice's budget");
    spawn::spawn(&image, alice_budget, exit, alice as *const () as usize, &[], &[alice_budget])
        .expect("Alice");
    let a = notice(exit);
    log!(logger, "[pid-pinning] Alice started {} children, then got error {}", a.code / 256, a.code % 256);
    let held = rd::usage(alice_budget).expect("Alice's usage").processes_usage;
    log!(logger, "[pid-pinning] Alice's budget counts {} PIDs held by untaken notices", held);
    let alice_ok = a.cause == Cause::Exited && held == LIMIT - 1;

    let bob_budget = rd::create(rd::USERS, &rd::spec(600, 1, 100)).expect("Bob's budget");
    let bob = spawn::spawn(&image, bob_budget, exit, exiter as *const () as usize, &[], &[]);
    let bob_ok = match bob {
        Ok(_) => {
            let b = notice(exit);
            b.cause == Cause::Exited && b.code == CHILD_CODE
        }
        Err(e) => {
            log!(logger, "[pid-pinning] Bob's process_create failed: {:?}", e);
            false
        }
    };
    log!(logger, "[pid-pinning] Bob's process started and exited: {}", bob_ok);

    // The move: two processes run in `child`, a budget below `parent`, and are killed with it;
    // their notices go to `pinned`, which nobody receives from.
    let pinned = rd::endpoint_create().expect("the untaken exit endpoint");
    let parent = rd::create(rd::USERS, &rd::spec(5_000, 3, 100)).expect("P");
    let child = rd::create(parent, &rd::spec(2_000, 2, 50)).expect("P's child");
    for _ in 0..2 {
        spawn::spawn(&image, child, pinned, parked as *const () as usize, &[], &[])
            .expect("a parked process");
    }
    rd::destroy(child).expect("destroying P's child");
    let moved = rd::usage(parent).expect("P's usage").processes_usage;
    log!(logger, "[pid-pinning] after its child's destruction P counts {} PIDs", moved);
    let first = rd::process_create(parent, pinned).map(|_| ());
    let second = rd::process_create(parent, pinned).map(|_| ());
    log!(logger, "[pid-pinning] P's next two process_create: {:?}, {:?}", first, second);
    let move_ok = moved == 2 && first.is_ok() && second == Err(Error::OutOfProcesses);

    if alice_ok && bob_ok && move_ok {
        log!(logger, "PID PINNING ATTACK TEST PASSED");
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { rd::process_exit(PANICKED) }
