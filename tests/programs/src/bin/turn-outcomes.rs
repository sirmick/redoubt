//! Trusted checker of a server's turn's outcomes (R13; kernel/ipc.md, "A server's turn"). The
//! main thread is the server: it makes `reply_receive` and `send_receive` calls and judges the
//! whole result the kernel hands back, `a1` (what the first half did) with the status, against
//! what its caller thread and its helper thread then see. A turn refused before anything is done
//! changes nothing; after the first half acts, only the receive half's errors remain, each with
//! what the first half did; a taken send goes on to its receive in the taker's entry, and its
//! receive's endpoint destroyed meanwhile is that half's `Dead`.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_sys::ReplyOutcome;
use test_programs::rd::{self, Error, FOREVER, FirstDone, Received};
use test_programs::{Logger, log};

/// The endpoint the caller calls and the server receives on.
static ENDPOINT: AtomicUsize = AtomicUsize::new(0);
/// A mint of it stamped with `SCOPE`, for the caller's last call.
static STAMPED: AtomicUsize = AtomicUsize::new(0);
/// The words of the reply each of the caller's calls got, by call (1-4), and the last's outcome.
static REPLY: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
static LAST: AtomicUsize = AtomicUsize::new(0);

/// The helper's next step and the one it finished; its object.
static STEP: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static OBJECT: AtomicUsize = AtomicUsize::new(0);
static OTHER: AtomicUsize = AtomicUsize::new(0);
/// Destroy budget `OBJECT` after a while.
const DESTROY_LATER: usize = 1;
/// Take the send queued on `OBJECT`, then send to `OTHER`.
const TAKE_THEN_SEND: usize = 2;
/// Destroy budget `OTHER`, then take the send queued on `OBJECT`.
const DESTROY_THEN_TAKE: usize = 3;

/// Long enough for the main thread to block in its turn first.
const LATER_MS: u64 = 20;

fn caller(_: usize) {
    let endpoint = ENDPOINT.load(Ordering::Acquire) as u32;
    for n in 1..=3 {
        let (out, reply) = rd::call_outcome(endpoint, &rd::body([n, 0, 0, 0]), None, FOREVER).unwrap();
        assert_eq!(out.status, Ok(()), "call {n}");
        REPLY[n].store(reply.unwrap().words[0], Ordering::Release);
    }
    let stamped = STAMPED.load(Ordering::Acquire) as u32;
    let (out, _) = rd::call_outcome(stamped, &rd::body([4, 0, 0, 0]), None, FOREVER).unwrap();
    LAST.store(out.status.err().map_or(0, |e| e as usize), Ordering::Release);
    REPLY[4].store(1, Ordering::Release);
}

fn helper(_: usize) {
    let mut done = 0;
    loop {
        let step = STEP.load(Ordering::Acquire);
        if step == done {
            test_programs::wait_ms(1);
            continue;
        }
        let (object, other) = (OBJECT.load(Ordering::Acquire) as u32, OTHER.load(Ordering::Acquire) as u32);
        match step % 4 {
            DESTROY_LATER => {
                test_programs::wait_ms(LATER_MS);
                rd::destroy(object).unwrap();
            }
            TAKE_THEN_SEND => {
                test_programs::wait_ms(LATER_MS);
                let Ok(Received::Message(m)) = rd::receive(Some(object), FOREVER, 0) else { panic!("take") };
                assert_eq!(m.body.words[0], 55);
                rd::send(other, &rd::body([77, 0, 0, 0]), None, FOREVER).unwrap();
            }
            DESTROY_THEN_TAKE => {
                test_programs::wait_ms(LATER_MS);
                rd::destroy(other).unwrap();
                let Ok(Received::Message(m)) = rd::receive(Some(object), FOREVER, 0) else { panic!("take") };
                assert_eq!(m.body.words[0], 56);
            }
            _ => panic!("step"),
        }
        done = step;
        DONE.store(done, Ordering::Release);
    }
}

/// Runs helper step `kind`; returns the step's number, for [`finished`].
fn later(kind: usize, object: u32, other: u32) -> usize {
    OBJECT.store(object as usize, Ordering::Release);
    OTHER.store(other as usize, Ordering::Release);
    let step = (STEP.load(Ordering::Acquire) / 4 + 1) * 4 + kind;
    STEP.store(step, Ordering::Release);
    step
}

fn finished(step: usize) {
    while DONE.load(Ordering::Acquire) != step {
        test_programs::wait_ms(1);
    }
}

/// The caller's call `n`, taken by a plain receive.
fn take(endpoint: u32, n: usize) -> u64 {
    let Ok(Received::Message(m)) = rd::receive(Some(endpoint), FOREVER, 0) else { panic!("call {n}") };
    assert_eq!(m.body.words[0], n);
    m.msg_id.get()
}

/// A budget below this program's own, to own an endpoint whose death a turn sees.
fn owned_endpoint() -> (u32, u32) {
    let budget = rd::create(rd::ROOT, &rd::spec(1, 0, 0)).unwrap();
    (budget, rd::endpoint_create_owned(Some(budget)).unwrap())
}

const DELIVERED: FirstDone = FirstDone::Replied(ReplyOutcome { delivered: true, installed: 0 });

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = test_programs::logsrv::start();
    let endpoint = rd::endpoint_create().unwrap();
    ENDPOINT.store(endpoint as usize, Ordering::Release);
    let scope = rd::create(rd::ROOT, &rd::spec(0, 0, 0)).unwrap();
    STAMPED.store(rd::mint_from_handle(endpoint, 7, Some(scope)).unwrap() as usize, Ordering::Release);
    rd::thread(caller, 0).expect("caller");
    rd::thread(helper, 0).expect("helper");

    // Refused before anything is done: `a1` = 0, the call still open and its caller waiting, the
    // current call kept, a transfer still the sender's.
    let id1 = take(endpoint, 1);
    let badged = rd::mint_from_handle(endpoint, 9, None).unwrap();
    let (gone_budget, gone) = owned_endpoint();
    rd::destroy(gone_budget).unwrap();
    let reply = rd::body([101, 0, 0, 0]);
    for (from, id, error) in [
        (badged, id1, Error::NotPermitted),
        (gone, id1, Error::Dead),
        (u32::MAX, id1, Error::BadHandle),
        (endpoint, id1 + 1000, Error::InvalidArgument),
    ] {
        assert_eq!(rd::reply_receive(id, &reply, Some(from), 0, 0), Ok((FirstDone::Nothing, Err(error))));
    }
    let page = rd::page();
    rd::poke(page, 0xfeed);
    let quiet = rd::endpoint_create().unwrap();
    for (to, from, error) in [(quiet, badged, Error::NotPermitted), (gone, endpoint, Error::Dead)] {
        let turn = rd::send_receive(to, &rd::body([0; 4]), rd::pages(page, 1), 0, Some(from), 0, 0);
        assert_eq!(turn, Ok((FirstDone::Nothing, Err(error))));
        assert_eq!(rd::peek(page), 0xfeed, "the transfer is still the sender's");
    }
    test_programs::wait_ms(LATER_MS);
    assert_eq!(REPLY[1].load(Ordering::Acquire), 0, "the caller is still waiting");
    log!(logger, "turn-outcomes refusals before anything is done change nothing");

    // The reply delivered, and the caller's next call taken, in one call.
    let Ok((DELIVERED, Ok(Received::Message(m)))) =
        rd::reply_receive(id1, &reply, Some(endpoint), FOREVER, 0)
    else {
        panic!("reply then receive")
    };
    assert_eq!((m.body.words[0], REPLY[1].load(Ordering::Acquire)), (2, 101));
    log!(logger, "turn-outcomes reply delivered and the next call taken in one call");

    // After the reply only the receive half's errors remain, with what the reply did. Timeout:
    // the caller readied by the reply runs no sooner than this entry ends, so nothing is queued.
    let turn = rd::reply_receive(m.msg_id.get(), &rd::body([102, 0, 0, 0]), Some(endpoint), 0, 0);
    assert_eq!(turn, Ok((DELIVERED, Err(Error::Timeout))));
    // Dead: the receive's endpoint is destroyed while the receive half waits.
    let id3 = take(endpoint, 3);
    assert_eq!(REPLY[2].load(Ordering::Acquire), 102, "stored before call 3");
    let (budget, doomed) = owned_endpoint();
    let step = later(DESTROY_LATER, budget, 0);
    let turn = rd::reply_receive(id3, &rd::body([103, 0, 0, 0]), Some(doomed), FOREVER, 0);
    assert_eq!(turn, Ok((DELIVERED, Err(Error::Dead))));
    finished(step);
    log!(logger, "turn-outcomes failed receive half reports the delivered reply");

    // The reply to an abandoned call is discarded: its caller's stamp was destroyed.
    let id4 = take(endpoint, 4);
    assert_eq!(REPLY[3].load(Ordering::Acquire), 103, "stored before call 4");
    rd::destroy(scope).unwrap();
    let notice = rd::receive(Some(endpoint), FOREVER, 0);
    assert!(matches!(notice, Ok(Received::Abandoned(id)) if id.get() == id4), "{notice:?}");
    let turn = rd::reply_receive(id4, &rd::body([104, 0, 0, 0]), Some(endpoint), 0, 0);
    assert_eq!(
        turn,
        Ok((FirstDone::Replied(ReplyOutcome { delivered: false, installed: 0 }), Err(Error::Timeout)))
    );
    while REPLY[4].load(Ordering::Acquire) == 0 {
        test_programs::wait_ms(1);
    }
    assert_eq!(LAST.load(Ordering::Acquire), Error::Dead as usize, "the caller saw its call die");
    log!(logger, "turn-outcomes reply to an abandoned call discarded");

    // A send that fails receives nothing, and its transfer is the sender's again: refused by
    // timing out, and by its endpoint destroyed while it waits (`Dead` with `a1` = 0).
    let turn = rd::send_receive(quiet, &rd::body([0; 4]), rd::pages(page, 1), 0, Some(endpoint), FOREVER, 0);
    assert_eq!(turn, Ok((FirstDone::Nothing, Err(Error::Timeout))));
    assert_eq!(rd::peek(page), 0xfeed);
    let (budget, doomed) = owned_endpoint();
    let step = later(DESTROY_LATER, budget, 0);
    let turn =
        rd::send_receive(doomed, &rd::body([0; 4]), rd::pages(page, 1), FOREVER, Some(endpoint), FOREVER, 0);
    assert_eq!(turn, Ok((FirstDone::Nothing, Err(Error::Dead))));
    finished(step);
    assert_eq!(rd::peek(page), 0xfeed);
    log!(logger, "turn-outcomes failed send receives nothing; transfer back");

    // A blocked send taken goes on to its receive in the taker's entry, and what is sent to it
    // then answers it; its receive's endpoint destroyed while it waits as a sender is that half's
    // `Dead`, with `a1` = 1.
    let (to_budget, to) = owned_endpoint();
    let step = later(TAKE_THEN_SEND, to, quiet);
    let Ok((FirstDone::Sent, Ok(Received::Message(m)))) =
        rd::send_receive(to, &rd::body([55, 0, 0, 0]), None, FOREVER, Some(quiet), FOREVER, 0)
    else {
        panic!("send then receive")
    };
    assert_eq!(m.body.words[0], 77);
    finished(step);
    let (budget, doomed) = owned_endpoint();
    let step = later(DESTROY_THEN_TAKE, to, budget);
    let turn = rd::send_receive(to, &rd::body([56, 0, 0, 0]), None, FOREVER, Some(doomed), FOREVER, 0);
    assert_eq!(turn, Ok((FirstDone::Sent, Err(Error::Dead))));
    finished(step);
    rd::destroy(to_budget).unwrap();
    log!(
        logger,
        "turn-outcomes taken send continues to its receive; its endpoint gone is Dead after the send"
    );
    log!(logger, "turn-outcomes TURN OUTCOMES PASSED");
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "turn-outcomes FAILED: {}", info);
    test_programs::park()
}
