//! Timeouts on the kernel's own timer (WP-K5; KERNEL-SPEC.md, I13 and IPC completion): every
//! blocking call returns by its timeout, with the ownership the completion table gives, and a
//! reply racing a timeout lands on exactly one side.
//!
//! This program is the bundle's first, so it holds the device objects (one of which is an
//! interrupt that never fires here); `log-server` runs beside it and is idle while this program
//! sleeps, so nothing but the timer can end a sleep.
//!
//! Lateness is how long after its deadline a sleep returned. It is asserted (at most
//! `LATE_US`) only where the case runs in virtual time (`icount`); the `-tcg` variant of the case
//! reports it without judging, because host load shows up as guest latency there.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use redoubt_sys::{Call, LendDisposition, ReplyOutcome, Return};
use test_programs::rd::{self, Error, MessageKind, Received};
use test_programs::{Logger, log};

/// The most a sleep may return after its deadline, in virtual time: a timer interrupt, one
/// expiry walk and a switch back, with room to spare.
const LATE_US: u64 = 1_000;

fn now() -> u64 { rd::time_now().expect("time_now") }

fn random(below: u64) -> u64 { rd::random().expect("random") % below.max(1) }

fn ok(b: bool) -> &'static str { if b { "ok" } else { "FAIL" } }

/// `reply` with its outcome.
fn reply(msg_id: u64) -> Result<ReplyOutcome, Error> {
    let rec = rd::body([0; rd::WORDS]).encode();
    let id = core::num::NonZeroU64::new(msg_id).ok_or(Error::InvalidArgument)?;
    match redoubt_sys::syscall(&Call::Reply { msg_id: id, body_rec: rec.as_ptr() as usize })? {
        Return::Reply(o) => Ok(o),
        _ => Err(Error::InvalidArgument),
    }
}

/// The endpoint the server thread receives on, and what it is told to do per call.
static EP: AtomicUsize = AtomicUsize::new(0);
/// How long the server holds each call before replying, in µs.
static HOLD: AtomicUsize = AtomicUsize::new(0);
/// Results of the server's last round: 1 delivered; 2 discarded after its one notice; 4 discarded,
/// the timeout having landed after the server looked; 3 anything else.
static SERVED: AtomicUsize = AtomicUsize::new(0);
/// Bumped by the server after each round.
static ROUNDS: AtomicUsize = AtomicUsize::new(0);

/// The server thread: take a call, hold it, look for an abandoned-call notice, reply.
extern "C" fn server(_: usize) -> ! {
    let ep = EP.load(SeqCst) as u32;
    loop {
        let Ok(Received::Message(m)) = rd::receive(Some(ep), rd::FOREVER, 0) else { continue };
        let MessageKind::Call { .. } = m.kind else { continue };
        let hold = HOLD.load(SeqCst) as u64;
        if hold > 0 {
            let _ = rd::receive(None, hold, 0);
        }
        // A caller that timed out abandoned the call: its notice comes once, if the server looks
        // before replying (the timeout may also land between that look and the reply: then the
        // reply is discarded with no notice seen). After the reply, nothing more comes.
        let first = rd::receive(Some(ep), 0, 0);
        let second = if matches!(first, Ok(Received::Abandoned(_))) {
            rd::receive(Some(ep), 0, 0)
        } else {
            Err(Error::Timeout)
        };
        let r = reply(m.msg_id.get());
        let after = rd::receive(Some(ep), 0, 0);
        let notice = matches!(first, Ok(Received::Abandoned(id)) if id == m.msg_id);
        let quiet = second == Err(Error::Timeout) && after == Err(Error::Timeout);
        let verdict = match (r, notice, quiet, first) {
            (Ok(o), false, true, Err(Error::Timeout)) if o.delivered => 1,
            (Ok(o), true, true, _) if !o.delivered && o.installed == 0 => 2,
            (Ok(o), false, true, Err(Error::Timeout)) if !o.delivered && o.installed == 0 => 4,
            _ => 3,
        };
        SERVED.store(verdict, SeqCst);
        ROUNDS.fetch_add(1, SeqCst);
    }
}

/// Wait (by sleeping) until the server has finished round `n`.
fn await_round(n: usize) {
    while ROUNDS.load(SeqCst) < n {
        let _ = rd::receive(None, 100, 0);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::log_rx();
    let mut logger = test_programs::logsrv::start();

    // Idle sleeps: nothing else runs, so only the kernel's timer can end them.
    let mut worst = 0;
    let mut early = false;
    for _ in 0..20 {
        let timeout = 1_000 + random(20_000);
        let before = now();
        let r = rd::receive(None, timeout, 0);
        let after = now();
        early |= after < before + timeout || r != Err(Error::Timeout);
        worst = worst.max(after.saturating_sub(before + timeout));
    }
    log!(logger, "[timeouts] {}: 20 idle sleeps returned Timeout, none early", ok(!early));
    log!(
        logger,
        "[timeouts] lateness {} ({} µs worst, bound {} µs)",
        if worst <= LATE_US { "ok" } else { "LATE" },
        worst,
        LATE_US
    );

    // A queued call and a queued send time out with their buffers returned: nobody receives.
    let ep = rd::endpoint_create().unwrap();
    let h = rd::mint_from_handle(ep, 9, None).unwrap();
    let lend = rd::page();
    let before = now();
    let (outcome, reply) = rd::call_outcome(h, &rd::body([1, 2, 3, 4]), rd::pages(lend, 1), 5_000).unwrap();
    let waited = now() - before;
    let good = outcome.status == Err(Error::Timeout)
        && outcome.lend == LendDisposition::Returned
        && !outcome.reply_present
        && reply.is_none()
        && waited >= 5_000;
    rd::poke(lend, 7); // still ours
    log!(logger, "[timeouts] {}: a queued call timed out after {} µs, lend returned", ok(good), waited);
    let before = now();
    let sent = rd::send(h, &rd::body([5, 6, 7, 8]), rd::pages(lend, 1), 5_000);
    let waited = now() - before;
    let good = sent == Err(Error::Timeout) && waited >= 5_000 && rd::peek(lend) == 7;
    log!(logger, "[timeouts] {}: a queued send timed out after {} µs, transfer returned", ok(good), waited);

    // Receive on an endpoint nothing arrives on, and on an interrupt that never fires.
    let quiet_ep = rd::endpoint_create().unwrap();
    let r = rd::receive(Some(quiet_ep), 3_000, 0);
    let irq = devices
        .map(|d| (d, rd::receive(Some(d), 1_000, 0)))
        .find(|(_, r)| *r == Err(Error::Timeout));
    log!(
        logger,
        "[timeouts] {}: receive timed out on an endpoint ({:?}) and on an interrupt (handle {:?})",
        ok(r == Err(Error::Timeout) && irq.is_some()),
        r,
        irq.map(|x| x.0)
    );

    // The server thread, for calls it takes.
    EP.store(ep as usize, SeqCst);
    let stack = rd::map_anon(8 * rd::PAGE_SIZE, rd::rw()).unwrap();
    rd::thread_create(server as *const () as usize, stack + 8 * rd::PAGE_SIZE - 16, 0).unwrap();

    // A taken call times out: the lend is consumed, the server gets one notice and its reply is
    // discarded.
    HOLD.store(20_000, SeqCst);
    let lend = rd::page();
    let (outcome, _) = rd::call_outcome(h, &rd::body([1, 0, 0, 0]), rd::pages(lend, 1), 5_000).unwrap();
    await_round(1);
    let good = outcome.status == Err(Error::Timeout)
        && outcome.lend == LendDisposition::Consumed
        && !outcome.reply_present
        && SERVED.load(SeqCst) == 2;
    log!(
        logger,
        "[timeouts] {}: a taken call timed out, lend consumed, one notice, reply discarded ({:?})",
        ok(good),
        outcome
    );

    // FOREVER never expires, nor does a timeout that saturates the clock: both calls end only
    // because the server replies, 30 ms later.
    let mut rounds = 1;
    for timeout in [rd::FOREVER, u64::MAX - 1, u64::MAX - now()] {
        HOLD.store(30_000, SeqCst);
        let before = now();
        let (outcome, _) = rd::call_outcome(h, &rd::body([2, 0, 0, 0]), None, timeout).unwrap();
        rounds += 1;
        await_round(rounds);
        let good = outcome.status == Ok(()) && now() - before >= 30_000 && SERVED.load(SeqCst) == 1;
        log!(logger, "[timeouts] {}: timeout {:#x} did not expire; the reply came", ok(good), timeout);
    }

    // A reply racing the timeout, 200 times, at random offsets either side of it: each lands on
    // exactly one side, the server's disposition agreeing with the caller's outcome, and both
    // sides happen.
    let (mut replied, mut timed_out, mut bad) = (0, 0, 0);
    let mut first_bad = None;
    for _ in 0..200 {
        // Wide enough either side that the server's own round trip (a few kernel entries, each
        // walking every thread) cannot push every reply past the deadline.
        let timeout = 20_000;
        let hold = timeout - 10_000 + random(20_000);
        HOLD.store(hold as usize, SeqCst);
        let lend = rd::page();
        let (outcome, _) = rd::call_outcome(h, &rd::body([3, 0, 0, 0]), rd::pages(lend, 1), timeout).unwrap();
        rounds += 1;
        await_round(rounds);
        match (outcome.status, outcome.lend, outcome.reply_present, SERVED.load(SeqCst)) {
            (Ok(()), LendDisposition::Returned, true, 1) => replied += 1,
            (Err(Error::Timeout), LendDisposition::Consumed, false, 2 | 4) => timed_out += 1,
            x => {
                bad += 1;
                first_bad.get_or_insert(x);
            }
        }
    }
    log!(
        logger,
        "[timeouts] {}: reply race: {} replied, {} timed out, {} inconsistent",
        ok(bad == 0 && replied > 0 && timed_out > 0),
        replied,
        timed_out,
        bad
    );
    if let Some(x) = first_bad {
        log!(logger, "[timeouts] first inconsistent round: {:?}", x);
    }

    // rdtime from user mode: monotonic, and in step with time_now.
    let (t0, u0) = (test_programs::read_time(), now());
    let mut last = t0;
    let mut monotonic = true;
    for _ in 0..1000 {
        let t = test_programs::read_time();
        monotonic &= t >= last;
        last = t;
    }
    let _ = rd::receive(None, 20_000, 0);
    let (t1, u1) = (test_programs::read_time(), now());
    let _ = rd::receive(None, 20_000, 0);
    let (t2, u2) = (test_programs::read_time(), now());
    // Ticks per ms over two intervals agree within 1%.
    let (r1, r2) = ((t1 - t0) * 1000 / (u1 - u0).max(1), (t2 - t1) * 1000 / (u2 - u1).max(1));
    let linear = r1.abs_diff(r2) * 100 <= r1.max(1);
    log!(
        logger,
        "[timeouts] {}: rdtime is monotonic and linear in time_now ({} and {} ticks per ms)",
        ok(monotonic && linear && t2 > t0),
        r1,
        r2
    );

    log!(logger, "TIMEOUTS TEST PASSED");
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[timeouts] FAIL: panic: {}", info);
    test_programs::park()
}
