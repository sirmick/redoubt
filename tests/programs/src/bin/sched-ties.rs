//! Ties in the stride queue (kernel/scheduling.md, "The current minimum and ties"), in a kernel
//! built with `sched-trace`: the bench's independent oracle (`tools/testbench`, `sched_oracle`)
//! checks every pick of the whole run against the four clauses. These checks, from the programs'
//! own first-run times, are a cheap second line (and catch a trace that lost its meaning).
//!
//! Six processes, each in a budget of its own at one weight, created in order (so ids ascend):
//! group B (b1, b2, b3) waits in `receive`, each on an endpoint of its own; group A (a1, a2, a3)
//! waits in `send` through handles stamped with one small budget X. A spinner then runs alone
//! long enough to lift the floor above all six own passes. This program then sends to b1, b2 and
//! b3 (three system calls, three kernel entries) and destroys X last (one call: every A send fails
//! `Dead` in one entry). All six wake at the floor, an equal pass, so ties decide:
//! - within one entry the lower id first: a1, a2, a3 (clause 3);
//! - a later entry's wakers first: A (woken last) before B, and b3, b2, b1 (clause 2).
//!
//! The expected order is a1, a2, a3, b3, b2, b1. The wakes come in cheap calls first and the
//! slow one (a destruction) last, so a slice ending during the destruction changes nothing.
//!
//! Wakers ahead of a requeued budget (clause 1) and requeues in order (clause 4) need two budgets
//! at exactly one pass, which the kernel's tick-exact charging makes a coincidence: the oracle's
//! host tests, the model's traces and the fault-injection run cover them.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const G: usize = 3;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("ties");
    let x = rd::create(rd::SYSTEM, &rd::spec(1, 0, 0)).unwrap();
    let stamped = rd::mint_from_handle(rd::endpoint_create().unwrap(), 1, Some(x)).unwrap();
    let (mut bs, mut to_b, mut a_s) = ([0usize; G], [0u32; G], [0usize; G]);
    for i in 0..G {
        let ep = rd::endpoint_create().unwrap();
        to_b[i] = rd::mint_from_handle(ep, 1, None).unwrap();
        let budget = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        bs[i] = b.start(budget, Role::TieReceiver, &[], &[ep]);
    }
    for slot in a_s.iter_mut() {
        let budget = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        *slot = b.start(budget, Role::TieSender, &[], &[stamped]);
    }
    let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    b.start(spinner, Role::Spin, &[], &[]);
    let (start, _) = b.go(20_000, 400_000);
    // The six block at once; the spinner runs alone for ten slices.
    let _ = rd::receive(None, (start + 100_000).saturating_sub(b.now_us()), 0);
    let mut sent = true;
    for h in to_b {
        sent &= rd::send(h, &rd::body([0; 4]), None, rd::FOREVER).is_ok();
    }
    let destroyed = rd::destroy(x);
    let first = b.collect(2 * G + 1);
    let (ta, tb) = (a_s.map(|i| first[i]), bs.map(|i| first[i]));
    b.note(format_args!("first runs (rdtime): A {:?}, B {:?}", ta, tb));
    b.check(
        sent && destroyed.is_ok() && ta.iter().chain(tb.iter()).all(|t| *t != 0),
        format_args!("B woken by three sends, A by one destroy ({:?}): all six woke", destroyed),
    );
    b.check(
        ta.windows(2).all(|w| w[0] < w[1]),
        format_args!("wakers of one entry ran lowest id first (clause 3)"),
    );
    b.check(
        tb.windows(2).all(|w| w[0] > w[1]) && ta[G - 1] < tb[G - 1],
        format_args!("a later entry's wakers ran first (clause 2)"),
    );
    b.finish("SCHED-TIES")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("ties", info) }
