//! A deadline's destruction is billed, all of it, to the dying budget's parent (R10, R12): a
//! creator that floods its own budget with empty weight-0 budgets on short deadlines pays for
//! destroying them, so its share falls as the flood grows and an equal-weight victim keeps half.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("deadline-flood");
    let mut counted = [0u64; 2];
    for (k, n) in [16u64, 64].into_iter().enumerate() {
        // Room for 64 budgets of its own.
        let creator = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        let c = b.start(creator, Role::DeadlineFlood, &[n], &[creator]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let (start, end) = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        let (cs, vs) = (b.share(counts[c], end - start), b.share(counts[v], end - start));
        counted[k] = counts[c];
        b.note(format_args!("{} deadlines a round: the creator got {} of 1000, the victim {}", n, cs, vs));
        b.check(vs + TOL >= 500, format_args!("{} deadlines a round: the victim got {} of 1000", n, vs));
        rd::destroy(creator).unwrap();
        rd::destroy(victim).unwrap();
    }
    b.check(
        counted[1] < counted[0],
        format_args!("the creator's count falls with the flood: {} then {}", counted[0], counted[1]),
    );
    b.finish("DEADLINE-FLOOD-BILLED")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("deadline-flood", info) }
