//! A deadline's destruction is billed, all of it, to the dying budget's parent (R10, R12): a
//! creator that floods its own budget with empty weight-0 budgets on short deadlines pays for
//! destroying them, so its share falls as the flood grows and an equal-weight victim keeps its
//! share. The program notes each count of the window, which the release case bounds at one hart,
//! and prints each victim's window for the traced twin's post-check, which judges the share on
//! the kernel's charges net of lock waits (`HART-SHARE`).

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role, mark};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("deadline-flood");
    let mut counted = [0u64; 2];
    // (deadlines a round, the share's name, the victim's mark: the flood's budgets weigh 0)
    for (k, (n, name, m)) in [(16u64, "16-deadlines", 2), (64, "64-deadlines", 3)].into_iter().enumerate() {
        // Room for 64 budgets of its own.
        let creator = b.budget(rd::USERS, 100, 3, rd::FOREVER);
        let victim = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        mark(victim, m);
        let c = b.start(creator, Role::DeadlineFlood, &[n], &[creator]);
        let v = b.start(victim, Role::Spin, &[], &[]);
        let (start, end) = b.go(50_000, WINDOW);
        let counts = b.collect(2);
        let (cs, vs) = (b.share(counts[c], end - start), b.share(counts[v], end - start));
        counted[k] = counts[c];
        b.note(format_args!("{} deadlines a round: the creator got {} of 1000, the victim {}", n, cs, vs));
        b.hart_share(name, (start, end), (TOL, "+"), (m, 1), &[(100, 1)]);
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
