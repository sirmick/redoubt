//! An ending process pumps each endpoint once, after all of its threads have ended (R4b, R3,
//! I15; kernel/ipc.md).
//!
//! This program runs first and judges. It makes endpoint E and starts process P with a receive
//! right to it:
//! - P's first thread takes `MAX_OPEN_CALLS` calls on E, the judge's, each of which times out and is
//!   abandoned but stays open until replied to, so R4a keeps P from taking another;
//! - P's second thread, C, calls E, and the judge's server thread S takes that call and holds it;
//! - P's third thread, R, waits in `receive` on E;
//! - the judge's thread Q calls E, and the call is queued: R may not take it (R4a), and no thread of the
//!   judge is receiving.
//!
//! Then P's budget is destroyed. Its first thread's calls close, so P falls under
//! `MAX_OPEN_CALLS`, and C's call is abandoned. Q's call must still be queued afterwards
//! (R4b: queued senders keep waiting for the restarted server), not taken by R on its way out and
//! failed `Dead`. A new server thread then takes Q's call and replies, and S gets exactly one
//! notice for C's call (R3, I15). Every verdict is what the kernel answered: Q's `call`, the new
//! server's `receive`, and S's.
//!
//! The order matters: C's tid is below R's, so C ends, and a kernel that pumps after each thread
//! pumps E, while R still receives on it. With R below C, R would end first and nothing would
//! show. In a traced build the threads' span of P's end covers the pumps after its threads too.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Error, MAX_OPEN_CALLS, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// How long each of the judge's filling calls waits before it is abandoned.
const FILL_TIMEOUT: u64 = 2_000;
/// Message tags, the first word of each body.
const FILL: usize = 1;
const FULL: usize = 2;
const READY: usize = 3;
const FROM_C: usize = 4;
const FROM_Q: usize = 5;
/// The new server's reply to Q.
const ANSWER: usize = 7;

/// E, in the judge's table.
static ENDPOINT: AtomicUsize = AtomicUsize::new(0);
/// S has taken C's call: its id, 0 until then.
static C_CALL: AtomicUsize = AtomicUsize::new(0);
/// The judge lets S receive again.
static S_GO: AtomicUsize = AtomicUsize::new(0);
/// What S's receives after the kill gave: notices for C's call, then anything else. `S_DONE` is
/// set once S's receive times out.
static S_NOTICES: AtomicUsize = AtomicUsize::new(0);
static S_OTHER: AtomicUsize = AtomicUsize::new(0);
static S_DONE: AtomicUsize = AtomicUsize::new(0);
/// Q's call: 0 while it waits, 1 for the new server's answer, 2 for `Dead`, 3 for anything else.
static Q: AtomicUsize = AtomicUsize::new(0);
/// The new server took Q's call.
static NEW_TOOK_Q: AtomicUsize = AtomicUsize::new(0);

fn endpoint() -> u32 { ENDPOINT.load(Ordering::Acquire) as u32 }

// --- P, with E in slot 1 and the judge's inbox in slot 2 ---------------------------------------

/// P's first thread: take `MAX_OPEN_CALLS` calls on E and never answer them, then make C and R.
extern "C" fn victim(_: usize) -> ! {
    let mut taken = 0;
    while taken < MAX_OPEN_CALLS {
        // The notices for the calls that timed out come too, before the next call (R3).
        match rd::receive(Some(1), WAIT, 0) {
            Ok(Received::Message(_)) => taken += 1,
            Ok(_) => {}
            Err(_) => rd::process_exit(1),
        }
    }
    // The judge answers once its server is waiting for C's call.
    if rd::call(2, &rd::body([FULL, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(2);
    }
    rd::thread(caller, 0).expect("C");
    test_programs::wait_ms(20);
    rd::thread(receiver, 0).expect("R");
    test_programs::wait_ms(20);
    rd::send_waiting(2, &rd::body([READY, 0, 0, 0]), None, WAIT).expect("saying C and R are in place");
    test_programs::park()
}

/// C: a call through E, which the judge's S takes and holds.
fn caller(_: usize) { let _ = rd::call(1, &rd::body([FROM_C, 0, 0, 0]), None, rd::FOREVER); }

/// R: a later thread waiting in `receive` on E. On a correct kernel it never gets a message.
fn receiver(_: usize) { let _ = rd::receive(Some(1), rd::FOREVER, 0); }

// --- The judge's threads ------------------------------------------------------------------------

/// S: take C's call, hold it until P is gone, then receive the notice it is owed.
fn server(_: usize) {
    match rd::receive(Some(endpoint()), WAIT, 0) {
        Ok(Received::Message(m)) if m.body.words[0] == FROM_C => {
            C_CALL.store(m.msg_id.get() as usize, Ordering::Release);
        }
        _ => return,
    }
    while S_GO.load(Ordering::Acquire) == 0 {
        test_programs::wait_ms(1);
    }
    let c = C_CALL.load(Ordering::Acquire) as u64;
    loop {
        match rd::receive(Some(endpoint()), 50_000, 0) {
            Ok(Received::Abandoned(id)) if id.get() == c => {
                S_NOTICES.fetch_add(1, Ordering::AcqRel);
                let _ = rd::reply(c, &rd::body([0; 4]));
            }
            Err(Error::Timeout) => break,
            _ => {
                S_OTHER.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
    S_DONE.store(1, Ordering::Release);
}

/// The restarted server: take one message on E and answer it.
fn new_server(_: usize) {
    if let Ok(Received::Message(m)) = rd::receive(Some(endpoint()), WAIT, 0) {
        if m.body.words[0] == FROM_Q {
            NEW_TOOK_Q.store(1, Ordering::Release);
        }
        let _ = rd::reply(m.msg_id.get(), &rd::body([ANSWER, 0, 0, 0]));
    }
}

/// Q: a call queued on E while P is killed.
fn queued(_: usize) {
    let got = match rd::call(endpoint(), &rd::body([FROM_Q, 0, 0, 0]), None, rd::FOREVER) {
        Ok(r) if r.words[0] == ANSWER => 1,
        Err(Error::Dead) => 2,
        _ => 3,
    };
    Q.store(got, Ordering::Release);
}

fn receive_message(from: u32, timeout: u64) -> Option<rd::Message> {
    match rd::receive(Some(from), timeout, 0) {
        Ok(Received::Message(m)) => Some(m),
        _ => None,
    }
}

/// Wait up to a second for `flag` to be set.
fn wait_for(flag: &AtomicUsize) -> usize {
    for _ in 0..1_000 {
        let v = flag.load(Ordering::Acquire);
        if v != 0 {
            return v;
        }
        test_programs::wait_ms(1);
    }
    flag.load(Ordering::Acquire)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("ending-pumps-once");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");
    let e = rd::endpoint_create().expect("E");
    ENDPOINT.store(e as usize, Ordering::Release);

    let budget_p = rd::create(rd::SYSTEM, &rd::spec(800, 1, 10)).expect("P's budget");
    spawn::spawn(&image, budget_p, exit, victim as *const () as usize, &[], &[e, inbox]).expect("P");

    // Fill P to `MAX_OPEN_CALLS`: each call is taken, times out and is abandoned, and stays open.
    let mut full = None;
    for _ in 0..4 * MAX_OPEN_CALLS {
        let _ = rd::call(e, &rd::body([FILL, 0, 0, 0]), None, FILL_TIMEOUT);
        if let Some(m) = receive_message(inbox, 0) {
            full = Some(m);
            break;
        }
    }
    let Some(full) = full.filter(|m| m.body.words[0] == FULL) else {
        b.check(false, format_args!("P took {} calls", MAX_OPEN_CALLS));
        b.finish("ENDING-PUMPS-ONCE")
    };
    rd::thread(server, 0).expect("S");
    test_programs::wait_ms(20);
    rd::reply(full.msg_id.get(), &rd::body([0; 4])).expect("letting P go on");
    let ready = receive_message(inbox, WAIT).is_some_and(|m| m.body.words[0] == READY);
    b.check(
        ready && C_CALL.load(Ordering::Acquire) != 0,
        format_args!("S holds C's call, and R is receiving on E"),
    );

    rd::thread(queued, 0).expect("Q");
    test_programs::wait_ms(20);
    b.check(Q.load(Ordering::Acquire) == 0, format_args!("Q's call is queued before the kill"));

    rd::destroy(budget_p).expect("destroying P's budget");
    test_programs::wait_ms(20);
    let q = Q.load(Ordering::Acquire);
    b.check(q == 0, format_args!("Q is still waiting after the kill (got {})", q));

    rd::thread(new_server, 0).expect("the new server");
    let q = wait_for(&Q);
    b.check(
        q == 1 && NEW_TOOK_Q.load(Ordering::Acquire) == 1,
        format_args!("a new server takes Q's call, and Q gets its reply (got {})", q),
    );

    S_GO.store(1, Ordering::Release);
    wait_for(&S_DONE);
    let (notices, other) = (S_NOTICES.load(Ordering::Acquire), S_OTHER.load(Ordering::Acquire));
    b.check(
        notices == 1 && other == 0,
        format_args!("C's abandoned call gets exactly one notice (got {}, and {} else)", notices, other),
    );
    b.finish("ENDING-PUMPS-ONCE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("ending-pumps-once", info) }
