//! A destroyed lineage's debt reaches a shared parent normalized by weight (kernel/scheduling.md,
//! "Inheritance"): under `users`, sixteen spinners of weight 100 run; U (100) spins with a
//! weight-1 grandchild G that runs a slice; G is destroyed, then U (a logout). A sibling S created
//! under `users` afterwards is not held back by G's raw debt: the bench's oracle judges S's
//! first run from the trace (`round`). This
//! program destroys an empty marker budget just before it makes S, and S, the first budget to
//! wake after the marker, must be picked before any other budget is picked twice; G's debt,
//! unlifted, would cost it about six rounds. The one-round bound itself rests on the lift's
//! arithmetic, which the oracle recomputes at every lift: under `users` the lift is a few
//! thousandths of a slice, so no boot exercises that bound (kernel/scheduling.md, "Residual
//! risks"). S's wait in µs is printed as a note.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const N: u64 = 16;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("debt-lift");
    for _ in 0..N {
        let s = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(s, Role::Spin, &[], &[]);
    }
    let u = b.budget(rd::USERS, 100, 2, rd::FOREVER);
    b.start(u, Role::Spin, &[], &[]);
    let g = b.budget(u, 1, 1, rd::FOREVER);
    b.start(g, Role::Spin, &[], &[]);
    let (_, end) = b.go(50_000, 3_000_000);
    // Let G run its slice, then end the lineage.
    let _ = rd::receive(None, 500_000, 0);
    rd::destroy(g).unwrap();
    rd::destroy(u).unwrap();
    // The oracle's marker: an empty budget, never woken, destroyed just before S is made.
    let marker = rd::create(rd::SYSTEM, &rd::spec(1, 0, 0)).unwrap();
    rd::destroy(marker).unwrap();
    let s = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    b.start(s, Role::Probe, &[], &[]);
    let created = b.now_us();
    b.go(0, end.saturating_sub(created));
    let counts = b.collect(N as usize + 1);
    let first = counts[N as usize + 2];
    b.check(first != 0, format_args!("the sibling ran (its round is the oracle's)"));
    b.note(format_args!("the sibling first ran {} us after creation", first.saturating_sub(created)));
    b.finish("SCHED-DEBT-LIFT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("debt-lift", info) }
