//! Ties in the stride queue (kernel/scheduling.md, "The current minimum and ties"), in a kernel
//! built with `sched-trace`: the bench's independent oracle (`tools/testbench`, `sched_oracle`)
//! checks every pick of the whole run against the four clauses. These checks, from the programs'
//! own first-run times, are a cheap second line (and catch a trace that lost its meaning).
//!
//! Six processes, each in a budget of its own at one weight, created in order (so ids ascend):
//! group B (b1, b2, b3) waits in `send` through handles stamped with one small budget Y, and group
//! A (a1, a2, a3) through handles stamped with another, X. A spinner then runs alone long enough
//! to lift the floor above all six own passes. This program then destroys Y (one call: every B
//! send fails `Dead` in one kernel entry) and then X (A's three, in a later entry). Each group's
//! three wake in one entry by construction, at the floor, an equal pass, so within a group the
//! lower id is picked first (clause 3): b1, b2, b3 and a1, a2, a3, under any slice. On one hart
//! each runs before the next is picked; on two the first two are picked onto the two harts and
//! may return to user mode in either order, and the third waits for a hart, so the program checks
//! that the group's last ran after the others. The oracle checks every pick on any number of
//! harts.
//!
//! A later entry's wakers rank first (clause 2: A before B) only while both groups are queued
//! together, and nothing keeps this program on the CPU from one destruction to the next: a slice
//! end between them may run B first, which no rule forbids. So clause 2 is the oracle's claim
//! alone, checked on whatever the trace records; the program prints the order it saw as a note.
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
    let y = rd::create(rd::SYSTEM, &rd::spec(1, 0, 0)).unwrap();
    let stamped_x = rd::mint_from_handle(rd::endpoint_create().unwrap(), 1, Some(x)).unwrap();
    let stamped_y = rd::mint_from_handle(rd::endpoint_create().unwrap(), 1, Some(y)).unwrap();
    let (mut bs, mut a_s) = ([0usize; G], [0usize; G]);
    for slot in bs.iter_mut() {
        let budget = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        *slot = b.start(budget, Role::TieSender, &[], &[stamped_y]);
    }
    for slot in a_s.iter_mut() {
        let budget = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        *slot = b.start(budget, Role::TieSender, &[], &[stamped_x]);
    }
    let spinner = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    b.start(spinner, Role::Spin, &[], &[]);
    let (start, _) = b.go(20_000, 400_000);
    // The six block at once; the spinner runs alone for a hundred slices.
    let _ = rd::receive(None, (start + 100_000).saturating_sub(b.now_us()), 0);
    let destroyed = (rd::destroy(y), rd::destroy(x));
    let first = b.collect(2 * G + 1);
    let (ta, tb) = (a_s.map(|i| first[i]), bs.map(|i| first[i]));
    b.note(format_args!("first runs (rdtime): A {:?}, B {:?}", ta, tb));
    b.check(
        destroyed.0.is_ok() && destroyed.1.is_ok() && ta.iter().chain(tb.iter()).all(|t| *t != 0),
        format_args!("B woken by one destroy, A by another ({:?}): all six woke", destroyed),
    );
    let last_ran_last = |t: [u64; G]| t[..G - 1].iter().all(|x| *x < t[G - 1]);
    b.check(
        last_ran_last(tb) && last_ran_last(ta),
        format_args!("wakers of one entry ran highest id last, in each group (clause 3)"),
    );
    b.note(format_args!(
        "the later entry's group ran first: {} (clause 2 is the oracle's alone)",
        ta[G - 1] < tb[0]
    ));
    b.finish("SCHED-TIES")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("ties", info) }
