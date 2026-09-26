//! A destroyed lineage's debt reaches a shared parent normalized by weight (kernel/scheduling.md,
//! "Inheritance"): under `users`, sixteen spinners of weight 100 run; U (100) spins with a
//! weight-1 grandchild G that runs a slice; G is destroyed, then U (a logout). A sibling S created
//! under `users` afterwards runs within one round (kernel/scheduling.md, "Residual risks"), not
//! after G's raw debt.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, SLICE_US};

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
    let s = b.budget(rd::USERS, 100, 1, rd::FOREVER);
    b.start(s, Role::Probe, &[], &[]);
    let created = b.now_us();
    b.go(0, end.saturating_sub(created));
    let counts = b.collect(N as usize + 1);
    let first = counts[N as usize + 2];
    let waited = first.saturating_sub(created);
    let bound = (N + 1 + 2) * SLICE_US;
    b.check(
        first != 0 && waited <= bound,
        format_args!("the sibling first ran {} us after creation (one round is {} us)", waited, bound),
    );
    b.finish("SCHED-DEBT-LIFT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("debt-lift", info) }
