//! Carving moves share, never duplicates it (stride weight is free weight; kernel/scheduling.md,
//! "Running while carved down"): U (weight 200) spins and carves C (100), which spins too,
//! against a victim V of weight 200; then a nested chain. U's subtree gets at most half: the
//! victim's share is at least its weight's share of the harts, judged by the post-check on the
//! kernel's charges net of lock waits (`HART-SHARE`), and its count over what the window would
//! give one loop alone is printed beside, as the useful work the kernel's per-slice time leaves. And the
//! refusals: a carve that would leave a budget holding a process with no free weight, and a
//! process in a budget whose weight is all carved (R7).

#![no_std]
#![no_main]

use test_programs::rd::{self, Error};
use test_programs::sched::{Bench, Role, mark};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("carve");
    // (the chain's depth, the share's name, the victim's mark: no carve weighs one)
    for (depth, name, m) in [(1usize, "chain-1", 3), (4, "chain-4", 7)] {
        let u = b.budget(rd::USERS, 200, 1 + depth as u32, rd::FOREVER);
        let v = b.budget(rd::USERS, 200, 1, rd::FOREVER);
        mark(v, m);
        b.start(u, Role::Spin, &[], &[]);
        // Each spinner of the chain at its free weight, one thread each.
        let mut chain = [(0u32, 1u32); 5];
        let (mut parent, mut w) = (u, 200u32);
        for level in 0..depth {
            chain[level].0 = w - w / 2;
            w /= 2;
            // Room for this level's process and every one below it.
            let c = b.budget(parent, w, (depth - level) as u32, rd::FOREVER);
            b.start(c, Role::Spin, &[], &[]);
            parent = c;
        }
        chain[depth].0 = w;
        let vi = b.start(v, Role::Spin, &[], &[]);
        let (start, end) = b.go(50_000, WINDOW);
        let counts = b.collect(depth + 2);
        let all: u64 = counts[vi - depth - 1..=vi].iter().sum();
        b.hart_share(name, (start, end), (TOL, "+"), (m, 1), &chain[..=depth]);
        b.note(format_args!(
            "a chain of {} carves: the victim counted {} of 1000 of all counts; useful work, {} of 1000 of the window",
            depth,
            counts[vi] * 1000 / all.max(1),
            b.share(counts[vi], end - start)
        ));
        rd::destroy(u).unwrap();
        rd::destroy(v).unwrap();
    }
    // A budget holding a process may not carve away its last free weight.
    let u = b.budget(rd::USERS, 200, 2, rd::FOREVER);
    b.start(u, Role::Spin, &[], &[]);
    let all = rd::create(u, &rd::spec(1, 0, 200)).err();
    let most = rd::create(u, &rd::spec(1, 0, 199)).is_ok();
    b.check(
        all == Some(Error::InvalidArgument) && most,
        format_args!("carving all of a process-holding budget's weight -> {:?}; all but one -> ok", all),
    );
    // A budget whose weight is all carved holds no process.
    let z = b.budget(rd::USERS, 10, 1, rd::FOREVER);
    rd::create(z, &rd::spec(1, 0, 10)).unwrap();
    let r = rd::process_create(z, b.exit_endpoint()).err();
    b.check(
        r == Some(Error::InvalidArgument),
        format_args!("a process in a budget with no free weight -> {:?}", r),
    );
    b.go(0, 1);
    let _ = b.collect(1);
    b.finish("SCHED-CARVE-INFLATION")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("carve", info) }
