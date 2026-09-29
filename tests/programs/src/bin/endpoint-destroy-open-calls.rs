//! Calls taken through an endpoint that is then destroyed (kernel/ipc.md, R3): `Dead` from
//! `receive` on it is the server's cue that every call it took there is abandoned, and no notice
//! follows. Each caller gets `Dead` with its lend consumed; the server's reply to each call is
//! `discarded`, mask 0, and frees what the call held.
//!
//! This program runs first and judges. An owner process in budget B creates endpoint E, which B
//! pays for and dies with, and hands its receive right here. A server in budget A takes two calls
//! on E, each with a lent page, says so, and waits in `receive` on E again; then B is destroyed.
//! The server reports what the kernel answered it in its exit code: `receive`'s `Dead`, each
//! reply's outcome, and its budget's usage before the calls against after the replies. The
//! callers' outcomes are the kernel's `call` results here.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, Error, Received, Return};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// Server exit-code bits: `receive` got `Dead`; the first and second replies were discarded with
/// mask 0; its usage came back to where it started.
const DEAD: u32 = 1;
const FIRST_DISCARDED: u32 = 2;
const SECOND_DISCARDED: u32 = 4;
const USAGE_BACK: u32 = 8;

/// Each caller's result: 1 for `Dead` with its lend consumed, 2 for anything else, 0 until then.
static CALLERS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// The owner, in B: make E, hand its receive right to the judge (slot 1), wait to be killed.
extern "C" fn owner(_: usize) -> ! {
    let e = rd::endpoint_create().expect("E");
    rd::send_waiting(1, &rd::body_with([0; 4], &[e]), None, WAIT).expect("handing E over");
    test_programs::park()
}

/// A reply to `msg_id`, and whether it was discarded with mask 0.
fn discarded(msg_id: u64) -> bool {
    let rec = rd::body([0; 4]).encode();
    let Some(id) = core::num::NonZeroU64::new(msg_id) else { return false };
    matches!(
        redoubt_sys::syscall(&rd::Call::Reply { msg_id: id, body_rec: rec.as_ptr() as usize }),
        Ok(Return::Reply(o)) if !o.delivered && o.installed == 0
    )
}

/// The server, in A, with E in slot 1, A in slot 2 and the judge's inbox in slot 3.
extern "C" fn server(_: usize) -> ! {
    let start = rd::usage(2).expect("A's usage").pages_usage;
    let mut ids = [0u64; 2];
    for id in &mut ids {
        match rd::receive(Some(1), WAIT, 0) {
            Ok(Received::Message(m)) => *id = m.msg_id.get(),
            _ => rd::process_exit(0),
        }
    }
    rd::send_waiting(3, &rd::body([0; 4]), None, WAIT).expect("saying both calls are taken");
    let mut code = 0;
    if rd::receive(Some(1), WAIT, 0) == Err(Error::Dead) {
        code |= DEAD;
    }
    if discarded(ids[0]) {
        code |= FIRST_DISCARDED;
    }
    if discarded(ids[1]) {
        code |= SECOND_DISCARDED;
    }
    if rd::usage(2).expect("A's usage").pages_usage == start {
        code |= USAGE_BACK;
    }
    rd::process_exit(code)
}

/// A caller: lend a page to E on a call nobody answers.
fn caller(arg: usize) {
    let (e, i) = (arg >> 1, arg & 1);
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("a page to lend");
    let dead = matches!(
        rd::call_outcome(e as u32, &rd::body([i, 0, 0, 0]), rd::pages(page, 1), rd::FOREVER),
        Ok((o, _)) if o.status == Err(Error::Dead) && o.lend == redoubt_sys::LendDisposition::Consumed
    );
    CALLERS[i].store(if dead { 1 } else { 2 }, Ordering::Release);
}

fn receive_message(from: u32) -> rd::Message {
    match rd::receive(Some(from), WAIT, 0).expect("a message") {
        Received::Message(m) => m,
        _ => panic!("expected a message"),
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("endpoint-destroy");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");

    let budget_b = rd::create(rd::SYSTEM, &rd::spec(600, 1, 10)).expect("B");
    spawn::spawn(&image, budget_b, exit, owner as *const () as usize, &[], &[inbox]).expect("the owner");
    let handed = receive_message(inbox);
    let e = handed.body.handles.as_slice()[0].expect("E's receive right").index();

    let budget_a = rd::create(rd::SYSTEM, &rd::spec(800, 1, 10)).expect("A");
    spawn::spawn(&image, budget_a, exit, server as *const () as usize, &[], &[e, budget_a, inbox])
        .expect("the server");
    for i in 0..2 {
        rd::thread(caller, (e as usize) << 1 | i).expect("a caller");
    }
    receive_message(inbox);
    // The server is back in `receive` on E.
    test_programs::wait_ms(10);
    rd::destroy(budget_b).expect("destroying B");

    let code = loop {
        match rd::receive(Some(exit), WAIT, 0) {
            Ok(Received::Exit(n)) if n.cause == Cause::Exited => break n.code,
            Ok(Received::Exit(_)) => continue,
            _ => break 0,
        }
    };
    for _ in 0..1_000 {
        if CALLERS.iter().all(|c| c.load(Ordering::Acquire) != 0) {
            break;
        }
        test_programs::wait_ms(1);
    }
    b.check(code & DEAD != 0, format_args!("the server's receive on the destroyed endpoint returns Dead"));
    b.check(
        CALLERS.iter().all(|c| c.load(Ordering::Acquire) == 1),
        format_args!("each caller gets Dead with its lend consumed"),
    );
    b.check(
        code & (FIRST_DISCARDED | SECOND_DISCARDED) == FIRST_DISCARDED | SECOND_DISCARDED,
        format_args!("the server's reply to each taken call is discarded, mask 0"),
    );
    b.check(
        code & USAGE_BACK != 0,
        format_args!("the server's usage is back to its start after both replies"),
    );
    b.finish("ENDPOINT-DESTROY")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("endpoint-destroy", info) }
