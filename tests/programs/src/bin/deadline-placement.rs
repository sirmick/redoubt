//! A deadline's destruction lands on the hart that runs the budget paying for it (kernel/timer.md,
//! "The hart timer"): a creator floods its own budget with empty weight-0 budgets whose deadlines
//! fall 600 µs ahead, after its last creation, and then only spins, so each is destroyed at a
//! timer interrupt and never at one of the creator's own entries; an equal-weight victim spins
//! beside it on the other hart. The traced twin's post-check judges the victim's share on the
//! kernel's charges (`HART-SHARE`) and bounds the timer work billed to others in the victim's
//! time; a kernel built with `timer-any-hart`, every hart's timer coming by every deadline, lands
//! a share of the destructions on the victim's hart and fails that bound.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("deadline-placement");
    // Room for 16 budgets of its own; their deadlines 600 µs ahead, past the round's creations.
    let creator = b.budget(rd::USERS, 100, 3, rd::FOREVER);
    let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    mark(victim, 2);
    let c = b.start(creator, Role::DeadlineFlood, &[16, 600], &[creator]);
    let v = b.start(victim, Role::Spin, &[], &[]);
    let (start, end) = b.go(50_000, WINDOW);
    let counts = b.collect(2);
    let (cs, vs) = (b.share(counts[c], end - start), b.share(counts[v], end - start));
    b.note(format_args!(
        "16 deadlines a round, 600 µs ahead: the creator got {} of 1000, the victim {}",
        cs, vs
    ));
    b.hart_share("placement", (start, end), (TOL, "+"), (2, 1), &[(100, 1)]);
    rd::destroy(creator).unwrap();
    rd::destroy(victim).unwrap();
    b.finish("DEADLINE-PLACEMENT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("deadline-placement", info) }
