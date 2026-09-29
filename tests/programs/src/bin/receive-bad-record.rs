//! A bad record takes nothing (kernel/ipc.md): a thread waiting in `receive` whose record page
//! another of its process's threads unmaps gets `InvalidArgument` when an abandoned-call notice or
//! an interrupt comes for it, and that notice or interrupt arrives on its next `receive` with a
//! good record.
//!
//! This program runs first, so it holds the goldfish RTC and its interrupt. The verdicts are the
//! kernel's: the results of `receive`, and how long the first one waited by `time_now`, which
//! tells a record the kernel re-checked at delivery from one refused when the call was decoded.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Call, Error, Received};
use test_programs::sched::{Bench, rtc};

const FOREVER: u64 = u64::MAX;
/// How long the caller waits before its call is abandoned, in µs.
const CALL_TIMEOUT: u64 = 40_000;
/// How long the RTC alarm is set ahead of the unmap, in ns.
const ALARM_AHEAD: u64 = 20_000_000;
/// The least a first `receive` must have waited to have been blocked when its record went.
const BLOCKED_US: u64 = 10_000;
/// The second `receive`'s timeout: what it waits for is already there.
const AGAIN: u64 = 1_000_000;

/// The receiving thread's record page, once mapped; 0 before.
static RECORD: AtomicUsize = AtomicUsize::new(0);
/// The first `receive`'s error (as `Error as usize`, or `usize::MAX` for none) and its wait in µs.
static FIRST: AtomicUsize = AtomicUsize::new(0);
static WAITED: AtomicUsize = AtomicUsize::new(0);
/// What the second `receive` got: the abandoned call's id, or 1 for an interrupt; 0 for anything
/// else, `usize::MAX` until it returns. Ids here are small, so a word holds them on rv32 too.
static SECOND: AtomicUsize = AtomicUsize::new(usize::MAX);
/// The id of the call the receiver took.
static TOOK: AtomicUsize = AtomicUsize::new(0);

/// A `receive` on `from` writing its record at `rec`: only its status matters here.
fn receive_at(from: u32, rec: usize) -> Result<(), Error> {
    let call =
        Call::Receive { from: Some(rd::h(from)), timeout: FOREVER, max_transfer: 0, received_rec: rec };
    redoubt_sys::syscall(&call).map(|_| ())
}

/// The first `receive` on `from`, with its record in a page of its own that the main thread
/// unmaps while it waits; then a second with a good record.
fn wait_twice(from: u32) {
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("a record page");
    let t0 = rd::time_now().unwrap();
    RECORD.store(page, Ordering::Release);
    let first = receive_at(from, page);
    WAITED.store((rd::time_now().unwrap() - t0) as usize, Ordering::Release);
    FIRST.store(first.err().map_or(usize::MAX, |e| e as usize), Ordering::Release);
    let second = match rd::receive(Some(from), AGAIN, 0) {
        Ok(Received::Abandoned(id)) => id.get() as usize,
        Ok(Received::Interrupt) => 1,
        _ => 0,
    };
    SECOND.store(second, Ordering::Release);
}

/// The server: take the caller's call, then wait on the endpoint while the call is abandoned.
fn server(endpoint: usize) {
    let endpoint = endpoint as u32;
    match rd::receive(Some(endpoint), 5_000_000, 0) {
        Ok(Received::Message(m)) => TOOK.store(m.msg_id.get() as usize, Ordering::Release),
        _ => return,
    }
    wait_twice(endpoint);
}

/// The caller: a call the server takes and never answers, so it is abandoned at the timeout.
fn caller(endpoint: usize) { let _ = rd::call(endpoint as u32, &rd::body([1, 0, 0, 0]), None, CALL_TIMEOUT); }

/// The interrupt's receiver.
fn interrupted(irq: usize) { wait_twice(irq as u32); }

/// Wait until the receiving thread has mapped its record page, give it time to block, unmap the
/// page; `then` makes the notice or interrupt come.
fn unmap_while_waiting(then: impl FnOnce()) {
    let mut page = 0;
    for _ in 0..5_000 {
        page = RECORD.load(Ordering::Acquire);
        if page != 0 {
            break;
        }
        test_programs::wait_ms(1);
    }
    test_programs::wait_ms(5);
    rd::unmap(page, rd::PAGE_SIZE).expect("unmapping the waiting thread's record");
    then();
    for _ in 0..5_000 {
        if SECOND.load(Ordering::Acquire) != usize::MAX {
            return;
        }
        test_programs::wait_ms(1);
    }
}

fn verdict(b: &mut Bench, what: &str, want: usize) {
    let first = FIRST.load(Ordering::Acquire);
    let waited = WAITED.load(Ordering::Acquire);
    b.check(
        first == Error::InvalidArgument as usize && waited as u64 >= BLOCKED_US,
        format_args!("{what} to a bad record: the waiting receive gets InvalidArgument (after {waited} us)"),
    );
    b.check(
        SECOND.load(Ordering::Acquire) == want,
        format_args!("{what} arrives on the next receive with a good record"),
    );
    RECORD.store(0, Ordering::Release);
    SECOND.store(usize::MAX, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::log_rx();
    let mut b = Bench::new("receive-bad-record");

    let endpoint = rd::endpoint_create().expect("an endpoint");
    rd::thread(server, endpoint as usize).expect("the server thread");
    rd::thread(caller, endpoint as usize).expect("the caller thread");
    unmap_while_waiting(|| {});
    let took = TOOK.load(Ordering::Acquire);
    verdict(&mut b, "an abandoned-call notice", took);

    let Some((_, base, irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("RECEIVE-BAD-RECORD")
    };
    rd::thread(interrupted, irq as usize).expect("the interrupt thread");
    unmap_while_waiting(|| rtc::alarm(base, rtc::now_ns(base) + ALARM_AHEAD));
    rtc::clear(base);
    verdict(&mut b, "an interrupt", 1);
    b.finish("RECEIVE-BAD-RECORD")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("receive-bad-record", info) }
