//! A slice is the thread's own time (R12, kernel/scheduling.md, "Preemption points"): an expiry
//! that a timer interrupt runs inside a thread's slice, for waits of another thread's, pauses the
//! slice, which resumes with what it had, so the interrupted thread keeps its turn among its
//! budget's threads.
//!
//! One budget, two processes. The victim's two threads, T (its main thread) and C, count for the
//! window. The attacker, a sibling process, has [`WAITERS`] threads waiting on one shared deadline,
//! which the first of them to run after it passed sets [`LEAD`] µs ahead, the time all of them take
//! to re-arm and a little more. The round-robin within the budget runs the threads in (pid, tid)
//! order from its cursor, wrapping, so after the attacker's threads it always runs T, and every
//! deadline falls due inside T's slice. A slice the expiry could end would end there, and the
//! cursor would move on to C for a whole slice: T would count less than C, every round. With the
//! slice paused, T and C count alike.
//!
//! The attacker sits in the victim's budget because across budgets stride evens the loss: a slice
//! cut short is charged only what it ran, so its budget stays first in the queue, and which thread
//! the waits fall on is the queue's turn order, not the attacker's.
//!
//! Judged here, on the victim's own counts: T's share of what the two counted at least its
//! round-robin share, half, less [`MARGIN`]. The post-check requires the deadlines to have come,
//! at least the case's floor of timer interrupts ending items in the budget's time
//! (`expiries_in`), and checks that each timer interrupt billed only the budgets of what it found.
//! That the attacker pays for every expiry cannot be told apart in one budget; that check stands
//! in for it.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark};

const WINDOW: u64 = 2_000_000;
/// The attacker's threads: as many as one process's stacks hold.
const WAITERS: u64 = 31;
/// How far ahead the attacker sets its deadline, µs: of 800 to 2,500 µs measured, the lead at which
/// an expiry that ended T's slice would cost T the most.
const LEAD: u64 = 1_200;
/// T's share of the two counts, per mille, may fall short of half by this much: twice the most it
/// fell short with the slice paused, 8 at any lead of 800 to 2,500 µs on either width, rounded up.
const MARGIN: u64 = 20;
/// The budget's mark weight, for the post-check.
const MARK: u32 = 2;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("slice-own-time");
    let budget = b.budget(rd::USERS, 100, 2, rd::FOREVER);
    mark(budget, MARK);
    let attacker = b.start(budget, Role::DueWaits, &[WAITERS, LEAD], &[]);
    let victim = b.start(budget, Role::CountPair, &[], &[]);
    b.go(50_000, WINDOW);
    let (mut counts, mut deadlines, mut got) = ([None; 2], None, 0);
    while got < 3 {
        let Some((kid, w)) = b.receive_report() else { break };
        let value = w[0] as u32 as u64 | (w[1] as u32 as u64) << 32;
        if kid == victim && w[2] < 2 {
            counts[w[2]] = Some(value);
        } else if kid == attacker {
            deadlines = Some(value);
        }
        got += 1;
    }
    let (Some(t), Some(c), Some(deadlines)) = (counts[0], counts[1], deadlines) else {
        b.check(false, format_args!("the victim's two counts and the attacker's report came"));
        b.finish("SCHED-SLICE-OWN-TIME")
    };
    let share = t * 1000 / (t + c).max(1);
    b.note(format_args!("the attacker set {deadlines} deadlines, {WAITERS} waits due at each"));
    b.check(
        share + MARGIN >= 500,
        format_args!(
            "T counted {share} of 1000 of the two threads' counts (T {t}, C {c}), at least {}",
            500 - MARGIN
        ),
    );
    while rd::receive(Some(b.exit_endpoint()), 0, 0).is_ok() {}
    rd::destroy(budget).unwrap();
    b.finish("SCHED-SLICE-OWN-TIME")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("slice-own-time", info) }
