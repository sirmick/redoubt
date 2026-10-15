//! A call's kernel time does not grow with what other budgets hold (kernel/scheduling.md R12),
//! beyond the memory calls `scan-bounds` measures: making an endpoint, minting a handle, closing
//! one, making a thread and its exit, a call and its reply, receiving a queued message, reading
//! the clock and a budget's usage. Each is made sixteen times on an empty system, then again
//! after one hostile budget has filled its handle table, made two thousand endpoints, queued
//! 190 threads in `send` at this program's endpoint and parked 60 more in timed waits. The
//! kernel's time for each is its lock section from a `hold-trace` kernel, net of the checked
//! build's audits; the bench's post-check (`CALL-BOUNDS`) holds the second window's longest
//! section of every call made in the first to twice the first's, or a floor of 100 ticks (10 µs
//! at the 10 MHz timebase: a scan of the full handle table, or of the endpoints, would cost tens
//! of microseconds more). A call the hostile makes in the second window only is reported, not
//! judged. This program's own checks say only that the hostile built what it was asked to.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use test_programs::rd::{self, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

/// Samples of each call, a window.
const SAMPLES: usize = 16;
/// The hostile's threads queued in `send` at this program's endpoint, and parked in timed waits.
const SENDERS: usize = 190;
const WAITERS: usize = 60;
/// The endpoints the hostile makes (each a kernel object frame), closing each handle.
const FILL_ENDPOINTS: usize = 2_000;
/// The second window's longest section of a call is held to this many times the first's, or the
/// floor in ticks.
const FACTOR: u64 = 2;
const FLOOR_TICKS: u64 = 100;
const STACK_PAGES: usize = 2;

/// A thread of this process running `f(arg)` on a fresh stack.
fn start_thread(f: extern "C" fn(usize) -> !, arg: usize) -> Result<u32, rd::Error> {
    let stack = rd::map_anon(STACK_PAGES * rd::PAGE_SIZE, rd::rw())?;
    rd::thread_create(f as *const () as usize, stack + STACK_PAGES * rd::PAGE_SIZE - 16, arg)
}

/// Threads of [`exiter`] that have exited.
static EXITED: AtomicUsize = AtomicUsize::new(0);

extern "C" fn exiter(_: usize) -> ! {
    EXITED.fetch_add(1, SeqCst);
    rd::thread_exit().ok();
    test_programs::park()
}

/// The server: reply to every call on the endpoint in `ep`.
extern "C" fn server(ep: usize) -> ! {
    loop {
        if let Ok(Received::Message(m)) = rd::receive(Some(ep as u32), rd::FOREVER, 0) {
            let _ = rd::reply(m.msg_id.get(), &rd::body([0; 4]));
        }
    }
}

/// The first window's queued sender: [`SAMPLES`] sends through the handle in `h`, each queued
/// until this program receives it.
extern "C" fn helper(h: usize) -> ! {
    for _ in 0..SAMPLES {
        let _ = rd::send(h as u32, &rd::body([0; 4]), None, rd::FOREVER);
    }
    rd::thread_exit().ok();
    test_programs::park()
}

/// The hostile's sender: one send at this program's endpoint (its slot 2), queued for good.
extern "C" fn sender(_: usize) -> ! {
    let _ = rd::send(2, &rd::body([0; 4]), None, rd::FOREVER);
    rd::thread_exit().ok();
    test_programs::park()
}

/// The hostile's waiter: a timed wait of an hour.
extern "C" fn waiter(_: usize) -> ! {
    let _ = rd::receive(None, 3_600_000_000, 0);
    rd::thread_exit().ok();
    test_programs::park()
}

/// The hostile, in a budget of its own: endpoints (each handle closed), its senders and waiters,
/// then every handle its table has room for; it reports the four counts on slot 1 and stays.
extern "C" fn hostile(_: usize) -> ! {
    let mut endpoints = 0;
    for _ in 0..FILL_ENDPOINTS {
        let Ok(h) = rd::endpoint_create() else { break };
        let _ = rd::close(h);
        endpoints += 1;
    }
    let (mut senders, mut waiters, mut handles) = (0, 0, 0);
    for _ in 0..SENDERS {
        senders += usize::from(start_thread(sender, 0).is_ok());
    }
    for _ in 0..WAITERS {
        waiters += usize::from(start_thread(waiter, 0).is_ok());
    }
    let own = rd::endpoint_create().expect("the hostile's endpoint");
    while rd::mint_from_handle(own, 1, None).is_ok() {
        handles += 1;
    }
    let _ = rd::send(1, &rd::body([endpoints, senders, waiters, handles]), None, rd::FOREVER);
    test_programs::park()
}

/// One window of the calls, in the kernel's ticks at its start and end: `time_now`'s
/// microseconds since the kernel's boot times `tpu`, the clock's ticks per microsecond, the end
/// rounded up; the trace's sections count from the same boot, not from the machine's `rdtime`,
/// which the firmware and the loader spend seconds of before it. `ev` is this program's endpoint,
/// with a sender queued at it [`SAMPLES`] times over; `srv` a send right on the server's.
fn measure(ev: u32, srv: u32, tpu: u64) -> (u64, u64) {
    let start = rd::time_now().expect("the clock") * tpu;
    for _ in 0..SAMPLES {
        let h = rd::endpoint_create().expect("an endpoint");
        rd::close(h).expect("its close");
    }
    for _ in 0..SAMPLES {
        let h = rd::mint_from_handle(ev, 1, None).expect("a mint");
        rd::close(h).expect("its close");
    }
    for i in 0..SAMPLES {
        start_thread(exiter, 0).expect("a thread");
        while EXITED.load(SeqCst) <= i {
            let _ = rd::receive(None, 100, 0);
        }
    }
    for _ in 0..SAMPLES {
        rd::call(srv, &rd::body([0; 4]), None, rd::FOREVER).expect("the server's reply");
    }
    for _ in 0..SAMPLES {
        match rd::receive(Some(ev), 1_000_000, 0) {
            Ok(Received::Message(_)) => {}
            other => panic!("expected a queued message, got {:?}", other.map(|_| ())),
        }
    }
    for _ in 0..SAMPLES {
        rd::time_now().expect("the clock");
        rd::usage(rd::SYSTEM).expect("system's usage");
    }
    (start, (rd::time_now().expect("the clock") + 1) * tpu)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("call-bounds");
    let ev = rd::endpoint_create().expect("this program's endpoint");
    let ev_send = rd::mint_from_handle(ev, 7, None).expect("a send right on it");
    let srv_ep = rd::endpoint_create().expect("the server's endpoint");
    let srv = rd::mint_from_handle(srv_ep, 1, None).expect("a send right on it");
    start_thread(server, srv_ep as usize).expect("the server");
    start_thread(helper, ev_send as usize).expect("the helper");
    EXITED.store(0, SeqCst);
    let empty = measure(ev, srv, b.tpu);

    // The hostile: room for its endpoints, its threads' stacks and its handle table.
    let fill = rd::create(rd::USERS, &rd::spec(4_000, 1, 100)).expect("the hostile's budget");
    let report = rd::endpoint_create().expect("the hostile's report endpoint");
    let tell = rd::mint_from_handle(report, 1, None).expect("a send right on it");
    spawn::spawn(b.image(), fill, b.exit_endpoint(), hostile as *const () as usize, &[], &[tell, ev_send])
        .expect("the hostile");
    let made = match rd::receive(Some(report), 600_000_000, 0) {
        Ok(Received::Message(m)) => m.body.words,
        _ => [0; 4],
    };
    b.note(format_args!(
        "the hostile made {} endpoints, {} senders, {} waiters and {} handles",
        made[0], made[1], made[2], made[3]
    ));
    b.check(
        made[0] >= FILL_ENDPOINTS / 2 && made[1] == SENDERS && made[2] == WAITERS && made[3] >= 3_000,
        format_args!("the hostile filled its handle table, made its endpoints and parked its threads"),
    );
    // Its threads block only once picked: let them.
    let _ = rd::receive(None, 500_000, 0);
    EXITED.store(0, SeqCst);
    let filled = measure(ev, srv, b.tpu);
    b.call_bounds(empty, filled, FACTOR, FLOOR_TICKS);
    b.finish("CALL-BOUNDS")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("call-bounds", info) }
